use super::decode::{AudioError, check_duration, sample};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
};
use tokio_util::sync::CancellationToken;

pub(super) struct Wav {
    input: BufReader<File>,
    remaining: u64,
    pub rate: u32,
    bytes_per_sample: usize,
    bytes_per_frame: usize,
    float: bool,
    buffer: Vec<u8>,
}

fn u16le(bytes: &[u8]) -> u16 {
    u16::from_le_bytes(bytes[..2].try_into().unwrap())
}
fn u32le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().unwrap())
}

impl Wav {
    pub fn open(file: File, size: u64, stop: &CancellationToken) -> Result<Self, AudioError> {
        let mut input = BufReader::new(file);
        let mut riff = [0; 12];
        input
            .read_exact(&mut riff)
            .map_err(|_| AudioError::Invalid)?;
        if &riff[..4] != b"RIFF" || &riff[8..] != b"WAVE" {
            return Err(AudioError::Unsupported);
        }
        let end = 8 + u64::from(u32le(&riff[4..]));
        if end > size {
            return Err(AudioError::Invalid);
        }
        let mut format = None;
        let mut data = None;
        let mut offset = 12;
        while offset + 8 <= end {
            if stop.is_cancelled() {
                return Err(AudioError::Stopped);
            }
            let mut header = [0; 8];
            input
                .read_exact(&mut header)
                .map_err(|_| AudioError::Invalid)?;
            let len = u64::from(u32le(&header[4..]));
            let start = offset + 8;
            if start + len > end {
                return Err(AudioError::Invalid);
            }
            match &header[..4] {
                b"fmt " if format.is_none() => {
                    if len < 16 {
                        return Err(AudioError::Invalid);
                    }
                    let mut fmt = [0; 40];
                    input
                        .read_exact(&mut fmt[..len.min(40) as usize])
                        .map_err(|_| AudioError::Invalid)?;
                    let mut tag = u16le(&fmt);
                    if tag == 0xfffe {
                        // KSDATAFORMAT_SUBTYPE_{PCM,IEEE_FLOAT}: only Data1 varies.
                        if len < 40
                            || u16le(&fmt[16..]) < 22
                            || &fmt[26..] != b"\0\0\0\0\x10\0\x80\0\0\xaa\0\x38\x9b\x71"
                        {
                            return Err(AudioError::Unsupported);
                        }
                        tag = u16le(&fmt[24..]);
                        if u16le(&fmt[18..]) > u16le(&fmt[14..]) {
                            return Err(AudioError::Invalid);
                        }
                    }
                    let channels = usize::from(u16le(&fmt[2..]));
                    let rate = u32le(&fmt[4..]);
                    let bits = u16le(&fmt[14..]);
                    let float = tag == 3;
                    if !matches!((tag, bits), (1, 8 | 16 | 24 | 32) | (3, 32)) {
                        return Err(AudioError::Unsupported);
                    }
                    let width = usize::from(bits / 8);
                    let frame = channels * width;
                    if rate == 0 || frame == 0 || frame != usize::from(u16le(&fmt[12..])) {
                        return Err(AudioError::Invalid);
                    }
                    format = Some((rate, width, frame, float));
                }
                b"data" if data.is_none() => data = Some((start, len)),
                _ => (),
            }
            if format.is_some() && data.is_some() {
                break;
            }
            offset = start + len + (len & 1);
            input
                .seek(SeekFrom::Start(offset))
                .map_err(|_| AudioError::Read)?;
        }
        let (rate, bytes_per_sample, bytes_per_frame, float) = format.ok_or(AudioError::Invalid)?;
        let (start, remaining) = data.ok_or(AudioError::Invalid)?;
        if !remaining.is_multiple_of(bytes_per_frame as u64) {
            return Err(AudioError::Invalid);
        }
        check_duration(remaining / bytes_per_frame as u64, rate)?;
        input
            .seek(SeekFrom::Start(start))
            .map_err(|_| AudioError::Read)?;
        Ok(Self {
            input,
            remaining,
            rate,
            bytes_per_sample,
            bytes_per_frame,
            float,
            buffer: vec![0; (64 * 1024 / bytes_per_frame).min(1024) * bytes_per_frame],
        })
    }

    pub fn duration_ms(&self) -> u64 {
        self.remaining / self.bytes_per_frame as u64 * 1000 / u64::from(self.rate)
    }

    pub fn read(&mut self, mono: &mut Vec<f32>) -> Result<bool, AudioError> {
        let n = self.remaining.min(self.buffer.len() as u64) as usize;
        if n == 0 {
            return Ok(false);
        }
        self.input
            .read_exact(&mut self.buffer[..n])
            .map_err(|_| AudioError::Read)?;
        self.remaining -= n as u64;
        let channels = (self.bytes_per_frame / self.bytes_per_sample) as f32;
        mono.extend(
            self.buffer[..n]
                .chunks_exact(self.bytes_per_frame)
                .map(|frame| {
                    frame
                        .chunks_exact(self.bytes_per_sample)
                        .map(|bytes| match bytes {
                            [b] => (f32::from(*b) - 128.0) / 128.0,
                            [a, b] => f32::from(i16::from_le_bytes([*a, *b])) / 32768.0,
                            [a, b, c] => i32::from_le_bytes([0, *a, *b, *c]) as f32 / 2147483648.0,
                            [a, b, c, d] if self.float => {
                                sample(f32::from_le_bytes([*a, *b, *c, *d]))
                            }
                            [a, b, c, d] => {
                                i32::from_le_bytes([*a, *b, *c, *d]) as f32 / 2147483648.0
                            }
                            _ => unreachable!("validated sample width"),
                        })
                        .sum::<f32>()
                        / channels
                }),
        );
        Ok(true)
    }
}
