use dangomushi::peer_identity::{
    self, PeerRandomSource, PeerRandomSourceError, PeerVerificationSubmission,
    PEER_VERIFICATION_CHALLENGE_BYTES,
};
use dangomushi::peer_transfer::{
    self, PeerTransferError, ReceivedTransfer, TransferPayload, PEER_TRANSFER_OFFER_LIFETIME,
};
use dangomushi::trusted_peers;
use dangomushi::workspace;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TRANSFER_SOURCE: &str = include_str!("../src/peer_transfer.rs");
const SERVER_SOURCE: &str = include_str!("../src/server.rs");
const SESSION_SOURCE: &str = include_str!("../src/session_state.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/ENCRYPTED_PEER_TRANSFER.md");
const UI: &str = include_str!("../src/native_ui.rs");

struct FixedRandom([u8; PEER_VERIFICATION_CHALLENGE_BYTES]);

impl PeerRandomSource for FixedRandom {
    fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), PeerRandomSourceError> {
        if destination.len() > self.0.len() {
            return Err(PeerRandomSourceError);
        }
        destination.copy_from_slice(&self.0[..destination.len()]);
        Ok(())
    }
}

fn temporary_workspace(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dangomushi-peer-transfer-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn initialize_identity(
    root: &Path,
    seed: u8,
) -> dangomushi::peer_identity::LocalPeerIdentity {
    let mut random = FixedRandom([seed; PEER_VERIFICATION_CHALLENGE_BYTES]);
    peer_identity::initialize_with_source(root, &mut random).expect("initialize identity")
}

fn verified_candidate(
    peer: &dangomushi::peer_identity::LocalPeerIdentity,
    peer_name: &str,
    challenge_byte: u8,
) -> dangomushi::peer_identity::VerifiedPeerCandidate {
    let now = Instant::now();
    let mut random = FixedRandom([challenge_byte; PEER_VERIFICATION_CHALLENGE_BYTES]);
    let challenge =
        peer_identity::begin_verification_with_source(now, &mut random).expect("challenge");
    let signature = peer
        .sign_verification(challenge.bytes(), peer_name)
        .expect("sign verification");

    peer_identity::complete_verification(
        now + Duration::from_secs(1),
        challenge,
        PeerVerificationSubmission {
            peer_name: peer_name.to_string(),
            public_key: *peer.public_key(),
            signature,
        },
    )
    .expect("verified candidate")
}

struct PeerPair {
    a_root: PathBuf,
    b_root: PathBuf,
    a: dangomushi::peer_identity::LocalPeerIdentity,
    b: dangomushi::peer_identity::LocalPeerIdentity,
}

impl PeerPair {
    fn new(label: &str) -> Self {
        let a_root = temporary_workspace(&format!("{label}-a"));
        let b_root = temporary_workspace(&format!("{label}-b"));
        workspace::initialize(&a_root).expect("workspace a");
        workspace::initialize(&b_root).expect("workspace b");

        let a = initialize_identity(&a_root, 21);
        let b = initialize_identity(&b_root, 22);

        let b_candidate = verified_candidate(&b, "Peer B", 31);
        trusted_peers::trust_verified_peer(&a_root, &b_candidate).expect("A trusts B");

        let a_candidate = verified_candidate(&a, "Peer A", 32);
        trusted_peers::trust_verified_peer(&b_root, &a_candidate).expect("B trusts A");

        Self {
            a_root,
            b_root,
            a,
            b,
        }
    }
}

impl Drop for PeerPair {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.a_root);
        let _ = fs::remove_dir_all(&self.b_root);
    }
}

#[test]
fn trusted_peers_round_trip_encrypted_text() {
    let pair = PeerPair::new("text");
    let now = Instant::now();

    let pending =
        peer_transfer::begin_receive(&pair.b_root, pair.a.fingerprint(), now).expect("offer");
    let offer = pending.offer_bytes();
    let encrypted = peer_transfer::seal_transfer(
        &pair.a_root,
        &offer,
        TransferPayload::Text("hello over encrypted peer transfer".to_string()),
    )
    .expect("seal");

    let received =
        peer_transfer::open_transfer(&pair.b_root, pending, &encrypted, now + Duration::from_secs(1))
            .expect("open");

    assert_eq!(
        received,
        ReceivedTransfer::Text("hello over encrypted peer transfer".to_string())
    );
}

#[test]
fn trusted_peers_round_trip_file_without_writing_destination() {
    let pair = PeerPair::new("file");
    let now = Instant::now();
    let pending =
        peer_transfer::begin_receive(&pair.b_root, pair.a.fingerprint(), now).expect("offer");

    let bytes = b"peer file bytes\0with binary".to_vec();
    let encrypted = peer_transfer::seal_transfer(
        &pair.a_root,
        &pending.offer_bytes(),
        TransferPayload::File {
            suggested_name: "transfer.bin".to_string(),
            bytes: bytes.clone(),
        },
    )
    .expect("seal file");

    let received =
        peer_transfer::open_transfer(&pair.b_root, pending, &encrypted, now + Duration::from_secs(1))
            .expect("open file");

    assert_eq!(
        received,
        ReceivedTransfer::File {
            suggested_name: "transfer.bin".to_string(),
            bytes,
        }
    );
    assert!(!pair.b_root.join("transfer.bin").exists());
    assert!(!pair.b_root.join("data/transfer.bin").exists());
}

#[test]
fn signatures_offer_binding_and_expiry_fail_closed() {
    let pair = PeerPair::new("auth");
    let now = Instant::now();

    let pending =
        peer_transfer::begin_receive(&pair.b_root, pair.a.fingerprint(), now).expect("offer");
    let mut tampered_offer = pending.offer_bytes();
    let last = tampered_offer.len() - 1;
    tampered_offer[last] ^= 0x80;
    assert_eq!(
        peer_transfer::seal_transfer(
            &pair.a_root,
            &tampered_offer,
            TransferPayload::Text("should not seal".to_string()),
        ),
        Err(PeerTransferError::AuthenticationFailed)
    );

    let expired =
        peer_transfer::begin_receive(&pair.b_root, pair.a.fingerprint(), now).expect("expired offer");
    let encrypted = peer_transfer::seal_transfer(
        &pair.a_root,
        &expired.offer_bytes(),
        TransferPayload::Text("expires".to_string()),
    )
    .expect("seal expired candidate");
    assert_eq!(
        peer_transfer::open_transfer(
            &pair.b_root,
            expired,
            &encrypted,
            now + PEER_TRANSFER_OFFER_LIFETIME,
        ),
        Err(PeerTransferError::OfferExpired)
    );
}

#[test]
fn envelope_tamper_and_replay_against_new_offer_fail_closed() {
    let pair = PeerPair::new("replay");
    let now = Instant::now();

    let pending =
        peer_transfer::begin_receive(&pair.b_root, pair.a.fingerprint(), now).expect("offer");
    let encrypted = peer_transfer::seal_transfer(
        &pair.a_root,
        &pending.offer_bytes(),
        TransferPayload::Text("authenticated".to_string()),
    )
    .expect("seal");

    let mut tampered = encrypted.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    assert_eq!(
        peer_transfer::open_transfer(
            &pair.b_root,
            pending,
            &tampered,
            now + Duration::from_secs(1),
        ),
        Err(PeerTransferError::AuthenticationFailed)
    );

    let fresh =
        peer_transfer::begin_receive(&pair.b_root, pair.a.fingerprint(), now).expect("fresh offer");
    assert_eq!(
        peer_transfer::open_transfer(
            &pair.b_root,
            fresh,
            &encrypted,
            now + Duration::from_secs(1),
        ),
        Err(PeerTransferError::AuthenticationFailed)
    );
}

#[test]
fn file_name_and_untrusted_peer_boundaries_are_explicit() {
    let pair = PeerPair::new("boundary");
    let now = Instant::now();
    let pending =
        peer_transfer::begin_receive(&pair.b_root, pair.a.fingerprint(), now).expect("offer");

    assert_eq!(
        peer_transfer::seal_transfer(
            &pair.a_root,
            &pending.offer_bytes(),
            TransferPayload::File {
                suggested_name: "../escape.txt".to_string(),
                bytes: b"no".to_vec(),
            },
        ),
        Err(PeerTransferError::InvalidFileName)
    );

    trusted_peers::remove_trusted_peer(&pair.b_root, pair.a.fingerprint()).expect("remove A trust");
    assert_eq!(
        peer_transfer::begin_receive(&pair.b_root, pair.a.fingerprint(), now).unwrap_err(),
        PeerTransferError::PeerNotTrusted
    );
}

#[test]
fn transfer_core_is_ephemeral_transport_independent_and_roadmap_bounded() {
    let production = TRANSFER_SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    for forbidden in [
        "TcpListener",
        "TcpStream",
        "UdpSocket",
        "std::net",
        "fs::write",
        "File::create",
        "OpenOptions",
        "workspace_backup",
        "session_state",
        "device_permissions",
    ] {
        assert!(
            !production.contains(forbidden),
            "transfer core must not open transport/persistence authority: {forbidden}"
        );
    }

    for required in [
        "EphemeralSecret",
        "was_contributory",
        "Hkdf::<Sha256>",
        "XChaCha20Poly1305",
        "ENVELOPE_SIGNATURE_DOMAIN",
        "PEER_TRANSFER_OFFER_LIFETIME",
        "MAX_TRANSFER_FILE_BYTES",
    ] {
        assert!(production.contains(required), "missing core boundary: {required}");
    }

    assert!(SERVER_SOURCE.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(!SERVER_SOURCE.contains("0.0.0.0"));
    assert!(!SERVER_SOURCE.contains("peer_transfer::"));

    for forbidden in [
        "peer_transfer_id=",
        "peer_transfer_key=",
        "peer_transfer_plaintext=",
        "peer_transfer_ciphertext=",
    ] {
        assert!(!SESSION_SOURCE.contains(forbidden));
    }

    for required in [
        "ephemeral X25519",
        "HKDF-SHA256",
        "XChaCha20Poly1305",
        "five minutes",
        "never writes a received file",
    ] {
        assert!(
            ARCHITECTURE.contains(required) || DOC.contains(required),
            "missing documented boundary: {required}"
        );
    }

    assert!(UI.contains("TRANSFER CORE READY"));
    assert!(ROADMAP.contains("- [x] **Encrypted text/file transfer**"));
    assert!(ROADMAP.contains(
        "- [ ] **Integrity verification for received files** — Exactly One Next Task"
    ));
}
