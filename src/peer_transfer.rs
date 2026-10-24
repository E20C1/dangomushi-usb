use crate::peer_identity::{
    self, LocalPeerIdentity, PEER_FINGERPRINT_BYTES, PEER_SIGNATURE_BYTES,
};
use crate::received_file_integrity::{
    self, PendingReceivedFile, RECEIVED_FILE_SHA256_BYTES,
};
use crate::trusted_peers::{self, TrustedPeer};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use ed25519_dalek::{Signature, VerifyingKey};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt;
use std::path::{Component, Path};
use std::time::{Duration, Instant};
use x25519_dalek::{EphemeralSecret, PublicKey};
use zeroize::Zeroizing;

const OFFER_MAGIC: &[u8; 8] = b"DGMPOFR1";
const ENVELOPE_MAGIC: &[u8; 8] = b"DGMPXFR1";
const PAYLOAD_MAGIC: &[u8; 8] = b"DGMPAY01";
const FORMAT_VERSION: u16 = 1;
const PAYLOAD_FORMAT_VERSION: u16 = 2;

const OFFER_SIGNATURE_DOMAIN: &[u8] = b"DANGOMUSHI-PEER-TRANSFER-OFFER-SIGNATURE-V1\0";
const ENVELOPE_SIGNATURE_DOMAIN: &[u8] = b"DANGOMUSHI-PEER-TRANSFER-ENVELOPE-SIGNATURE-V1\0";
const KDF_INFO_DOMAIN: &[u8] = b"DANGOMUSHI-PEER-TRANSFER-HKDF-SHA256-V1\0";

const TRANSFER_ID_BYTES: usize = 32;
const X25519_PUBLIC_BYTES: usize = 32;
const NONCE_BYTES: usize = 24;
const AEAD_TAG_BYTES: usize = 16;
const OFFER_UNSIGNED_BYTES: usize =
    8 + 2 + TRANSFER_ID_BYTES + PEER_FINGERPRINT_BYTES * 2 + X25519_PUBLIC_BYTES;
pub const PEER_TRANSFER_OFFER_BYTES: usize = OFFER_UNSIGNED_BYTES + PEER_SIGNATURE_BYTES;
const ENVELOPE_HEADER_BYTES: usize =
    8 + 2 + TRANSFER_ID_BYTES + PEER_FINGERPRINT_BYTES * 2 + X25519_PUBLIC_BYTES * 2 + NONCE_BYTES + 8;
const PAYLOAD_HEADER_BYTES: usize = 8 + 2 + 1 + 2 + 8;
const FILE_INTEGRITY_ALGORITHM_SHA256: u8 = 1;
const FILE_INTEGRITY_RECORD_BYTES: usize = 1 + RECEIVED_FILE_SHA256_BYTES;

const PAYLOAD_KIND_TEXT: u8 = 1;
const PAYLOAD_KIND_FILE: u8 = 2;

pub const PEER_TRANSFER_OFFER_LIFETIME: Duration = Duration::from_secs(5 * 60);
pub const MAX_TRANSFER_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TRANSFER_FILE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_TRANSFER_FILE_NAME_BYTES: usize = 255;
pub const MAX_TRANSFER_PLAINTEXT_BYTES: usize =
    PAYLOAD_HEADER_BYTES
        + MAX_TRANSFER_FILE_NAME_BYTES
        + FILE_INTEGRITY_RECORD_BYTES
        + MAX_TRANSFER_FILE_BYTES;
pub const MAX_ENCRYPTED_TRANSFER_BYTES: usize =
    ENVELOPE_HEADER_BYTES + MAX_TRANSFER_PLAINTEXT_BYTES + AEAD_TAG_BYTES + PEER_SIGNATURE_BYTES;

pub struct PendingPeerReceive {
    offer: PeerTransferOffer,
    receiver_secret: EphemeralSecret,
    expires_at: Instant,
}

impl PendingPeerReceive {
    pub fn offer_bytes(&self) -> Vec<u8> {
        self.offer.encode()
    }

    pub const fn transfer_id(&self) -> &[u8; TRANSFER_ID_BYTES] {
        &self.offer.transfer_id
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.expires_at.saturating_duration_since(now)
    }
}

impl fmt::Debug for PendingPeerReceive {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingPeerReceive")
            .field("offer", &self.offer)
            .field("receiver_secret", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum TransferPayload {
    Text(String),
    File { suggested_name: String, bytes: Vec<u8> },
}

impl fmt::Debug for TransferPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => formatter
                .debug_struct("TransferPayload::Text")
                .field("bytes", &text.len())
                .finish(),
            Self::File {
                suggested_name: _,
                bytes,
            } => formatter
                .debug_struct("TransferPayload::File")
                .field("suggested_name", &"[REDACTED]")
                .field("bytes", &bytes.len())
                .finish(),
        }
    }
}

#[derive(PartialEq, Eq)]
pub enum ReceivedTransfer {
    Text(String),
    File(PendingReceivedFile),
}

impl fmt::Debug for ReceivedTransfer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => formatter
                .debug_struct("ReceivedTransfer::Text")
                .field("bytes", &text.len())
                .finish(),
            Self::File(file) => formatter
                .debug_tuple("ReceivedTransfer::File")
                .field(file)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerTransferError {
    WorkspaceUnavailable,
    LocalIdentityUnavailable,
    PeerNotTrusted,
    InvalidOffer,
    OfferExpired,
    InvalidEnvelope,
    InvalidPayload,
    InvalidFileName,
    PayloadTooLarge,
    RandomSourceUnavailable,
    NonContributoryKey,
    AuthenticationFailed,
}

impl fmt::Display for PeerTransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::WorkspaceUnavailable => "peer-transfer workspace state is unavailable",
            Self::LocalIdentityUnavailable => "local peer identity is unavailable",
            Self::PeerNotTrusted => "peer is not currently trusted",
            Self::InvalidOffer => "peer transfer offer is invalid",
            Self::OfferExpired => "peer transfer offer expired",
            Self::InvalidEnvelope => "encrypted peer transfer envelope is invalid",
            Self::InvalidPayload => "decrypted peer transfer payload is invalid",
            Self::InvalidFileName => "peer transfer file name is invalid",
            Self::PayloadTooLarge => "peer transfer payload exceeds the configured limit",
            Self::RandomSourceUnavailable => "operating-system random source is unavailable",
            Self::NonContributoryKey => "peer key agreement was non-contributory",
            Self::AuthenticationFailed => "peer transfer authentication failed",
        };
        formatter.write_str(message)
    }
}

impl Error for PeerTransferError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PeerTransferOffer {
    transfer_id: [u8; TRANSFER_ID_BYTES],
    recipient_fingerprint: [u8; PEER_FINGERPRINT_BYTES],
    expected_sender_fingerprint: [u8; PEER_FINGERPRINT_BYTES],
    recipient_ephemeral_public: [u8; X25519_PUBLIC_BYTES],
    signature: [u8; PEER_SIGNATURE_BYTES],
}

impl PeerTransferOffer {
    fn unsigned_bytes(&self) -> Vec<u8> {
        encode_offer_unsigned(
            &self.transfer_id,
            &self.recipient_fingerprint,
            &self.expected_sender_fingerprint,
            &self.recipient_ephemeral_public,
        )
    }

    fn encode(&self) -> Vec<u8> {
        let mut encoded = self.unsigned_bytes();
        encoded.extend_from_slice(&self.signature);
        debug_assert_eq!(encoded.len(), PEER_TRANSFER_OFFER_BYTES);
        encoded
    }

    fn decode(encoded: &[u8]) -> Result<Self, PeerTransferError> {
        if encoded.len() != PEER_TRANSFER_OFFER_BYTES {
            return Err(PeerTransferError::InvalidOffer);
        }
        if &encoded[..8] != OFFER_MAGIC {
            return Err(PeerTransferError::InvalidOffer);
        }
        if read_u16(encoded, 8).ok_or(PeerTransferError::InvalidOffer)? != FORMAT_VERSION {
            return Err(PeerTransferError::InvalidOffer);
        }

        let mut offset = 10;
        let transfer_id = take_array::<TRANSFER_ID_BYTES>(encoded, &mut offset)
            .ok_or(PeerTransferError::InvalidOffer)?;
        let recipient_fingerprint = take_array::<PEER_FINGERPRINT_BYTES>(encoded, &mut offset)
            .ok_or(PeerTransferError::InvalidOffer)?;
        let expected_sender_fingerprint =
            take_array::<PEER_FINGERPRINT_BYTES>(encoded, &mut offset)
                .ok_or(PeerTransferError::InvalidOffer)?;
        let recipient_ephemeral_public =
            take_array::<X25519_PUBLIC_BYTES>(encoded, &mut offset)
                .ok_or(PeerTransferError::InvalidOffer)?;
        let signature = take_array::<PEER_SIGNATURE_BYTES>(encoded, &mut offset)
            .ok_or(PeerTransferError::InvalidOffer)?;

        if offset != encoded.len() {
            return Err(PeerTransferError::InvalidOffer);
        }

        Ok(Self {
            transfer_id,
            recipient_fingerprint,
            expected_sender_fingerprint,
            recipient_ephemeral_public,
            signature,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnvelopeHeader {
    transfer_id: [u8; TRANSFER_ID_BYTES],
    sender_fingerprint: [u8; PEER_FINGERPRINT_BYTES],
    recipient_fingerprint: [u8; PEER_FINGERPRINT_BYTES],
    sender_ephemeral_public: [u8; X25519_PUBLIC_BYTES],
    recipient_ephemeral_public: [u8; X25519_PUBLIC_BYTES],
    nonce: [u8; NONCE_BYTES],
    ciphertext_len: u64,
}

impl EnvelopeHeader {
    fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(ENVELOPE_HEADER_BYTES);
        encoded.extend_from_slice(ENVELOPE_MAGIC);
        encoded.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
        encoded.extend_from_slice(&self.transfer_id);
        encoded.extend_from_slice(&self.sender_fingerprint);
        encoded.extend_from_slice(&self.recipient_fingerprint);
        encoded.extend_from_slice(&self.sender_ephemeral_public);
        encoded.extend_from_slice(&self.recipient_ephemeral_public);
        encoded.extend_from_slice(&self.nonce);
        encoded.extend_from_slice(&self.ciphertext_len.to_be_bytes());
        debug_assert_eq!(encoded.len(), ENVELOPE_HEADER_BYTES);
        encoded
    }

    fn decode(encoded: &[u8]) -> Result<Self, PeerTransferError> {
        if encoded.len() != ENVELOPE_HEADER_BYTES
            || &encoded[..8] != ENVELOPE_MAGIC
            || read_u16(encoded, 8) != Some(FORMAT_VERSION)
        {
            return Err(PeerTransferError::InvalidEnvelope);
        }

        let mut offset = 10;
        let transfer_id = take_array::<TRANSFER_ID_BYTES>(encoded, &mut offset)
            .ok_or(PeerTransferError::InvalidEnvelope)?;
        let sender_fingerprint = take_array::<PEER_FINGERPRINT_BYTES>(encoded, &mut offset)
            .ok_or(PeerTransferError::InvalidEnvelope)?;
        let recipient_fingerprint = take_array::<PEER_FINGERPRINT_BYTES>(encoded, &mut offset)
            .ok_or(PeerTransferError::InvalidEnvelope)?;
        let sender_ephemeral_public = take_array::<X25519_PUBLIC_BYTES>(encoded, &mut offset)
            .ok_or(PeerTransferError::InvalidEnvelope)?;
        let recipient_ephemeral_public = take_array::<X25519_PUBLIC_BYTES>(encoded, &mut offset)
            .ok_or(PeerTransferError::InvalidEnvelope)?;
        let nonce = take_array::<NONCE_BYTES>(encoded, &mut offset)
            .ok_or(PeerTransferError::InvalidEnvelope)?;
        let ciphertext_len = take_array::<8>(encoded, &mut offset)
            .map(u64::from_be_bytes)
            .ok_or(PeerTransferError::InvalidEnvelope)?;

        if offset != encoded.len() {
            return Err(PeerTransferError::InvalidEnvelope);
        }

        Ok(Self {
            transfer_id,
            sender_fingerprint,
            recipient_fingerprint,
            sender_ephemeral_public,
            recipient_ephemeral_public,
            nonce,
            ciphertext_len,
        })
    }
}

pub fn begin_receive(
    workspace_root: &Path,
    expected_sender_fingerprint: &[u8; PEER_FINGERPRINT_BYTES],
    now: Instant,
) -> Result<PendingPeerReceive, PeerTransferError> {
    let local = load_local_identity(workspace_root)?;
    let peers = trusted_peers::load(workspace_root)
        .map_err(|_| PeerTransferError::WorkspaceUnavailable)?;
    if !peers.contains(expected_sender_fingerprint) {
        return Err(PeerTransferError::PeerNotTrusted);
    }

    let mut transfer_id = [0_u8; TRANSFER_ID_BYTES];
    getrandom::fill(&mut transfer_id).map_err(|_| PeerTransferError::RandomSourceUnavailable)?;

    let receiver_secret = EphemeralSecret::random();
    let recipient_ephemeral_public = PublicKey::from(&receiver_secret).to_bytes();

    let unsigned = encode_offer_unsigned(
        &transfer_id,
        local.fingerprint(),
        expected_sender_fingerprint,
        &recipient_ephemeral_public,
    );
    let signature = local.sign_message(&offer_signature_message(&unsigned));

    Ok(PendingPeerReceive {
        offer: PeerTransferOffer {
            transfer_id,
            recipient_fingerprint: *local.fingerprint(),
            expected_sender_fingerprint: *expected_sender_fingerprint,
            recipient_ephemeral_public,
            signature,
        },
        receiver_secret,
        expires_at: now + PEER_TRANSFER_OFFER_LIFETIME,
    })
}

pub fn seal_transfer(
    workspace_root: &Path,
    encoded_offer: &[u8],
    payload: TransferPayload,
) -> Result<Vec<u8>, PeerTransferError> {
    let offer = PeerTransferOffer::decode(encoded_offer)?;
    let local = load_local_identity(workspace_root)?;

    if offer.expected_sender_fingerprint != *local.fingerprint() {
        return Err(PeerTransferError::AuthenticationFailed);
    }

    let peers = trusted_peers::load(workspace_root)
        .map_err(|_| PeerTransferError::WorkspaceUnavailable)?;
    let recipient = peers
        .peer_for(&offer.recipient_fingerprint)
        .ok_or(PeerTransferError::PeerNotTrusted)?;

    verify_signature(
        recipient,
        &offer_signature_message(&offer.unsigned_bytes()),
        &offer.signature,
    )
    .map_err(|_| PeerTransferError::AuthenticationFailed)?;

    let plaintext = encode_payload(payload)?;
    let sender_secret = EphemeralSecret::random();
    let sender_ephemeral_public = PublicKey::from(&sender_secret).to_bytes();
    let recipient_ephemeral_public = PublicKey::from(offer.recipient_ephemeral_public);
    let shared = sender_secret.diffie_hellman(&recipient_ephemeral_public);
    if !shared.was_contributory() {
        return Err(PeerTransferError::NonContributoryKey);
    }

    let mut nonce = [0_u8; NONCE_BYTES];
    getrandom::fill(&mut nonce).map_err(|_| PeerTransferError::RandomSourceUnavailable)?;

    let ciphertext_len = plaintext
        .len()
        .checked_add(AEAD_TAG_BYTES)
        .ok_or(PeerTransferError::PayloadTooLarge)?;
    if ciphertext_len > MAX_TRANSFER_PLAINTEXT_BYTES + AEAD_TAG_BYTES {
        return Err(PeerTransferError::PayloadTooLarge);
    }

    let header = EnvelopeHeader {
        transfer_id: offer.transfer_id,
        sender_fingerprint: *local.fingerprint(),
        recipient_fingerprint: offer.recipient_fingerprint,
        sender_ephemeral_public,
        recipient_ephemeral_public: offer.recipient_ephemeral_public,
        nonce,
        ciphertext_len: ciphertext_len as u64,
    };
    let header_bytes = header.encode();
    let key = derive_transfer_key(shared.as_bytes(), &header)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&*key)
        .map_err(|_| PeerTransferError::AuthenticationFailed)?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&header.nonce),
            Payload {
                msg: &plaintext,
                aad: &header_bytes,
            },
        )
        .map_err(|_| PeerTransferError::AuthenticationFailed)?;

    if ciphertext.len() != ciphertext_len {
        return Err(PeerTransferError::AuthenticationFailed);
    }

    let signature_message = envelope_signature_message(&header_bytes, &ciphertext);
    let signature = local.sign_message(&signature_message);

    let total = ENVELOPE_HEADER_BYTES
        .checked_add(ciphertext.len())
        .and_then(|value| value.checked_add(PEER_SIGNATURE_BYTES))
        .ok_or(PeerTransferError::PayloadTooLarge)?;
    if total > MAX_ENCRYPTED_TRANSFER_BYTES {
        return Err(PeerTransferError::PayloadTooLarge);
    }

    let mut encoded = Vec::with_capacity(total);
    encoded.extend_from_slice(&header_bytes);
    encoded.extend_from_slice(&ciphertext);
    encoded.extend_from_slice(&signature);
    Ok(encoded)
}

pub fn open_transfer(
    workspace_root: &Path,
    pending: PendingPeerReceive,
    encoded: &[u8],
    now: Instant,
) -> Result<ReceivedTransfer, PeerTransferError> {
    if now >= pending.expires_at {
        return Err(PeerTransferError::OfferExpired);
    }

    let (header, ciphertext, signature) = parse_envelope(encoded)?;
    if header.transfer_id != pending.offer.transfer_id
        || header.recipient_fingerprint != pending.offer.recipient_fingerprint
        || header.sender_fingerprint != pending.offer.expected_sender_fingerprint
        || header.recipient_ephemeral_public != pending.offer.recipient_ephemeral_public
    {
        return Err(PeerTransferError::AuthenticationFailed);
    }

    let local = load_local_identity(workspace_root)?;
    if header.recipient_fingerprint != *local.fingerprint() {
        return Err(PeerTransferError::AuthenticationFailed);
    }

    let peers = trusted_peers::load(workspace_root)
        .map_err(|_| PeerTransferError::WorkspaceUnavailable)?;
    let sender = peers
        .peer_for(&header.sender_fingerprint)
        .ok_or(PeerTransferError::PeerNotTrusted)?;

    let header_bytes = header.encode();
    verify_signature(
        sender,
        &envelope_signature_message(&header_bytes, ciphertext),
        signature,
    )
    .map_err(|_| PeerTransferError::AuthenticationFailed)?;

    let sender_ephemeral_public = PublicKey::from(header.sender_ephemeral_public);
    let shared = pending
        .receiver_secret
        .diffie_hellman(&sender_ephemeral_public);
    if !shared.was_contributory() {
        return Err(PeerTransferError::NonContributoryKey);
    }

    let key = derive_transfer_key(shared.as_bytes(), &header)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&*key)
        .map_err(|_| PeerTransferError::AuthenticationFailed)?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&header.nonce),
                Payload {
                    msg: ciphertext,
                    aad: &header_bytes,
                },
            )
            .map_err(|_| PeerTransferError::AuthenticationFailed)?,
    );

    if plaintext.len() > MAX_TRANSFER_PLAINTEXT_BYTES {
        return Err(PeerTransferError::PayloadTooLarge);
    }

    decode_payload(&plaintext, header.sender_fingerprint)
}

fn load_local_identity(workspace_root: &Path) -> Result<LocalPeerIdentity, PeerTransferError> {
    peer_identity::load(workspace_root)
        .map_err(|_| PeerTransferError::WorkspaceUnavailable)?
        .ok_or(PeerTransferError::LocalIdentityUnavailable)
}

fn encode_offer_unsigned(
    transfer_id: &[u8; TRANSFER_ID_BYTES],
    recipient_fingerprint: &[u8; PEER_FINGERPRINT_BYTES],
    expected_sender_fingerprint: &[u8; PEER_FINGERPRINT_BYTES],
    recipient_ephemeral_public: &[u8; X25519_PUBLIC_BYTES],
) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(OFFER_UNSIGNED_BYTES);
    encoded.extend_from_slice(OFFER_MAGIC);
    encoded.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
    encoded.extend_from_slice(transfer_id);
    encoded.extend_from_slice(recipient_fingerprint);
    encoded.extend_from_slice(expected_sender_fingerprint);
    encoded.extend_from_slice(recipient_ephemeral_public);
    debug_assert_eq!(encoded.len(), OFFER_UNSIGNED_BYTES);
    encoded
}

fn offer_signature_message(unsigned: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(OFFER_SIGNATURE_DOMAIN.len() + unsigned.len());
    message.extend_from_slice(OFFER_SIGNATURE_DOMAIN);
    message.extend_from_slice(unsigned);
    message
}

fn envelope_signature_message(header: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(ENVELOPE_SIGNATURE_DOMAIN);
    hasher.update(header);
    hasher.update(ciphertext);
    hasher.finalize().to_vec()
}

fn verify_signature(
    peer: &TrustedPeer,
    message: &[u8],
    signature: &[u8; PEER_SIGNATURE_BYTES],
) -> Result<(), PeerTransferError> {
    let verifying_key =
        VerifyingKey::from_bytes(peer.public_key()).map_err(|_| PeerTransferError::AuthenticationFailed)?;
    let signature = Signature::from_bytes(signature);
    verifying_key
        .verify_strict(message, &signature)
        .map_err(|_| PeerTransferError::AuthenticationFailed)
}

fn derive_transfer_key(
    shared_secret: &[u8; 32],
    header: &EnvelopeHeader,
) -> Result<Zeroizing<[u8; 32]>, PeerTransferError> {
    let mut info = Vec::with_capacity(
        KDF_INFO_DOMAIN.len()
            + PEER_FINGERPRINT_BYTES * 2
            + X25519_PUBLIC_BYTES * 2,
    );
    info.extend_from_slice(KDF_INFO_DOMAIN);
    info.extend_from_slice(&header.sender_fingerprint);
    info.extend_from_slice(&header.recipient_fingerprint);
    info.extend_from_slice(&header.sender_ephemeral_public);
    info.extend_from_slice(&header.recipient_ephemeral_public);

    let hkdf = Hkdf::<Sha256>::new(Some(&header.transfer_id), shared_secret);
    let mut key = Zeroizing::new([0_u8; 32]);
    hkdf.expand(&info, &mut *key)
        .map_err(|_| PeerTransferError::AuthenticationFailed)?;
    Ok(key)
}

fn encode_payload(payload: TransferPayload) -> Result<Zeroizing<Vec<u8>>, PeerTransferError> {
    match payload {
        TransferPayload::Text(text) => {
            if text.len() > MAX_TRANSFER_TEXT_BYTES {
                return Err(PeerTransferError::PayloadTooLarge);
            }

            let body = text.into_bytes();
            let body_len =
                u64::try_from(body.len()).map_err(|_| PeerTransferError::PayloadTooLarge)?;
            let total = PAYLOAD_HEADER_BYTES
                .checked_add(body.len())
                .ok_or(PeerTransferError::PayloadTooLarge)?;
            if total > MAX_TRANSFER_PLAINTEXT_BYTES {
                return Err(PeerTransferError::PayloadTooLarge);
            }

            let mut encoded = Vec::with_capacity(total);
            encoded.extend_from_slice(PAYLOAD_MAGIC);
            encoded.extend_from_slice(&PAYLOAD_FORMAT_VERSION.to_be_bytes());
            encoded.push(PAYLOAD_KIND_TEXT);
            encoded.extend_from_slice(&0_u16.to_be_bytes());
            encoded.extend_from_slice(&body_len.to_be_bytes());
            encoded.extend_from_slice(&body);
            Ok(Zeroizing::new(encoded))
        }
        TransferPayload::File {
            suggested_name,
            bytes,
        } => {
            validate_file_name(&suggested_name)?;
            if bytes.len() > MAX_TRANSFER_FILE_BYTES {
                return Err(PeerTransferError::PayloadTooLarge);
            }

            let name = suggested_name.into_bytes();
            let name_len =
                u16::try_from(name.len()).map_err(|_| PeerTransferError::InvalidFileName)?;
            let body_len =
                u64::try_from(bytes.len()).map_err(|_| PeerTransferError::PayloadTooLarge)?;
            let sha256 = received_file_integrity::sha256_bytes(&bytes);

            let total = PAYLOAD_HEADER_BYTES
                .checked_add(name.len())
                .and_then(|value| value.checked_add(FILE_INTEGRITY_RECORD_BYTES))
                .and_then(|value| value.checked_add(bytes.len()))
                .ok_or(PeerTransferError::PayloadTooLarge)?;
            if total > MAX_TRANSFER_PLAINTEXT_BYTES {
                return Err(PeerTransferError::PayloadTooLarge);
            }

            let mut encoded = Vec::with_capacity(total);
            encoded.extend_from_slice(PAYLOAD_MAGIC);
            encoded.extend_from_slice(&PAYLOAD_FORMAT_VERSION.to_be_bytes());
            encoded.push(PAYLOAD_KIND_FILE);
            encoded.extend_from_slice(&name_len.to_be_bytes());
            encoded.extend_from_slice(&body_len.to_be_bytes());
            encoded.extend_from_slice(&name);
            encoded.push(FILE_INTEGRITY_ALGORITHM_SHA256);
            encoded.extend_from_slice(&sha256);
            encoded.extend_from_slice(&bytes);
            Ok(Zeroizing::new(encoded))
        }
    }
}

fn decode_payload(
    encoded: &[u8],
    sender_fingerprint: [u8; PEER_FINGERPRINT_BYTES],
) -> Result<ReceivedTransfer, PeerTransferError> {
    if encoded.len() < PAYLOAD_HEADER_BYTES
        || &encoded[..8] != PAYLOAD_MAGIC
        || read_u16(encoded, 8) != Some(PAYLOAD_FORMAT_VERSION)
    {
        return Err(PeerTransferError::InvalidPayload);
    }

    let kind = encoded[10];
    let name_len = read_u16(encoded, 11).ok_or(PeerTransferError::InvalidPayload)? as usize;
    let body_len_u64 = read_u64(encoded, 13).ok_or(PeerTransferError::InvalidPayload)?;
    let body_len = usize::try_from(body_len_u64).map_err(|_| PeerTransferError::PayloadTooLarge)?;

    if name_len > MAX_TRANSFER_FILE_NAME_BYTES || encoded.len() > MAX_TRANSFER_PLAINTEXT_BYTES {
        return Err(PeerTransferError::InvalidPayload);
    }

    let name_start = PAYLOAD_HEADER_BYTES;
    let name_end = name_start
        .checked_add(name_len)
        .ok_or(PeerTransferError::InvalidPayload)?;
    let name_bytes = encoded
        .get(name_start..name_end)
        .ok_or(PeerTransferError::InvalidPayload)?;

    match kind {
        PAYLOAD_KIND_TEXT => {
            if name_len != 0 || body_len > MAX_TRANSFER_TEXT_BYTES {
                return Err(PeerTransferError::InvalidPayload);
            }
            let expected_len = PAYLOAD_HEADER_BYTES
                .checked_add(body_len)
                .ok_or(PeerTransferError::InvalidPayload)?;
            if expected_len != encoded.len() {
                return Err(PeerTransferError::InvalidPayload);
            }

            let body = &encoded[PAYLOAD_HEADER_BYTES..];
            let text = std::str::from_utf8(body)
                .map_err(|_| PeerTransferError::InvalidPayload)?
                .to_owned();
            Ok(ReceivedTransfer::Text(text))
        }
        PAYLOAD_KIND_FILE => {
            if body_len > MAX_TRANSFER_FILE_BYTES {
                return Err(PeerTransferError::PayloadTooLarge);
            }

            let integrity_start = name_end;
            let integrity_end = integrity_start
                .checked_add(FILE_INTEGRITY_RECORD_BYTES)
                .ok_or(PeerTransferError::InvalidPayload)?;
            let body_start = integrity_end;
            let expected_len = body_start
                .checked_add(body_len)
                .ok_or(PeerTransferError::InvalidPayload)?;
            if expected_len != encoded.len() {
                return Err(PeerTransferError::InvalidPayload);
            }

            let algorithm = *encoded
                .get(integrity_start)
                .ok_or(PeerTransferError::InvalidPayload)?;
            if algorithm != FILE_INTEGRITY_ALGORITHM_SHA256 {
                return Err(PeerTransferError::InvalidPayload);
            }

            let expected_sha256: [u8; RECEIVED_FILE_SHA256_BYTES] = encoded
                .get(integrity_start + 1..integrity_end)
                .ok_or(PeerTransferError::InvalidPayload)?
                .try_into()
                .map_err(|_| PeerTransferError::InvalidPayload)?;
            let body = encoded
                .get(body_start..)
                .ok_or(PeerTransferError::InvalidPayload)?;

            let suggested_name = std::str::from_utf8(name_bytes)
                .map_err(|_| PeerTransferError::InvalidFileName)?
                .to_owned();
            validate_file_name(&suggested_name)?;

            Ok(ReceivedTransfer::File(
                PendingReceivedFile::from_authenticated_payload(
                    suggested_name,
                    body_len_u64,
                    expected_sha256,
                    sender_fingerprint,
                    body.to_vec(),
                ),
            ))
        }
        _ => Err(PeerTransferError::InvalidPayload),
    }
}

fn validate_file_name(name: &str) -> Result<(), PeerTransferError> {
    if name.is_empty()
        || name.len() > MAX_TRANSFER_FILE_NAME_BYTES
        || name == "."
        || name == ".."
        || name.chars().any(char::is_control)
        || name.contains('/')
        || name.contains('\\')
    {
        return Err(PeerTransferError::InvalidFileName);
    }

    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(PeerTransferError::InvalidFileName);
    }

    Ok(())
}

fn parse_envelope(
    encoded: &[u8],
) -> Result<(EnvelopeHeader, &[u8], &[u8; PEER_SIGNATURE_BYTES]), PeerTransferError> {
    if encoded.len() < ENVELOPE_HEADER_BYTES + AEAD_TAG_BYTES + PEER_SIGNATURE_BYTES
        || encoded.len() > MAX_ENCRYPTED_TRANSFER_BYTES
    {
        return Err(PeerTransferError::InvalidEnvelope);
    }

    let header = EnvelopeHeader::decode(&encoded[..ENVELOPE_HEADER_BYTES])?;
    let ciphertext_len =
        usize::try_from(header.ciphertext_len).map_err(|_| PeerTransferError::PayloadTooLarge)?;
    if ciphertext_len < AEAD_TAG_BYTES
        || ciphertext_len > MAX_TRANSFER_PLAINTEXT_BYTES + AEAD_TAG_BYTES
    {
        return Err(PeerTransferError::InvalidEnvelope);
    }

    let ciphertext_end = ENVELOPE_HEADER_BYTES
        .checked_add(ciphertext_len)
        .ok_or(PeerTransferError::InvalidEnvelope)?;
    let total = ciphertext_end
        .checked_add(PEER_SIGNATURE_BYTES)
        .ok_or(PeerTransferError::InvalidEnvelope)?;
    if total != encoded.len() {
        return Err(PeerTransferError::InvalidEnvelope);
    }

    let ciphertext = &encoded[ENVELOPE_HEADER_BYTES..ciphertext_end];
    let signature: &[u8; PEER_SIGNATURE_BYTES] = encoded[ciphertext_end..]
        .try_into()
        .map_err(|_| PeerTransferError::InvalidEnvelope)?;
    Ok((header, ciphertext, signature))
}

fn read_u16(encoded: &[u8], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = encoded.get(offset..offset + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

fn read_u64(encoded: &[u8], offset: usize) -> Option<u64> {
    let bytes: [u8; 8] = encoded.get(offset..offset + 8)?.try_into().ok()?;
    Some(u64::from_be_bytes(bytes))
}

fn take_array<const N: usize>(encoded: &[u8], offset: &mut usize) -> Option<[u8; N]> {
    let end = offset.checked_add(N)?;
    let bytes: [u8; N] = encoded.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_rejects_paths_and_controls() {
        for invalid in ["", ".", "..", "../x", "a/b", "a\\b", "bad\nname"] {
            assert_eq!(
                validate_file_name(invalid),
                Err(PeerTransferError::InvalidFileName)
            );
        }
        assert!(validate_file_name("notes.txt").is_ok());
    }

    #[test]
    fn payload_debug_does_not_reveal_contents() {
        let payload = TransferPayload::Text("top secret text".to_string());
        let debug = format!("{payload:?}");
        assert!(!debug.contains("top secret text"));
        assert!(debug.contains("bytes"));
    }

    #[test]
    fn offer_decoder_rejects_trailing_bytes() {
        let mut encoded = vec![0_u8; PEER_TRANSFER_OFFER_BYTES + 1];
        encoded[..8].copy_from_slice(OFFER_MAGIC);
        encoded[8..10].copy_from_slice(&FORMAT_VERSION.to_be_bytes());
        assert_eq!(
            PeerTransferOffer::decode(&encoded),
            Err(PeerTransferError::InvalidOffer)
        );
    }
}
