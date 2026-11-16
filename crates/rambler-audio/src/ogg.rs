use crate::{AudioError, MAX_PENDING_BYTES};
use bytes::Bytes;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU32, Ordering},
};

static SERIAL: AtomicU32 = AtomicU32::new(1);
const MAX_PACKET: usize = 1275;

/// RFC 3533/7845 framing only. Native libopus produces the encoded packets.
/// One packet is at most 1275 bytes; <=50 packets/page plus a <=255-entry
/// lacing table keep a complete audio page below 64KiB. A 255-byte multiple
/// requires a terminating zero lace. Headers are separate complete pages.
pub(crate) struct Ogg {
    serial: u32,
    sequence: u32,
    packet_limit: usize,
    packets: usize,
    lacing: Vec<u8>,
    body: Vec<u8>,
    granule: u64,
    pub output: VecDeque<Bytes>,
    output_bytes: usize,
}
impl Ogg {
    pub fn new(page_duration_ms: u16, pre_skip: u16) -> Result<Self, AudioError> {
        let mut out = Self {
            serial: SERIAL.fetch_add(1, Ordering::Relaxed),
            sequence: 0,
            packet_limit: usize::from(page_duration_ms).div_ceil(20),
            packets: 0,
            lacing: vec![],
            body: vec![],
            granule: 0,
            output: VecDeque::new(),
            output_bytes: 0,
        };
        let mut head = b"OpusHead".to_vec();
        head.extend([1, 1]);
        head.extend(pre_skip.to_le_bytes());
        head.extend(16000_u32.to_le_bytes());
        head.extend([0, 0, 0]);
        out.page(2, 0, &[head.len() as u8], &head)?;
        let vendor = b"rambler-audio";
        let mut tags = b"OpusTags".to_vec();
        tags.extend((vendor.len() as u32).to_le_bytes());
        tags.extend(vendor);
        tags.extend(0_u32.to_le_bytes());
        out.page(0, 0, &[tags.len() as u8], &tags)?;
        Ok(out)
    }
    pub fn packet(&mut self, packet: &[u8], granule: u64, eos: bool) -> Result<(), AudioError> {
        if packet.is_empty() || packet.len() > MAX_PACKET {
            return Err(AudioError::OutputLimit);
        }
        if self.lacing.len() + packet.len() / 255 + 1 > 255 {
            self.flush(false)?;
        }
        let mut remaining = packet.len();
        while remaining >= 255 {
            self.lacing.push(255);
            remaining -= 255;
        }
        self.lacing.push(remaining as u8);
        self.body.extend(packet);
        self.packets += 1;
        self.granule = granule;
        if self.packets >= self.packet_limit || eos {
            self.flush(eos)?;
        }
        Ok(())
    }
    fn flush(&mut self, eos: bool) -> Result<(), AudioError> {
        let body = std::mem::take(&mut self.body);
        let lacing = std::mem::take(&mut self.lacing);
        self.page(if eos { 4 } else { 0 }, self.granule, &lacing, &body)?;
        self.packets = 0;
        Ok(())
    }
    fn page(
        &mut self,
        flags: u8,
        granule: u64,
        lacing: &[u8],
        body: &[u8],
    ) -> Result<(), AudioError> {
        let size = 27 + lacing.len() + body.len();
        if lacing.len() > 255
            || self
                .output_bytes
                .checked_add(size)
                .is_none_or(|s| s > MAX_PENDING_BYTES)
        {
            return Err(AudioError::OutputLimit);
        }
        let sequence = self.sequence;
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(AudioError::CounterOverflow)?;
        let mut page = Vec::with_capacity(size);
        page.extend(b"OggS");
        page.extend([0, flags]);
        page.extend(granule.to_le_bytes());
        page.extend(self.serial.to_le_bytes());
        page.extend(sequence.to_le_bytes());
        page.extend([0; 4]);
        page.push(lacing.len() as u8);
        page.extend(lacing);
        page.extend(body);
        let checksum = crc(&page);
        page[22..26].copy_from_slice(&checksum.to_le_bytes());
        self.output_bytes += page.len();
        self.output.push_back(page.into());
        Ok(())
    }
    pub fn pop(&mut self) -> Option<Bytes> {
        let out = self.output.pop_front()?;
        self.output_bytes -= out.len();
        Some(out)
    }
}
fn crc(bytes: &[u8]) -> u32 {
    let mut crc = 0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
#[path = "ogg_tests.rs"]
mod tests;
