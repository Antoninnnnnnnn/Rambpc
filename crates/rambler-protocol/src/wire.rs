//! Schema-free protobuf wire helpers. Only wire types 0/1/2/5 are accepted.
use crate::{Error, MAX_FIELDS, MAX_MESSAGE_BYTES};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Varint(u64),
    Fixed64([u8; 8]),
    Bytes(Vec<u8>),
    Fixed32([u8; 4]),
}

/// `raw` retains the key, length and value exactly, including noncanonical varints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub number: u32,
    pub wire_type: u8,
    pub value: Value,
    pub raw: Vec<u8>,
}

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

pub(crate) fn read_varint(data: &[u8], offset: &mut usize) -> Result<u64, Error> {
    let mut value = 0;
    for index in 0..10 {
        let byte = *data.get(*offset).ok_or(Error::Truncated)?;
        *offset += 1;
        if index == 9 && byte > 1 {
            return Err(Error::VarintOverflow);
        }
        value |= u64::from(byte & 127) << (7 * index);
        if byte < 128 {
            return Ok(value);
        }
    }
    Err(Error::VarintOverflow)
}

#[derive(Clone, Copy)]
pub(crate) enum RefValue<'a> {
    Varint(u64),
    Data(&'a [u8]),
}
#[derive(Clone, Copy)]
pub(crate) struct RefField<'a> {
    pub number: u32,
    pub wire_type: u8,
    pub value: RefValue<'a>,
    pub raw: &'a [u8],
}
impl RefField<'_> {
    pub fn owned(self) -> Field {
        // Fixed widths were validated by parse_ref; copy without fallible casts.
        let value = match self.value {
            RefValue::Varint(v) => Value::Varint(v),
            RefValue::Data(d) if self.wire_type == 1 => {
                let mut b = [0; 8];
                b.copy_from_slice(d);
                Value::Fixed64(b)
            }
            RefValue::Data(d) if self.wire_type == 5 => {
                let mut b = [0; 4];
                b.copy_from_slice(d);
                Value::Fixed32(b)
            }
            RefValue::Data(d) => Value::Bytes(d.to_vec()),
        };
        Field {
            number: self.number,
            wire_type: self.wire_type,
            value,
            raw: self.raw.to_vec(),
        }
    }
}

pub(crate) fn parse_ref<'a>(
    data: &'a [u8],
    remaining: &mut usize,
) -> Result<Vec<RefField<'a>>, Error> {
    crate::bounded(data.len())?;
    let mut fields = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        if *remaining == 0 {
            return Err(Error::FieldLimit);
        }
        *remaining -= 1;
        let start = offset;
        let key = read_varint(data, &mut offset)?;
        let number = key >> 3;
        if !(1..=0x1fff_ffff).contains(&number) {
            return Err(Error::FieldNumber);
        }
        let wire_type = (key & 7) as u8;
        let value = if wire_type == 0 {
            RefValue::Varint(read_varint(data, &mut offset)?)
        } else {
            let len = match wire_type {
                1 => 8,
                2 => usize::try_from(read_varint(data, &mut offset)?)
                    .map_err(|_| Error::Truncated)?,
                5 => 4,
                _ => return Err(Error::WireType),
            };
            let end = offset.checked_add(len).ok_or(Error::Truncated)?;
            let slice = data.get(offset..end).ok_or(Error::Truncated)?;
            offset = end;
            RefValue::Data(slice)
        };
        fields.push(RefField {
            number: number as u32,
            wire_type,
            value,
            raw: &data[start..offset],
        });
    }
    Ok(fields)
}

pub fn parse_fields(data: &[u8]) -> Result<Vec<Field>, Error> {
    parse_fields_with_limits(data, MAX_MESSAGE_BYTES, MAX_FIELDS)
}
/// Limits may tighten, but cannot relax, the SDK bounds.
pub fn parse_fields_with_limits(
    data: &[u8],
    max_size: usize,
    max_fields: usize,
) -> Result<Vec<Field>, Error> {
    if data.len() > max_size.min(MAX_MESSAGE_BYTES) {
        return Err(Error::SizeLimit);
    }
    let mut remaining = max_fields.min(MAX_FIELDS);
    Ok(parse_ref(data, &mut remaining)?
        .into_iter()
        .map(RefField::owned)
        .collect())
}
