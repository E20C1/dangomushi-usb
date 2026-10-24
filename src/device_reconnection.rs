use crate::device_permissions::{self, PermissionScope};
use crate::lan_access_control::LanEnableControl;
use crate::trusted_devices;
use ed25519_dalek::{Signature, VerifyingKey};
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::time::{Duration, Instant};

pub const RECONNECT_CHALLENGE_BYTES: usize = 32;
pub const RECONNECT_SIGNATURE_BYTES: usize = 64;
pub const RECONNECT_CHALLENGE_LIFETIME: Duration = Duration::from_secs(2 * 60);

const SIGNATURE_DOMAIN: &[u8] = b"DANGOMUSHI-DEVICE-RECONNECT-V1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectRandomSourceError;

impl fmt::Display for ReconnectRandomSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("random source unavailable")
    }
}

impl Error for ReconnectRandomSourceError {}

pub trait ReconnectRandomSource {
    fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), ReconnectRandomSourceError>;
}

#[derive(Debug, Default)]
pub struct OsReconnectRandom;

impl ReconnectRandomSource for OsReconnectRandom {
    fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), ReconnectRandomSourceError> {
        getrandom::fill(destination).map_err(|_| ReconnectRandomSourceError)
    }
}

pub struct ReconnectChallenge {
    fingerprint: [u8; 32],
    challenge: [u8; RECONNECT_CHALLENGE_BYTES],
    expires_at: Instant,
}

impl ReconnectChallenge {
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub const fn challenge(&self) -> &[u8; RECONNECT_CHALLENGE_BYTES] {
        &self.challenge
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.expires_at.saturating_duration_since(now)
    }
}

impl fmt::Debug for ReconnectChallenge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReconnectChallenge")
            .field("fingerprint", &hex_upper(&self.fingerprint))
            .field("challenge", &"[PUBLIC RANDOM CHALLENGE]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconnectProof {
    pub fingerprint: [u8; 32],
    pub signature: [u8; RECONNECT_SIGNATURE_BYTES],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedDeviceSession {
    fingerprint: [u8; 32],
    device_name: String,
}

impl AuthenticatedDeviceSession {
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn fingerprint_hex(&self) -> String {
        hex_upper(&self.fingerprint)
    }

    pub fn is_currently_valid(
        &self,
        workspace_root: &Path,
        lan_control: &LanEnableControl,
    ) -> Result<bool, ReconnectError> {
        if !lan_control.is_enabled() {
            return Ok(false);
        }

        let trusted = trusted_devices::load(workspace_root)
            .map_err(|_| ReconnectError::TrustedStateUnavailable)?;
        Ok(trusted
            .devices()
            .iter()
            .any(|device| device.fingerprint() == &self.fingerprint))
    }

    pub fn authorize(
        &self,
        workspace_root: &Path,
        lan_control: &LanEnableControl,
        scope: PermissionScope,
    ) -> Result<bool, ReconnectError> {
        if !self.is_currently_valid(workspace_root, lan_control)? {
            return Ok(false);
        }

        device_permissions::authorize(workspace_root, &self.fingerprint, scope)
            .map_err(|_| ReconnectError::PermissionStateUnavailable)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectError {
    LanControlDisabled,
    RandomSourceUnavailable,
    DeviceNotTrusted,
    TrustedStateUnavailable,
    FingerprintMismatch,
    Expired,
    InvalidSignature,
    PermissionStateUnavailable,
}

impl fmt::Display for ReconnectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::LanControlDisabled => "LAN control must be explicitly enabled first",
            Self::RandomSourceUnavailable => "operating-system random source is unavailable",
            Self::DeviceNotTrusted => "device public key is not currently trusted",
            Self::TrustedStateUnavailable => "trusted-device state could not be verified",
            Self::FingerprintMismatch => "reconnect proof fingerprint does not match its challenge",
            Self::Expired => "reconnect challenge expired",
            Self::InvalidSignature => "reconnect proof signature was rejected",
            Self::PermissionStateUnavailable => "device permission state could not be verified",
        };
        formatter.write_str(message)
    }
}

impl Error for ReconnectError {}

pub fn begin(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    fingerprint: &[u8; 32],
    now: Instant,
) -> Result<ReconnectChallenge, ReconnectError> {
    let mut random = OsReconnectRandom;
    begin_with_source(workspace_root, lan_control, fingerprint, now, &mut random)
}

pub fn begin_with_source<R: ReconnectRandomSource + ?Sized>(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    fingerprint: &[u8; 32],
    now: Instant,
    random: &mut R,
) -> Result<ReconnectChallenge, ReconnectError> {
    if !lan_control.is_enabled() {
        return Err(ReconnectError::LanControlDisabled);
    }

    let trusted = trusted_devices::load(workspace_root)
        .map_err(|_| ReconnectError::TrustedStateUnavailable)?;
    if !trusted
        .devices()
        .iter()
        .any(|device| device.fingerprint() == fingerprint)
    {
        return Err(ReconnectError::DeviceNotTrusted);
    }

    let mut challenge = [0u8; RECONNECT_CHALLENGE_BYTES];
    random
        .fill_bytes(&mut challenge)
        .map_err(|_| ReconnectError::RandomSourceUnavailable)?;

    Ok(ReconnectChallenge {
        fingerprint: *fingerprint,
        challenge,
        expires_at: now + RECONNECT_CHALLENGE_LIFETIME,
    })
}

pub fn complete(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    now: Instant,
    challenge: ReconnectChallenge,
    proof: ReconnectProof,
) -> Result<AuthenticatedDeviceSession, ReconnectError> {
    if !lan_control.is_enabled() {
        return Err(ReconnectError::LanControlDisabled);
    }
    if now >= challenge.expires_at {
        return Err(ReconnectError::Expired);
    }
    if proof.fingerprint != challenge.fingerprint {
        return Err(ReconnectError::FingerprintMismatch);
    }

    let trusted = trusted_devices::load(workspace_root)
        .map_err(|_| ReconnectError::TrustedStateUnavailable)?;
    let device = trusted
        .devices()
        .iter()
        .find(|device| device.fingerprint() == &challenge.fingerprint)
        .ok_or(ReconnectError::DeviceNotTrusted)?;

    let verifying_key =
        VerifyingKey::from_bytes(device.public_key()).map_err(|_| ReconnectError::InvalidSignature)?;
    let signature = Signature::from_bytes(&proof.signature);
    let message = reconnect_signature_message(&challenge.challenge, &challenge.fingerprint);

    verifying_key
        .verify_strict(&message, &signature)
        .map_err(|_| ReconnectError::InvalidSignature)?;

    Ok(AuthenticatedDeviceSession {
        fingerprint: challenge.fingerprint,
        device_name: device.device_name().to_string(),
    })
}

pub fn reconnect_signature_message(
    challenge: &[u8; RECONNECT_CHALLENGE_BYTES],
    fingerprint: &[u8; 32],
) -> Vec<u8> {
    let mut message =
        Vec::with_capacity(SIGNATURE_DOMAIN.len() + RECONNECT_CHALLENGE_BYTES + fingerprint.len());
    message.extend_from_slice(SIGNATURE_DOMAIN);
    message.extend_from_slice(challenge);
    message.extend_from_slice(fingerprint);
    message
}

fn hex_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[(byte >> 4) as usize]));
        output.push(char::from(HEX[(byte & 0x0F) as usize]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedRandom {
        bytes: [u8; RECONNECT_CHALLENGE_BYTES],
    }

    impl ReconnectRandomSource for FixedRandom {
        fn fill_bytes(
            &mut self,
            destination: &mut [u8],
        ) -> Result<(), ReconnectRandomSourceError> {
            destination.copy_from_slice(&self.bytes);
            Ok(())
        }
    }

    #[test]
    fn signature_message_is_domain_separated_and_deterministic() {
        let challenge = [7u8; RECONNECT_CHALLENGE_BYTES];
        let fingerprint = [9u8; 32];
        let first = reconnect_signature_message(&challenge, &fingerprint);
        let second = reconnect_signature_message(&challenge, &fingerprint);
        assert_eq!(first, second);
        assert!(first.starts_with(SIGNATURE_DOMAIN));
        assert_ne!(
            first,
            reconnect_signature_message(&[8u8; RECONNECT_CHALLENGE_BYTES], &fingerprint)
        );
    }

    #[test]
    fn fixed_random_source_is_injectable() {
        let mut random = FixedRandom {
            bytes: [11u8; RECONNECT_CHALLENGE_BYTES],
        };
        let mut output = [0u8; RECONNECT_CHALLENGE_BYTES];
        random.fill_bytes(&mut output).expect("fixed random");
        assert_eq!(output, [11u8; RECONNECT_CHALLENGE_BYTES]);
    }
}
