//! Incremental gRPC envelope decoder. Payloads are opaque to the transport.
use crate::TransportError;
use bytes::{Buf, Bytes, BytesMut};
use tonic::Code;

const PREFIX_LEN: usize = 5;

pub(super) struct Deframer {
    // None means there is no allocation to try to recycle. Some can be empty
    // while its allocation is either reusable or still shared with an output.
    buffer: Option<BytesMut>,
    limit: usize,
    #[cfg(test)]
    copied_input: usize,
}

fn message_size(bytes: &[u8], limit: usize) -> Result<Option<usize>, TransportError> {
    if bytes.len() < PREFIX_LEN {
        return Ok(None);
    }
    match bytes[0] {
        0 => {}
        1 => return Err(TransportError::CompressionUnsupported),
        _ => return Err(TransportError::Protocol),
    }
    let size = u32::from_be_bytes(bytes[1..PREFIX_LEN].try_into().unwrap()) as usize;
    if size > limit {
        return Err(TransportError::Grpc(Code::ResourceExhausted));
    }
    Ok(Some(size))
}

impl Deframer {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            buffer: None,
            limit,
            #[cfg(test)]
            copied_input: 0,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.buffer.as_ref().is_none_or(BytesMut::is_empty)
    }

    /// Accept a DATA frame and, at most, emit its first ready message. The
    /// caller must drain `message()` before feeding the next frame.
    pub(super) fn frame(&mut self, chunk: &[u8]) -> Result<Option<Bytes>, TransportError> {
        if self.is_empty() {
            if let Some(size) = message_size(chunk, self.limit)?
                && size <= chunk.len() - PREFIX_LEN
            {
                if size == chunk.len() - PREFIX_LEN {
                    if size == 0 {
                        return Ok(Some(Bytes::new()));
                    }
                    // Keep allocation-free lane reuse when prior outputs have
                    // been dropped. If the buffer cannot be reclaimed, copy only
                    // the payload into a compact Bytes, without a split (and its
                    // shared allocation) or pinning h2's receive slab. Drop the
                    // empty buffer; a later frame can reestablish reuse.
                    if self.buffer.as_mut().is_some_and(|buffer| {
                        let spare = buffer.capacity();
                        // An empty, used buffer has consumed at least a prefix.
                        // Try reclaiming that space before giving up on reuse,
                        // even if this particular frame will need it to grow.
                        spare < chunk.len() && !buffer.try_reclaim(spare + 1)
                    }) {
                        self.buffer = None;
                        #[cfg(test)]
                        {
                            self.copied_input += size;
                        }
                        return Ok(Some(Bytes::copy_from_slice(&chunk[PREFIX_LEN..])));
                    }
                }
                self.extend(chunk);
                return Ok(Some(self.take_message(size)));
            }
            self.extend(chunk);
            return Ok(None);
        }
        self.extend(chunk);
        self.message()
    }

    pub(super) fn message(&mut self) -> Result<Option<Bytes>, TransportError> {
        let Some(buffer) = &self.buffer else {
            return Ok(None);
        };
        if let Some(size) = message_size(buffer, self.limit)?
            && size <= buffer.len() - PREFIX_LEN
        {
            return Ok(Some(self.take_message(size)));
        }
        Ok(None)
    }

    fn extend(&mut self, chunk: &[u8]) {
        if !chunk.is_empty() {
            self.buffer.get_or_insert_default().extend_from_slice(chunk);
            #[cfg(test)]
            {
                self.copied_input += chunk.len();
            }
        }
    }

    fn take_message(&mut self, size: usize) -> Bytes {
        let buffer = self.buffer.as_mut().unwrap();
        buffer.advance(PREFIX_LEN);
        buffer.split_to(size).freeze()
    }
}

#[cfg(test)]
mod tests;
