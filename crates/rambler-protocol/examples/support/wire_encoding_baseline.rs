//! Frozen encoding helpers from d9af4f5c0767361fb9b2a13043e228f924e720bf.
//! Function bodies/signatures below are verbatim wire.rs lines 21..59.
//! Original git-LF file: 5192 bytes, SHA256
//! c9a6fac56e0497eb12d6104ee418e47b7e9f3c011c9b5fc5f52ae0c80edd2a6b.
//! `crate::bounded` is reproduced verbatim in the example root from lib.rs.
use rambler_protocol::Error;

pub fn encode_varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(10);
    while value >= 128 {
        out.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    out.push(value as u8);
    out
}

fn key(number: u32, wire_type: u8) -> Result<Vec<u8>, Error> {
    if !(1..=0x1fff_ffff).contains(&number) {
        return Err(Error::FieldNumber);
    }
    Ok(encode_varint(
        (u64::from(number) << 3) | u64::from(wire_type),
    ))
}
pub fn varint_field(number: u32, value: u64) -> Result<Vec<u8>, Error> {
    let mut out = key(number, 0)?;
    out.extend(encode_varint(value));
    Ok(out)
}
pub fn bytes_field(number: u32, value: &[u8]) -> Result<Vec<u8>, Error> {
    crate::bounded(value.len())?;
    let mut out = key(number, 2)?;
    out.extend(encode_varint(value.len() as u64));
    out.extend_from_slice(value);
    crate::bounded(out.len())?;
    Ok(out)
}
pub fn string_field(number: u32, value: &str) -> Result<Vec<u8>, Error> {
    bytes_field(number, value.as_bytes())
}
pub(crate) fn fixed32_field(number: u32, value: [u8; 4]) -> Result<Vec<u8>, Error> {
    let mut out = key(number, 5)?;
    out.extend(value);
    Ok(out)
}
