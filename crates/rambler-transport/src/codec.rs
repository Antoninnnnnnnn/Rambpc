//! Opaque gRPC payload codec: tonic alone owns the five-byte gRPC envelope.
use bytes::{Buf, BufMut, Bytes};
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};

/// No protobuf interpretation, normalization, or reconstruction occurs.
#[derive(Clone, Copy, Default, Debug)]
pub struct RawBytesCodec;
#[derive(Clone, Copy, Default, Debug)]
pub struct RawBytesEncoder;
#[derive(Clone, Copy, Default, Debug)]
pub struct RawBytesDecoder;
impl Codec for RawBytesCodec {
    type Encode = Bytes;
    type Decode = Bytes;
    type Encoder = RawBytesEncoder;
    type Decoder = RawBytesDecoder;
    fn encoder(&mut self) -> Self::Encoder {
        RawBytesEncoder
    }
    fn decoder(&mut self) -> Self::Decoder {
        RawBytesDecoder
    }
}
impl Encoder for RawBytesEncoder {
    type Item = Bytes;
    type Error = tonic::Status;
    fn encode(&mut self, item: Bytes, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        dst.put(item);
        Ok(())
    }
}
impl Decoder for RawBytesDecoder {
    type Item = Bytes;
    type Error = tonic::Status;
    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Bytes>, Self::Error> {
        Ok(Some(src.copy_to_bytes(src.remaining())))
    }
}
