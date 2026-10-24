use dangomushi::peer_identity::{
    self, PeerRandomSource, PeerRandomSourceError, PeerVerificationSubmission,
    PEER_VERIFICATION_CHALLENGE_BYTES,
};
use dangomushi::peer_transfer::{self, ReceivedTransfer, TransferPayload};
use dangomushi::trusted_peers;
use dangomushi::workspace;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const INTEGRITY_SOURCE: &str = include_str!("../src/received_file_integrity.rs");
const TRANSFER_SOURCE: &str = include_str!("../src/peer_transfer.rs");
const SERVER_SOURCE: &str = include_str!("../src/server.rs");
const SESSION_SOURCE: &str = include_str!("../src/session_state.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/RECEIVED_FILE_INTEGRITY.md");
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
        "dangomushi-received-integrity-{label}-{}-{unique}",
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

struct Pair {
    sender_root: PathBuf,
    receiver_root: PathBuf,
    sender: dangomushi::peer_identity::LocalPeerIdentity,
    receiver: dangomushi::peer_identity::LocalPeerIdentity,
}

impl Pair {
    fn new(label: &str) -> Self {
        let sender_root = temporary_workspace(&format!("{label}-sender"));
        let receiver_root = temporary_workspace(&format!("{label}-receiver"));
        workspace::initialize(&sender_root).expect("sender workspace");
        workspace::initialize(&receiver_root).expect("receiver workspace");

        let sender = initialize_identity(&sender_root, 61);
        let receiver = initialize_identity(&receiver_root, 62);

        let receiver_candidate = verified_candidate(&receiver, "Receiver", 71);
        trusted_peers::trust_verified_peer(&sender_root, &receiver_candidate)
            .expect("sender trusts receiver");

        let sender_candidate = verified_candidate(&sender, "Sender", 72);
        trusted_peers::trust_verified_peer(&receiver_root, &sender_candidate)
            .expect("receiver trusts sender");

        Self {
            sender_root,
            receiver_root,
            sender,
            receiver,
        }
    }
}

impl Drop for Pair {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.sender_root);
        let _ = fs::remove_dir_all(&self.receiver_root);
    }
}

#[test]
fn encrypted_file_arrives_pending_then_verifies_against_sender_commitment() {
    let pair = Pair::new("roundtrip");
    let now = Instant::now();
    let pending_receive =
        peer_transfer::begin_receive(&pair.receiver_root, pair.sender.fingerprint(), now)
            .expect("receive offer");

    let bytes = b"abc".to_vec();
    let envelope = peer_transfer::seal_transfer(
        &pair.sender_root,
        &pending_receive.offer_bytes(),
        TransferPayload::File {
            suggested_name: "abc.bin".to_string(),
            bytes: bytes.clone(),
        },
    )
    .expect("seal file");

    let received = peer_transfer::open_transfer(
        &pair.receiver_root,
        pending_receive,
        &envelope,
        now + Duration::from_secs(1),
    )
    .expect("open transfer");

    let ReceivedTransfer::File(pending_file) = received else {
        panic!("expected file");
    };

    assert_eq!(pending_file.suggested_name(), "abc.bin");
    assert_eq!(pending_file.declared_len(), 3);
    assert_eq!(
        pending_file.expected_sha256_hex(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(pending_file.sender_fingerprint(), pair.sender.fingerprint());

    let verified = pending_file.verify().expect("verify sender commitment");
    assert_eq!(verified.bytes(), bytes.as_slice());
    assert_eq!(verified.bytes_verified(), 3);
    assert_eq!(
        verified.sha256_hex(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(verified.sender_fingerprint(), pair.sender.fingerprint());
}

#[test]
fn verification_state_does_not_create_a_received_file_on_disk() {
    let pair = Pair::new("no-write");
    let now = Instant::now();
    let pending_receive =
        peer_transfer::begin_receive(&pair.receiver_root, pair.sender.fingerprint(), now)
            .expect("receive offer");
    let envelope = peer_transfer::seal_transfer(
        &pair.sender_root,
        &pending_receive.offer_bytes(),
        TransferPayload::File {
            suggested_name: "never-written.bin".to_string(),
            bytes: vec![0xA5; 4096],
        },
    )
    .expect("seal file");

    let received = peer_transfer::open_transfer(
        &pair.receiver_root,
        pending_receive,
        &envelope,
        now + Duration::from_secs(1),
    )
    .expect("open transfer");

    let ReceivedTransfer::File(pending_file) = received else {
        panic!("expected file");
    };
    let verified = pending_file.verify().expect("verify");
    assert_eq!(verified.bytes_verified(), 4096);

    assert!(!pair.receiver_root.join("never-written.bin").exists());
    assert!(!pair
        .receiver_root
        .join("data/documents/never-written.bin")
        .exists());
}

#[test]
fn pending_type_keeps_raw_bytes_behind_verification_gate() {
    let start = INTEGRITY_SOURCE
        .find("impl PendingReceivedFile")
        .expect("pending impl");
    let end = INTEGRITY_SOURCE[start..]
        .find("impl fmt::Debug for PendingReceivedFile")
        .map(|offset| start + offset)
        .expect("pending impl end");
    let pending_impl = &INTEGRITY_SOURCE[start..end];

    assert!(pending_impl.contains("pub fn verify(self)"));
    assert!(pending_impl.contains("expected_sha256"));
    assert!(pending_impl.contains("declared_len"));
    assert!(!pending_impl.contains("pub fn bytes("));
    assert!(!pending_impl.contains("pub fn into_bytes("));

    let verified_start = INTEGRITY_SOURCE
        .find("impl VerifiedReceivedFile")
        .expect("verified impl");
    let verified_end = INTEGRITY_SOURCE[verified_start..]
        .find("impl fmt::Debug for VerifiedReceivedFile")
        .map(|offset| verified_start + offset)
        .expect("verified impl end");
    let verified_impl = &INTEGRITY_SOURCE[verified_start..verified_end];
    assert!(verified_impl.contains("pub fn bytes(&self)"));
    assert!(verified_impl.contains("pub fn into_bytes(self)"));
}

#[test]
fn payload_v2_carries_authenticated_sha256_record() {
    for required in [
        "PAYLOAD_FORMAT_VERSION: u16 = 2",
        "FILE_INTEGRITY_ALGORITHM_SHA256",
        "RECEIVED_FILE_SHA256_BYTES",
        "received_file_integrity::sha256_bytes(&bytes)",
        "PendingReceivedFile::from_authenticated_payload",
    ] {
        assert!(
            TRANSFER_SOURCE.contains(required),
            "missing file-integrity framing evidence: {required}"
        );
    }

    assert!(TRANSFER_SOURCE.contains("encoded.extend_from_slice(&sha256)"));
    assert!(TRANSFER_SOURCE.contains("algorithm != FILE_INTEGRITY_ALGORITHM_SHA256"));
}

#[test]
fn integrity_core_is_ephemeral_and_opens_no_new_authority() {
    let production = INTEGRITY_SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    for forbidden in [
        "std::fs",
        "fs::write",
        "File::create",
        "OpenOptions",
        "std::net",
        "TcpListener",
        "TcpStream",
        "UdpSocket",
        "workspace_backup",
        "session_state",
    ] {
        assert!(
            !production.contains(forbidden),
            "integrity core must not gain persistence/transport authority: {forbidden}"
        );
    }

    assert!(SERVER_SOURCE.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(!SERVER_SOURCE.contains("0.0.0.0"));
    assert!(!SERVER_SOURCE.contains("received_file_integrity::"));

    for forbidden in [
        "received_file_bytes=",
        "received_file_sha256=",
        "received_file_verified=",
    ] {
        assert!(!SESSION_SOURCE.contains(forbidden));
    }
}

#[test]
fn docs_and_roadmap_keep_integrity_meaning_narrow_and_advance_once() {
    for required in [
        "sender-authenticated commitment",
        "PendingReceivedFile",
        "VerifiedReceivedFile",
        "not malware",
        "no received file is written",
    ] {
        assert!(
            DOC.contains(required) || ARCHITECTURE.contains(required),
            "missing integrity boundary documentation: {required}"
        );
    }

    assert!(UI.contains("RECEIVED FILE INTEGRITY READY"));
    assert!(ROADMAP.contains("- [x] **Integrity verification for received files**"));
    assert!(ROADMAP.contains("- [x] **Direct LAN path first**"));
    assert!(ROADMAP.contains("- [x] **Internet P2P threat review**"));
}
