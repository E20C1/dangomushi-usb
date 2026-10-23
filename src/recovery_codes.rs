use crate::vault_storage::VaultPlaintext;
use std::error::Error;
use std::fmt;
use std::str;
use zeroize::Zeroize;

const PAYLOAD_MAGIC: &[u8; 8] = b"DGMRC001";
const PAYLOAD_VERSION: u16 = 1;
const PAYLOAD_HEADER_BYTES: usize = 14;

pub const MAX_RECOVERY_CODES: usize = 128;
pub const MAX_RECOVERY_CODE_BYTES: usize = 4096;
pub const MAX_RECOVERY_CODE_PAYLOAD_BYTES: usize = 1024 * 1024;

#[must_use]
pub struct RecoveryCode {
    value: String,
}

impl RecoveryCode {
    fn new(value: String) -> Self {
        Self { value }
    }

    pub fn expose_secret(&self) -> &str {
        &self.value
    }

    pub fn len(&self) -> usize {
        self.value.len()
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
}

impl Drop for RecoveryCode {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl fmt::Debug for RecoveryCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoveryCode([REDACTED])")
    }
}

#[must_use]
pub struct RecoveryCodeSet {
    codes: Vec<RecoveryCode>,
}

impl RecoveryCodeSet {
    pub fn new(mut values: Vec<String>) -> Result<Self, RecoveryCodeError> {
        if values.is_empty() {
            return Err(RecoveryCodeError::NoRecoveryCodes);
        }

        if values.len() > MAX_RECOVERY_CODES {
            zeroize_strings(&mut values);
            return Err(RecoveryCodeError::TooManyRecoveryCodes);
        }

        let validation_error = values.iter().find_map(|value| {
            if value.trim().is_empty() {
                Some(RecoveryCodeError::EmptyRecoveryCode)
            } else if value.len() > MAX_RECOVERY_CODE_BYTES {
                Some(RecoveryCodeError::RecoveryCodeTooLong)
            } else {
                None
            }
        });

        if let Some(error) = validation_error {
            zeroize_strings(&mut values);
            return Err(error);
        }

        Ok(Self {
            codes: values.into_iter().map(RecoveryCode::new).collect(),
        })
    }

    pub fn len(&self) -> usize {
        self.codes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    pub fn code(&self, index: usize) -> Option<&RecoveryCode> {
        self.codes.get(index)
    }

    pub fn encode_vault_payload(&self) -> Result<RecoveryCodeVaultPayload, RecoveryCodeError> {
        let mut total = PAYLOAD_HEADER_BYTES;

        for code in &self.codes {
            total = total
                .checked_add(4)
                .and_then(|value| value.checked_add(code.len()))
                .ok_or(RecoveryCodeError::PayloadTooLarge)?;
        }

        if total > MAX_RECOVERY_CODE_PAYLOAD_BYTES {
            return Err(RecoveryCodeError::PayloadTooLarge);
        }

        let count =
            u32::try_from(self.codes.len()).map_err(|_| RecoveryCodeError::TooManyRecoveryCodes)?;

        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(PAYLOAD_MAGIC);
        bytes.extend_from_slice(&PAYLOAD_VERSION.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());

        for code in &self.codes {
            let length = u32::try_from(code.len())
                .map_err(|_| RecoveryCodeError::RecoveryCodeTooLong)?;
            bytes.extend_from_slice(&length.to_le_bytes());
            bytes.extend_from_slice(code.expose_secret().as_bytes());
        }

        Ok(RecoveryCodeVaultPayload { bytes })
    }

    pub fn from_vault_plaintext(plaintext: &VaultPlaintext) -> Result<Self, RecoveryCodeError> {
        decode_payload_bytes(plaintext.expose_secret())
    }
}

impl fmt::Debug for RecoveryCodeSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoveryCodeSet([REDACTED])")
    }
}

#[must_use]
pub struct RecoveryCodeVaultPayload {
    bytes: Vec<u8>,
}

impl RecoveryCodeVaultPayload {
    pub fn expose_secret(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl Drop for RecoveryCodeVaultPayload {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl fmt::Debug for RecoveryCodeVaultPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoveryCodeVaultPayload([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryCodeError {
    NoRecoveryCodes,
    TooManyRecoveryCodes,
    EmptyRecoveryCode,
    RecoveryCodeTooLong,
    PayloadTooLarge,
    InvalidPayload,
    UnsupportedPayloadVersion,
    InvalidUtf8,
}

impl fmt::Display for RecoveryCodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NoRecoveryCodes => "at least one recovery code is required",
            Self::TooManyRecoveryCodes => "recovery code count exceeds the supported limit",
            Self::EmptyRecoveryCode => "recovery codes must not be blank",
            Self::RecoveryCodeTooLong => "a recovery code exceeds the supported size limit",
            Self::PayloadTooLarge => "recovery code payload exceeds the supported size limit",
            Self::InvalidPayload => "recovery code vault payload is invalid",
            Self::UnsupportedPayloadVersion => {
                "recovery code vault payload version is unsupported"
            }
            Self::InvalidUtf8 => "recovery code vault payload contains invalid text",
        };
        formatter.write_str(message)
    }
}

impl Error for RecoveryCodeError {}

fn decode_payload_bytes(bytes: &[u8]) -> Result<RecoveryCodeSet, RecoveryCodeError> {
    if bytes.len() < PAYLOAD_HEADER_BYTES || bytes.len() > MAX_RECOVERY_CODE_PAYLOAD_BYTES {
        return Err(RecoveryCodeError::InvalidPayload);
    }

    if &bytes[0..8] != PAYLOAD_MAGIC {
        return Err(RecoveryCodeError::InvalidPayload);
    }

    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != PAYLOAD_VERSION {
        return Err(RecoveryCodeError::UnsupportedPayloadVersion);
    }

    let count = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize;
    if count == 0 {
        return Err(RecoveryCodeError::NoRecoveryCodes);
    }
    if count > MAX_RECOVERY_CODES {
        return Err(RecoveryCodeError::TooManyRecoveryCodes);
    }

    let mut offset = PAYLOAD_HEADER_BYTES;
    let mut values = Vec::with_capacity(count);

    for _ in 0..count {
        if offset.checked_add(4).is_none_or(|end| end > bytes.len()) {
            zeroize_strings(&mut values);
            return Err(RecoveryCodeError::InvalidPayload);
        }

        let length = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;

        if length == 0 || length > MAX_RECOVERY_CODE_BYTES {
            zeroize_strings(&mut values);
            return Err(if length == 0 {
                RecoveryCodeError::EmptyRecoveryCode
            } else {
                RecoveryCodeError::RecoveryCodeTooLong
            });
        }

        let end = match offset.checked_add(length) {
            Some(end) if end <= bytes.len() => end,
            _ => {
                zeroize_strings(&mut values);
                return Err(RecoveryCodeError::InvalidPayload);
            }
        };

        let value = match str::from_utf8(&bytes[offset..end]) {
            Ok(value) => value.to_owned(),
            Err(_) => {
                zeroize_strings(&mut values);
                return Err(RecoveryCodeError::InvalidUtf8);
            }
        };

        values.push(value);
        offset = end;
    }

    if offset != bytes.len() {
        zeroize_strings(&mut values);
        return Err(RecoveryCodeError::InvalidPayload);
    }

    RecoveryCodeSet::new(values)
}

fn zeroize_strings(values: &mut [String]) {
    for value in values {
        value.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE_A: &str = "SYNTHETIC_RECOVERY_CODE_ALPHA";
    const CODE_B: &str = "SYNTHETIC_RECOVERY_CODE_BETA";

    fn set() -> RecoveryCodeSet {
        RecoveryCodeSet::new(vec![CODE_A.to_string(), CODE_B.to_string()])
            .expect("synthetic recovery code set")
    }

    #[test]
    fn preserves_recovery_code_text_exactly() {
        let set = set();

        assert_eq!(set.len(), 2);
        assert_eq!(set.code(0).expect("code 0").expose_secret(), CODE_A);
        assert_eq!(set.code(1).expect("code 1").expose_secret(), CODE_B);
    }

    #[test]
    fn recovery_code_types_have_redacted_debug_output() {
        let set = set();
        let code_debug = format!("{:?}", set.code(0).expect("code"));
        let set_debug = format!("{set:?}");
        let payload_debug = format!("{:?}", set.encode_vault_payload().expect("payload"));

        assert_eq!(code_debug, "RecoveryCode([REDACTED])");
        assert_eq!(set_debug, "RecoveryCodeSet([REDACTED])");
        assert_eq!(
            payload_debug,
            "RecoveryCodeVaultPayload([REDACTED])"
        );
        assert!(!code_debug.contains(CODE_A));
        assert!(!set_debug.contains(CODE_A));
        assert!(!payload_debug.contains(CODE_A));
    }

    #[test]
    fn payload_round_trip_preserves_values() {
        let set = set();
        let payload = set.encode_vault_payload().expect("payload");

        let decoded = decode_payload_bytes(payload.expose_secret()).expect("decode");

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded.code(0).expect("code 0").expose_secret(), CODE_A);
        assert_eq!(decoded.code(1).expect("code 1").expose_secret(), CODE_B);
    }

    #[test]
    fn payload_is_versioned_and_rejects_trailing_data() {
        let set = set();
        let payload = set.encode_vault_payload().expect("payload");
        let mut bytes = payload.expose_secret().to_vec();

        assert_eq!(&bytes[0..8], PAYLOAD_MAGIC);
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), PAYLOAD_VERSION);

        bytes.push(0);
        assert_eq!(
            decode_payload_bytes(&bytes).unwrap_err(),
            RecoveryCodeError::InvalidPayload
        );
        bytes.zeroize();
    }

    #[test]
    fn payload_rejects_unsupported_version() {
        let set = set();
        let payload = set.encode_vault_payload().expect("payload");
        let mut bytes = payload.expose_secret().to_vec();

        bytes[8..10].copy_from_slice(&2_u16.to_le_bytes());

        assert_eq!(
            decode_payload_bytes(&bytes).unwrap_err(),
            RecoveryCodeError::UnsupportedPayloadVersion
        );
        bytes.zeroize();
    }

    #[test]
    fn invalid_input_is_rejected_without_echoing_values() {
        let error = RecoveryCodeSet::new(vec!["   ".to_string()])
            .expect_err("blank recovery code must fail");

        assert_eq!(error, RecoveryCodeError::EmptyRecoveryCode);
        assert_eq!(error.to_string(), "recovery codes must not be blank");
    }

    #[test]
    fn oversized_code_is_rejected() {
        let value = "X".repeat(MAX_RECOVERY_CODE_BYTES + 1);

        assert_eq!(
            RecoveryCodeSet::new(vec![value]).unwrap_err(),
            RecoveryCodeError::RecoveryCodeTooLong
        );
    }

    #[test]
    fn too_many_codes_are_rejected() {
        let values = (0..=MAX_RECOVERY_CODES)
            .map(|_| "SYNTHETIC_RECOVERY_CODE".to_string())
            .collect();

        assert_eq!(
            RecoveryCodeSet::new(values).unwrap_err(),
            RecoveryCodeError::TooManyRecoveryCodes
        );
    }
}
