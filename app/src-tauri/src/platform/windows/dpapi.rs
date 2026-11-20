use crate::ports::{ProtectError, Protector};
use windows::Win32::{
    Foundation::{HLOCAL, LocalFree},
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};
use windows::core::PCWSTR;
use zeroize::{Zeroize, Zeroizing};

// This is a stable application namespace, not a secret. Changing it would
// make previously stored entries unreadable.
const ENTROPY: &[u8] = b"Sambler.Windows.UserStorage.v1";

#[derive(Default)]
pub struct Dpapi;

impl Dpapi {
    pub fn new() -> Self {
        Self
    }
}

fn input_blob(data: &[u8]) -> Option<CRYPT_INTEGER_BLOB> {
    Some(CRYPT_INTEGER_BLOB {
        cbData: data.len().try_into().ok()?,
        // The DPAPI input blobs are read-only; Win32 uses a mutable pointer type.
        pbData: data.as_ptr().cast_mut(),
    })
}

#[derive(Default)]
struct Output {
    blob: CRYPT_INTEGER_BLOB,
    plaintext: bool,
}

impl Output {
    fn bytes(&self) -> &[u8] {
        if self.blob.pbData.is_null() || self.blob.cbData == 0 {
            return &[];
        }
        // DPAPI returned this owned allocation and its byte length together.
        unsafe { std::slice::from_raw_parts(self.blob.pbData, self.blob.cbData as usize) }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        if !self.blob.pbData.is_null() {
            unsafe {
                if self.plaintext {
                    std::slice::from_raw_parts_mut(self.blob.pbData, self.blob.cbData as usize)
                        .zeroize();
                }
                LocalFree(Some(HLOCAL(self.blob.pbData.cast())));
            }
        }
    }
}

impl Protector for Dpapi {
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError> {
        let input = input_blob(plain).ok_or(ProtectError::Protect)?;
        let entropy = input_blob(ENTROPY).ok_or(ProtectError::Protect)?;
        let mut output = Output::default();
        // No CRYPTPROTECT_LOCAL_MACHINE: the Windows user owns this data.
        unsafe {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output.blob,
            )
        }
        .map_err(|_| ProtectError::Protect)?;
        Ok(output.bytes().to_vec())
    }

    fn unprotect(&self, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
        let input = input_blob(blob).ok_or(ProtectError::Unreadable)?;
        let entropy = input_blob(ENTROPY).ok_or(ProtectError::Unreadable)?;
        let mut output = Output {
            plaintext: true,
            ..Output::default()
        };
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output.blob,
            )
        }
        .map_err(|_| ProtectError::Unreadable)?;
        Ok(Zeroizing::new(output.bytes().to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_scope_round_trip_and_invalid_blob() {
        let dpapi = Dpapi::new();
        let plain = b"Sambler DPAPI fixture";
        let encrypted = dpapi.protect(plain).unwrap();
        assert!(!encrypted.windows(plain.len()).any(|part| part == plain));
        assert!(dpapi.unprotect(&encrypted).unwrap().as_slice() == plain);
        assert!(matches!(
            dpapi.unprotect(b"invalid"),
            Err(ProtectError::Unreadable)
        ));
    }
}
