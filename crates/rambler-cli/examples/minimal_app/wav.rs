//! Bounded reader for conventional, uncompressed mono PCM16LE RIFF/WAVE.
use std::{io, path::Path};
use tokio::{fs::File, io::AsyncReadExt};

const MAX_WAV_BYTES: usize = 3_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Rate {
    Khz16,
    Khz48,
}

pub(super) struct Wav<'a> {
    pub rate: Rate,
    pub pcm: &'a [u8],
}

pub(super) async fn read(path: &Path) -> io::Result<Vec<u8>> {
    let mut wav = Vec::new();
    File::open(path)
        .await?
        .take(MAX_WAV_BYTES as u64 + 1)
        .read_to_end(&mut wav)
        .await?;
    Ok(wav)
}

impl<'a> Wav<'a> {
    pub(super) fn parse(wav: &'a [u8]) -> io::Result<Self> {
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "expected a nonempty mono PCM16LE RIFF/WAVE at 16 or 48 kHz (max 3 MB)",
            )
        };
        let (head, mut chunks) = wav.split_first_chunk::<12>().ok_or_else(invalid)?;
        if wav.len() > MAX_WAV_BYTES
            || &head[..4] != b"RIFF"
            || &head[8..] != b"WAVE"
            || le32(&head[4..8]) as usize != wav.len() - 8
        {
            return Err(invalid());
        }
        let (mut rate, mut pcm) = (None, None);
        while !chunks.is_empty() {
            let (head, rest) = chunks.split_first_chunk::<8>().ok_or_else(invalid)?;
            let size = le32(&head[4..]) as usize;
            let (data, rest) = rest.split_at_checked(size).ok_or_else(invalid)?;
            // RIFF pads odd-sized chunks; the pad byte is not part of the payload.
            chunks = rest.get((size % 2)..).ok_or_else(invalid)?;
            match &head[..4] {
                b"fmt " if rate.is_none() => {
                    // PCM tag 1, one channel, two bytes/sample, 16 bits.
                    if !(data.len() == 16 || data.len() == 18 && data[16..] == [0, 0])
                        || data[..4] != [1, 0, 1, 0]
                        || data[12..16] != [2, 0, 16, 0]
                    {
                        return Err(invalid());
                    }
                    let hz = le32(&data[4..8]);
                    rate = match hz {
                        16_000 => Some(Rate::Khz16),
                        48_000 => Some(Rate::Khz48),
                        _ => return Err(invalid()),
                    };
                    if le32(&data[8..12]) != hz * 2 {
                        return Err(invalid());
                    }
                }
                b"data" if rate.is_some() && pcm.is_none() => {
                    if data.is_empty() || !data.len().is_multiple_of(2) {
                        return Err(invalid());
                    }
                    pcm = Some(data);
                }
                b"fmt " | b"data" => return Err(invalid()),
                _ => {} // Optional chunks (such as LIST) are not sent to the SDK.
            }
        }
        Ok(Self {
            rate: rate.ok_or_else(invalid)?,
            pcm: pcm.ok_or_else(invalid)?,
        })
    }
}

fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("four bytes from a checked header"))
}
