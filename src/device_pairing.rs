use crate::lan_access_control::LanEnableControl;
use ed25519_dalek::{Signature, VerifyingKey};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub const PAIRING_CODE_CHARS: usize = 16;
pub const PAIRING_CHALLENGE_BYTES: usize = 32;
pub const PAIRING_PROOF_BYTES: usize = 32;
pub const PAIRING_PUBLIC_KEY_BYTES: usize = 32;
pub const PAIRING_SIGNATURE_BYTES: usize = 64;
pub const MAX_PAIRING_ATTEMPTS: u8 = 5;
pub const MAX_DEVICE_NAME_BYTES: usize = 64;
pub const PAIRING_LIFETIME: Duration = Duration::from_secs(5 * 60);

const CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const TRANSCRIPT_DOMAIN: &[u8] = b"DANGOMUSHI-PAIRING-V1\0";
const SIGNATURE_DOMAIN: &[u8] = b"DANGOMUSHI-PAIRING-SIGNATURE-V1\0";
const FINGERPRINT_DOMAIN: &[u8] = b"DANGOMUSHI-DEVICE-KEY-FINGERPRINT-V1\0";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairingRandomSourceError;

impl fmt::Display for PairingRandomSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("random source unavailable")
    }
}

impl Error for PairingRandomSourceError {}

pub trait PairingRandomSource {
    fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), PairingRandomSourceError>;
}

#[derive(Debug, Default)]
pub struct OsPairingRandom;

impl PairingRandomSource for OsPairingRandom {
    fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), PairingRandomSourceError> {
        getrandom::fill(destination).map_err(|_| PairingRandomSourceError)
    }
}

pub struct PairingCode {
    value: Zeroizing<String>,
}

impl PairingCode {
    pub fn expose_secret(&self) -> &str {
        self.value.as_str()
    }
}

impl fmt::Debug for PairingCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PairingCode([REDACTED])")
    }
}

pub struct PairingInvitation {
    code: PairingCode,
    challenge: [u8; PAIRING_CHALLENGE_BYTES],
    expires_at: Instant,
}

impl PairingInvitation {
    pub fn code(&self) -> &PairingCode {
        &self.code
    }

    pub const fn challenge(&self) -> &[u8; PAIRING_CHALLENGE_BYTES] {
        &self.challenge
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.expires_at.saturating_duration_since(now)
    }
}

impl fmt::Debug for PairingInvitation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PairingInvitation")
            .field("code", &"[REDACTED]")
            .field("challenge", &"[PUBLIC CHALLENGE]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingSubmission {
    pub device_name: String,
    pub public_key: [u8; PAIRING_PUBLIC_KEY_BYTES],
    pub code_proof: [u8; PAIRING_PROOF_BYTES],
    pub signature: [u8; PAIRING_SIGNATURE_BYTES],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCandidateSummary {
    device_name: String,
    fingerprint: [u8; 32],
}

impl PairingCandidateSummary {
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub fn fingerprint_hex(&self) -> String {
        hex_upper(&self.fingerprint)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedDeviceCandidate {
    device_name: String,
    public_key: [u8; PAIRING_PUBLIC_KEY_BYTES],
    fingerprint: [u8; 32],
}

impl PairedDeviceCandidate {
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub const fn public_key(&self) -> &[u8; PAIRING_PUBLIC_KEY_BYTES] {
        &self.public_key
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub fn fingerprint_hex(&self) -> String {
        hex_upper(&self.fingerprint)
    }

    fn summary(&self) -> PairingCandidateSummary {
        PairingCandidateSummary {
            device_name: self.device_name.clone(),
            fingerprint: self.fingerprint,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingStatus {
    Idle,
    Waiting {
        attempts_remaining: u8,
        remaining: Duration,
    },
    AwaitingApproval {
        summary: PairingCandidateSummary,
        remaining: Duration,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingError {
    LanControlDisabled,
    AlreadyActive,
    RandomSourceUnavailable,
    InvalidPairingCode,
    InvalidDeviceName,
    NotWaiting,
    NotAwaitingApproval,
    ProofRejected,
    AttemptsExhausted,
    Expired,
}

impl fmt::Display for PairingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::LanControlDisabled => "LAN control must be explicitly enabled first",
            Self::AlreadyActive => "a pairing session is already active",
            Self::RandomSourceUnavailable => "operating-system random source is unavailable",
            Self::InvalidPairingCode => "pairing code format is invalid",
            Self::InvalidDeviceName => "device name is invalid",
            Self::NotWaiting => "pairing session is not waiting for a device proof",
            Self::NotAwaitingApproval => "no pairing candidate is awaiting approval",
            Self::ProofRejected => "pairing proof was rejected",
            Self::AttemptsExhausted => "pairing proof attempt limit was reached",
            Self::Expired => "pairing session expired",
        };
        formatter.write_str(message)
    }
}

impl Error for PairingError {}

struct ActivePairing {
    code_key: Zeroizing<[u8; PAIRING_CODE_CHARS]>,
    challenge: [u8; PAIRING_CHALLENGE_BYTES],
    expires_at: Instant,
    attempts_remaining: u8,
}

struct PendingCandidate {
    candidate: PairedDeviceCandidate,
    expires_at: Instant,
}

enum PairingState {
    Idle,
    Waiting(ActivePairing),
    AwaitingApproval(PendingCandidate),
}

pub struct DevicePairing {
    state: PairingState,
}

impl Default for DevicePairing {
    fn default() -> Self {
        Self {
            state: PairingState::Idle,
        }
    }
}

impl DevicePairing {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin(
        &mut self,
        lan_control: &LanEnableControl,
        now: Instant,
    ) -> Result<PairingInvitation, PairingError> {
        let mut random = OsPairingRandom;
        self.begin_with_source(lan_control, now, &mut random)
    }

    pub fn begin_with_source<R: PairingRandomSource + ?Sized>(
        &mut self,
        lan_control: &LanEnableControl,
        now: Instant,
        random: &mut R,
    ) -> Result<PairingInvitation, PairingError> {
        if !lan_control.is_enabled() {
            return Err(PairingError::LanControlDisabled);
        }

        self.expire_if_needed(now);
        if !matches!(self.state, PairingState::Idle) {
            return Err(PairingError::AlreadyActive);
        }

        let mut random_code = [0u8; PAIRING_CODE_CHARS];
        let mut challenge = [0u8; PAIRING_CHALLENGE_BYTES];
        random
            .fill_bytes(&mut random_code)
            .map_err(|_| PairingError::RandomSourceUnavailable)?;
        random
            .fill_bytes(&mut challenge)
            .map_err(|_| PairingError::RandomSourceUnavailable)?;

        let mut normalized = [0u8; PAIRING_CODE_CHARS];
        for (target, random_byte) in normalized.iter_mut().zip(random_code) {
            *target = CODE_ALPHABET[(random_byte & 31) as usize];
        }

        let display = format_pairing_code(&normalized);
        let expires_at = now + PAIRING_LIFETIME;

        self.state = PairingState::Waiting(ActivePairing {
            code_key: Zeroizing::new(normalized),
            challenge,
            expires_at,
            attempts_remaining: MAX_PAIRING_ATTEMPTS,
        });

        Ok(PairingInvitation {
            code: PairingCode {
                value: Zeroizing::new(display),
            },
            challenge,
            expires_at,
        })
    }

    pub fn status(&mut self, now: Instant) -> PairingStatus {
        self.expire_if_needed(now);

        match &self.state {
            PairingState::Idle => PairingStatus::Idle,
            PairingState::Waiting(active) => PairingStatus::Waiting {
                attempts_remaining: active.attempts_remaining,
                remaining: active.expires_at.saturating_duration_since(now),
            },
            PairingState::AwaitingApproval(pending) => PairingStatus::AwaitingApproval {
                summary: pending.candidate.summary(),
                remaining: pending.expires_at.saturating_duration_since(now),
            },
        }
    }

    pub fn submit(
        &mut self,
        lan_control: &LanEnableControl,
        now: Instant,
        submission: PairingSubmission,
    ) -> Result<PairingCandidateSummary, PairingError> {
        if !lan_control.is_enabled() {
            self.cancel();
            return Err(PairingError::LanControlDisabled);
        }

        let state = std::mem::replace(&mut self.state, PairingState::Idle);
        let active = match state {
            PairingState::Waiting(active) => active,
            PairingState::Idle => return Err(PairingError::NotWaiting),
            PairingState::AwaitingApproval(pending) => {
                self.state = PairingState::AwaitingApproval(pending);
                return Err(PairingError::NotWaiting);
            }
        };

        if now >= active.expires_at {
            return Err(PairingError::Expired);
        }

        let transcript = match pairing_transcript(
            &active.challenge,
            &submission.device_name,
            &submission.public_key,
        ) {
            Ok(transcript) => transcript,
            Err(error) => {
                self.state = PairingState::Waiting(active);
                return Err(error);
            }
        };

        let mut mac = HmacSha256::new_from_slice(&active.code_key[..])
            .expect("HMAC accepts pairing code keys");
        mac.update(&transcript);
        if mac.verify_slice(&submission.code_proof).is_err() {
            return self.reject_proof(active);
        }

        let verifying_key = match VerifyingKey::from_bytes(&submission.public_key) {
            Ok(key) => key,
            Err(_) => return self.reject_proof(active),
        };
        let signature = Signature::from_bytes(&submission.signature);
        let signature_message = pairing_signature_message(&transcript, &submission.code_proof);

        if verifying_key
            .verify_strict(&signature_message, &signature)
            .is_err()
        {
            return self.reject_proof(active);
        }

        let candidate = PairedDeviceCandidate {
            device_name: submission.device_name,
            public_key: submission.public_key,
            fingerprint: device_key_fingerprint(&submission.public_key),
        };
        let summary = candidate.summary();

        self.state = PairingState::AwaitingApproval(PendingCandidate {
            candidate,
            expires_at: active.expires_at,
        });

        Ok(summary)
    }

    pub fn approve(
        &mut self,
        lan_control: &LanEnableControl,
        now: Instant,
    ) -> Result<PairedDeviceCandidate, PairingError> {
        if !lan_control.is_enabled() {
            self.cancel();
            return Err(PairingError::LanControlDisabled);
        }

        let state = std::mem::replace(&mut self.state, PairingState::Idle);
        match state {
            PairingState::AwaitingApproval(pending) => {
                if now >= pending.expires_at {
                    Err(PairingError::Expired)
                } else {
                    Ok(pending.candidate)
                }
            }
            PairingState::Waiting(active) => {
                self.state = PairingState::Waiting(active);
                Err(PairingError::NotAwaitingApproval)
            }
            PairingState::Idle => Err(PairingError::NotAwaitingApproval),
        }
    }

    pub fn reject_candidate(&mut self) -> bool {
        if matches!(self.state, PairingState::AwaitingApproval(_)) {
            self.state = PairingState::Idle;
            true
        } else {
            false
        }
    }

    pub fn cancel(&mut self) -> bool {
        let changed = !matches!(self.state, PairingState::Idle);
        self.state = PairingState::Idle;
        changed
    }

    fn reject_proof(
        &mut self,
        mut active: ActivePairing,
    ) -> Result<PairingCandidateSummary, PairingError> {
        active.attempts_remaining = active.attempts_remaining.saturating_sub(1);
        if active.attempts_remaining == 0 {
            Err(PairingError::AttemptsExhausted)
        } else {
            self.state = PairingState::Waiting(active);
            Err(PairingError::ProofRejected)
        }
    }

    fn expire_if_needed(&mut self, now: Instant) -> bool {
        let expired = match &self.state {
            PairingState::Idle => false,
            PairingState::Waiting(active) => now >= active.expires_at,
            PairingState::AwaitingApproval(pending) => now >= pending.expires_at,
        };
        if expired {
            self.state = PairingState::Idle;
        }
        expired
    }
}

pub fn pairing_transcript(
    challenge: &[u8; PAIRING_CHALLENGE_BYTES],
    device_name: &str,
    public_key: &[u8; PAIRING_PUBLIC_KEY_BYTES],
) -> Result<Vec<u8>, PairingError> {
    validate_device_name(device_name)?;
    let name_bytes = device_name.as_bytes();
    let name_len = u16::try_from(name_bytes.len()).expect("device name bound fits u16");

    let mut transcript = Vec::with_capacity(
        TRANSCRIPT_DOMAIN.len()
            + PAIRING_CHALLENGE_BYTES
            + PAIRING_PUBLIC_KEY_BYTES
            + 2
            + name_bytes.len(),
    );
    transcript.extend_from_slice(TRANSCRIPT_DOMAIN);
    transcript.extend_from_slice(challenge);
    transcript.extend_from_slice(public_key);
    transcript.extend_from_slice(&name_len.to_be_bytes());
    transcript.extend_from_slice(name_bytes);
    Ok(transcript)
}

pub fn pairing_proof(
    pairing_code: &str,
    challenge: &[u8; PAIRING_CHALLENGE_BYTES],
    device_name: &str,
    public_key: &[u8; PAIRING_PUBLIC_KEY_BYTES],
) -> Result<[u8; PAIRING_PROOF_BYTES], PairingError> {
    let normalized = normalize_pairing_code(pairing_code)?;
    let transcript = pairing_transcript(challenge, device_name, public_key)?;
    let mut mac =
        HmacSha256::new_from_slice(&normalized).expect("HMAC accepts pairing code keys");
    mac.update(&transcript);
    let bytes = mac.finalize().into_bytes();
    let mut proof = [0u8; PAIRING_PROOF_BYTES];
    proof.copy_from_slice(&bytes);
    Ok(proof)
}

pub fn pairing_signature_message(
    transcript: &[u8],
    proof: &[u8; PAIRING_PROOF_BYTES],
) -> Vec<u8> {
    let mut message = Vec::with_capacity(SIGNATURE_DOMAIN.len() + transcript.len() + proof.len());
    message.extend_from_slice(SIGNATURE_DOMAIN);
    message.extend_from_slice(transcript);
    message.extend_from_slice(proof);
    message
}

fn validate_device_name(device_name: &str) -> Result<(), PairingError> {
    if device_name.is_empty()
        || device_name.len() > MAX_DEVICE_NAME_BYTES
        || device_name.trim() != device_name
        || device_name.chars().any(|character| character.is_control())
    {
        return Err(PairingError::InvalidDeviceName);
    }
    Ok(())
}

fn normalize_pairing_code(code: &str) -> Result<[u8; PAIRING_CODE_CHARS], PairingError> {
    let mut normalized = [0u8; PAIRING_CODE_CHARS];
    let mut count = 0usize;

    for byte in code.bytes() {
        if matches!(byte, b'-' | b' ') {
            continue;
        }

        let upper = byte.to_ascii_uppercase();
        if !CODE_ALPHABET.contains(&upper) || count >= PAIRING_CODE_CHARS {
            return Err(PairingError::InvalidPairingCode);
        }
        normalized[count] = upper;
        count += 1;
    }

    if count != PAIRING_CODE_CHARS {
        return Err(PairingError::InvalidPairingCode);
    }

    Ok(normalized)
}

fn format_pairing_code(normalized: &[u8; PAIRING_CODE_CHARS]) -> String {
    let mut display = String::with_capacity(PAIRING_CODE_CHARS + 3);
    for (index, byte) in normalized.iter().enumerate() {
        if index > 0 && index % 4 == 0 {
            display.push('-');
        }
        display.push(char::from(*byte));
    }
    display
}

fn device_key_fingerprint(public_key: &[u8; PAIRING_PUBLIC_KEY_BYTES]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_DOMAIN);
    hasher.update(public_key);
    let digest = hasher.finalize();
    let mut fingerprint = [0u8; 32];
    fingerprint.copy_from_slice(&digest);
    fingerprint
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
    use ed25519_dalek::{Signer, SigningKey};

    struct FixedRandom {
        bytes: Vec<u8>,
        offset: usize,
    }

    impl FixedRandom {
        fn new(bytes: Vec<u8>) -> Self {
            Self { bytes, offset: 0 }
        }
    }

    impl PairingRandomSource for FixedRandom {
        fn fill_bytes(
            &mut self,
            destination: &mut [u8],
        ) -> Result<(), PairingRandomSourceError> {
            let end = self.offset + destination.len();
            destination.copy_from_slice(&self.bytes[self.offset..end]);
            self.offset = end;
            Ok(())
        }
    }

    fn enabled_lan() -> LanEnableControl {
        let mut control = LanEnableControl::default();
        control.enable_for_this_run();
        control
    }

    fn deterministic_invitation(
        pairing: &mut DevicePairing,
        lan: &LanEnableControl,
        now: Instant,
    ) -> PairingInvitation {
        let bytes: Vec<u8> = (0u8..48u8).collect();
        let mut random = FixedRandom::new(bytes);
        pairing
            .begin_with_source(lan, now, &mut random)
            .expect("pairing begins")
    }

    fn valid_submission(
        invitation: &PairingInvitation,
        signing_key: &SigningKey,
        device_name: &str,
    ) -> PairingSubmission {
        let public_key = signing_key.verifying_key().to_bytes();
        let transcript =
            pairing_transcript(invitation.challenge(), device_name, &public_key).expect("transcript");
        let proof = pairing_proof(
            invitation.code().expose_secret(),
            invitation.challenge(),
            device_name,
            &public_key,
        )
        .expect("proof");
        let signature_message = pairing_signature_message(&transcript, &proof);
        let signature = signing_key.sign(&signature_message).to_bytes();

        PairingSubmission {
            device_name: device_name.to_string(),
            public_key,
            code_proof: proof,
            signature,
        }
    }

    #[test]
    fn pairing_requires_explicit_lan_control() {
        let mut pairing = DevicePairing::default();
        let lan = LanEnableControl::default();
        let now = Instant::now();
        assert_eq!(
            pairing.begin(&lan, now).unwrap_err(),
            PairingError::LanControlDisabled
        );
    }

    #[test]
    fn generated_code_is_grouped_short_lived_and_redacted_in_debug() {
        let mut pairing = DevicePairing::default();
        let lan = enabled_lan();
        let now = Instant::now();
        let invitation = deterministic_invitation(&mut pairing, &lan, now);

        let code = invitation.code().expose_secret();
        assert_eq!(code.len(), 19);
        assert_eq!(code.as_bytes()[4], b'-');
        assert_eq!(code.as_bytes()[9], b'-');
        assert_eq!(code.as_bytes()[14], b'-');
        assert_eq!(invitation.remaining(now), PAIRING_LIFETIME);
        assert!(!format!("{:?}", invitation.code()).contains(code));
    }

    #[test]
    fn valid_hmac_and_ed25519_proof_requires_explicit_approval() {
        let mut pairing = DevicePairing::default();
        let lan = enabled_lan();
        let now = Instant::now();
        let invitation = deterministic_invitation(&mut pairing, &lan, now);
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);

        let submission = valid_submission(&invitation, &signing_key, "Pixel 10");
        let summary = pairing
            .submit(&lan, now + Duration::from_secs(1), submission)
            .expect("proof accepted");

        assert_eq!(summary.device_name(), "Pixel 10");
        assert!(matches!(
            pairing.status(now + Duration::from_secs(1)),
            PairingStatus::AwaitingApproval { .. }
        ));

        let candidate = pairing
            .approve(&lan, now + Duration::from_secs(2))
            .expect("explicit approval");
        assert_eq!(candidate.device_name(), "Pixel 10");
        assert_eq!(
            candidate.public_key(),
            &signing_key.verifying_key().to_bytes()
        );
        assert!(matches!(pairing.status(now), PairingStatus::Idle));
    }

    #[test]
    fn proof_failures_are_bounded_and_use_one_generic_error() {
        let mut pairing = DevicePairing::default();
        let lan = enabled_lan();
        let now = Instant::now();
        let invitation = deterministic_invitation(&mut pairing, &lan, now);
        let signing_key = SigningKey::from_bytes(&[9u8; 32]);
        let mut submission = valid_submission(&invitation, &signing_key, "Phone");
        submission.code_proof = [0u8; PAIRING_PROOF_BYTES];

        for _ in 0..(MAX_PAIRING_ATTEMPTS - 1) {
            assert_eq!(
                pairing
                    .submit(&lan, now + Duration::from_secs(1), submission.clone())
                    .unwrap_err(),
                PairingError::ProofRejected
            );
        }

        assert_eq!(
            pairing
                .submit(&lan, now + Duration::from_secs(1), submission)
                .unwrap_err(),
            PairingError::AttemptsExhausted
        );
        assert!(matches!(pairing.status(now), PairingStatus::Idle));
    }

    #[test]
    fn expired_session_cannot_accept_proof() {
        let mut pairing = DevicePairing::default();
        let lan = enabled_lan();
        let now = Instant::now();
        let invitation = deterministic_invitation(&mut pairing, &lan, now);
        let signing_key = SigningKey::from_bytes(&[11u8; 32]);
        let submission = valid_submission(&invitation, &signing_key, "Tablet");

        assert_eq!(
            pairing
                .submit(&lan, now + PAIRING_LIFETIME, submission)
                .unwrap_err(),
            PairingError::Expired
        );
        assert!(matches!(pairing.status(now), PairingStatus::Idle));
    }

    #[test]
    fn disabling_lan_control_cancels_pairing_authority() {
        let mut pairing = DevicePairing::default();
        let mut lan = enabled_lan();
        let now = Instant::now();
        let invitation = deterministic_invitation(&mut pairing, &lan, now);
        let signing_key = SigningKey::from_bytes(&[13u8; 32]);
        let submission = valid_submission(&invitation, &signing_key, "Laptop");

        lan.disable();
        assert_eq!(
            pairing.submit(&lan, now, submission).unwrap_err(),
            PairingError::LanControlDisabled
        );
        assert!(matches!(pairing.status(now), PairingStatus::Idle));
    }

    #[test]
    fn code_normalization_accepts_grouping_but_rejects_wrong_alphabet() {
        let challenge = [3u8; PAIRING_CHALLENGE_BYTES];
        let signing_key = SigningKey::from_bytes(&[17u8; 32]);
        let public_key = signing_key.verifying_key().to_bytes();

        let grouped = "ABCD-EFGH-JKLM-NPQR";
        let compact = "ABCDEFGHJKLMNPQR";
        assert_eq!(
            pairing_proof(grouped, &challenge, "Phone", &public_key).expect("grouped"),
            pairing_proof(compact, &challenge, "Phone", &public_key).expect("compact")
        );
        assert_eq!(
            pairing_proof("IIII-IIII-IIII-IIII", &challenge, "Phone", &public_key).unwrap_err(),
            PairingError::InvalidPairingCode
        );
    }
}
