use crate::peer_identity::PEER_FINGERPRINT_BYTES;
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt;

pub const RECEIVED_FILE_SHA256_BYTES: usize = 32;

#[derive(PartialEq, Eq)]
pub struct PendingReceivedFile {
    suggested_name: String,
    declared_len: u64,
    expected_sha256: [u8; RECEIVED_FILE_SHA256_BYTES],
    sender_fingerprint: [u8; PEER_FINGERPRINT_BYTES],
    bytes: Vec<u8>,
}

impl PendingReceivedFile {
    pub(crate) fn from_authenticated_payload(
        suggested_name: String,
        declared_len: u64,
        expected_sha256: [u8; RECEIVED_FILE_SHA256_BYTES],
        sender_fingerprint: [u8; PEER_FINGERPRINT_BYTES],
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            suggested_name,
            declared_len,
            expected_sha256,
            sender_fingerprint,
            bytes,
        }
    }

    pub fn suggested_name(&self) -> &str {
        &self.suggested_name
    }

    pub const fn declared_len(&self) -> u64 {
        self.declared_len
    }

    pub const fn expected_sha256(&self) -> &[u8; RECEIVED_FILE_SHA256_BYTES] {
        &self.expected_sha256
    }

    pub fn expected_sha256_hex(&self) -> String {
        hex_lower(&self.expected_sha256)
    }

    pub const fn sender_fingerprint(&self) -> &[u8; PEER_FINGERPRINT_BYTES] {
        &self.sender_fingerprint
    }

    pub fn verify(self) -> Result<VerifiedReceivedFile, ReceivedFileIntegrityError> {
        let actual_len =
            u64::try_from(self.bytes.len()).map_err(|_| ReceivedFileIntegrityError::LengthMismatch)?;
        if actual_len != self.declared_len {
            return Err(ReceivedFileIntegrityError::LengthMismatch);
        }

        let actual_sha256 = sha256_bytes(&self.bytes);
        if actual_sha256 != self.expected_sha256 {
            return Err(ReceivedFileIntegrityError::DigestMismatch);
        }

        Ok(VerifiedReceivedFile {
            suggested_name: self.suggested_name,
            bytes: self.bytes,
            sha256: actual_sha256,
            sender_fingerprint: self.sender_fingerprint,
        })
    }
}

impl fmt::Debug for PendingReceivedFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingReceivedFile")
            .field("suggested_name", &"[REDACTED]")
            .field("declared_len", &self.declared_len)
            .field("expected_sha256", &self.expected_sha256_hex())
            .field("sender_fingerprint", &hex_upper(&self.sender_fingerprint))
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

#[derive(PartialEq, Eq)]
pub struct VerifiedReceivedFile {
    suggested_name: String,
    bytes: Vec<u8>,
    sha256: [u8; RECEIVED_FILE_SHA256_BYTES],
    sender_fingerprint: [u8; PEER_FINGERPRINT_BYTES],
}

impl VerifiedReceivedFile {
    pub fn suggested_name(&self) -> &str {
        &self.suggested_name
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn bytes_verified(&self) -> u64 {
        self.bytes.len() as u64
    }

    pub const fn sha256(&self) -> &[u8; RECEIVED_FILE_SHA256_BYTES] {
        &self.sha256
    }

    pub fn sha256_hex(&self) -> String {
        hex_lower(&self.sha256)
    }

    pub const fn sender_fingerprint(&self) -> &[u8; PEER_FINGERPRINT_BYTES] {
        &self.sender_fingerprint
    }
}

impl fmt::Debug for VerifiedReceivedFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedReceivedFile")
            .field("suggested_name", &"[REDACTED]")
            .field("bytes_verified", &self.bytes.len())
            .field("sha256", &self.sha256_hex())
            .field("sender_fingerprint", &hex_upper(&self.sender_fingerprint))
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceivedFileIntegrityError {
    LengthMismatch,
    DigestMismatch,
}

impl fmt::Display for ReceivedFileIntegrityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::LengthMismatch => "received file length does not match authenticated metadata",
            Self::DigestMismatch => "received file SHA-256 does not match authenticated metadata",
        };
        formatter.write_str(message)
    }
}

impl Error for ReceivedFileIntegrityError {}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> [u8; RECEIVED_FILE_SHA256_BYTES] {
    let digest = Sha256::digest(bytes);
    let mut output = [0_u8; RECEIVED_FILE_SHA256_BYTES];
    output.copy_from_slice(&digest);
    output
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn hex_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA256_ABC: [u8; 32] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
        0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
        0xf2, 0x00, 0x15, 0xad,
    ];

    #[test]
    fn authenticated_metadata_verifies_known_bytes() {
        let pending = PendingReceivedFile::from_authenticated_payload(
            "abc.bin".to_string(),
            3,
            SHA256_ABC,
            [7_u8; PEER_FINGERPRINT_BYTES],
            b"abc".to_vec(),
        );
        let verified = pending.verify().expect("verified");
        assert_eq!(verified.bytes(), b"abc");
        assert_eq!(verified.sha256(), &SHA256_ABC);
        assert_eq!(verified.bytes_verified(), 3);
    }

    #[test]
    fn length_mismatch_fails_closed_before_digest_acceptance() {
        let pending = PendingReceivedFile::from_authenticated_payload(
            "abc.bin".to_string(),
            4,
            SHA256_ABC,
            [8_u8; PEER_FINGERPRINT_BYTES],
            b"abc".to_vec(),
        );
        assert_eq!(
            pending.verify().unwrap_err(),
            ReceivedFileIntegrityError::LengthMismatch
        );
    }

    #[test]
    fn digest_mismatch_fails_closed() {
        let pending = PendingReceivedFile::from_authenticated_payload(
            "abc.bin".to_string(),
            3,
            [0_u8; RECEIVED_FILE_SHA256_BYTES],
            [9_u8; PEER_FINGERPRINT_BYTES],
            b"abc".to_vec(),
        );
        assert_eq!(
            pending.verify().unwrap_err(),
            ReceivedFileIntegrityError::DigestMismatch
        );
    }

    #[test]
    fn debug_never_prints_filename_or_contents() {
        let pending = PendingReceivedFile::from_authenticated_payload(
            "secret-name.bin".to_string(),
            3,
            SHA256_ABC,
            [10_u8; PEER_FINGERPRINT_BYTES],
            b"abc".to_vec(),
        );
        let debug = format!("{pending:?}");
        assert!(!debug.contains("secret-name.bin"));
        assert!(!debug.contains("abc"));
        assert!(debug.contains("[REDACTED]"));
    }
}
