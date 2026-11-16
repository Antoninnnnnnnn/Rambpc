//! Uncompressed gRPC envelope prefixes; payload ownership is handled by the caller.
use bytes::Bytes;

// The entire one-byte length domain, not a workload-specific selection. This
// immutable 1,280-byte table is built at compile time, with no runtime allocation.
static SMALL_PREFIXES: [[u8; 5]; 256] = {
    let mut prefixes = [[0; 5]; 256];
    let mut len = 0;
    while len < prefixes.len() {
        prefixes[len][4] = len as u8;
        len += 1;
    }
    prefixes
};

/// The caller must validate the payload length fits u32 before calling this.
pub(crate) fn grpc_prefix(len: u32) -> Bytes {
    if len < 256 {
        Bytes::from_static(&SMALL_PREFIXES[len as usize])
    } else {
        let mut header = [0u8; 5];
        header[1..].copy_from_slice(&len.to_be_bytes());
        Bytes::copy_from_slice(&header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_uncompressed_big_endian_prefixes() {
        for (len, expected) in [
            (0, [0, 0, 0, 0, 0]),
            (1, [0, 0, 0, 0, 1]),
            (127, [0, 0, 0, 0, 127]),
            (128, [0, 0, 0, 0, 128]),
            (255, [0, 0, 0, 0, 255]),
            (256, [0, 0, 0, 1, 0]),
            (1024, [0, 0, 0, 4, 0]),
            (65535, [0, 0, 0, 255, 255]),
            (65536, [0, 0, 1, 0, 0]),
            (u32::MAX, [0, 255, 255, 255, 255]),
        ] {
            assert_eq!(grpc_prefix(len).as_ref(), &expected);
        }
    }

    #[test]
    fn every_one_byte_length_has_the_canonical_prefix() {
        for low in 0..=u8::MAX {
            assert_eq!(grpc_prefix(u32::from(low)).as_ref(), &[0, 0, 0, 0, low]);
        }
    }

    #[test]
    fn fallback_boundaries_preserve_all_four_length_bytes() {
        for len in [
            256u32,
            257,
            65535,
            65536,
            65537,
            0x00ff_ffff,
            0x0100_0000,
            0x0100_0001,
            u32::MAX - 1,
            u32::MAX,
        ] {
            // Independent arithmetic oracle, not the constructor's to_be_bytes.
            let expected = [
                0,
                ((len >> 24) & 255) as u8,
                ((len >> 16) & 255) as u8,
                ((len >> 8) & 255) as u8,
                (len & 255) as u8,
            ];
            assert_eq!(grpc_prefix(len).as_ref(), &expected);
        }
    }

    #[test]
    fn small_prefixes_borrow_the_immutable_table() {
        for len in 0..256u32 {
            let first = grpc_prefix(len);
            let second = grpc_prefix(len);
            assert_eq!(first.as_ptr(), SMALL_PREFIXES[len as usize].as_ptr());
            assert_eq!(first.as_ptr(), second.as_ptr());
        }
    }
}
