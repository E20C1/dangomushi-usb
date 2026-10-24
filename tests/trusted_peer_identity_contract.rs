use dangomushi::peer_identity::{
    self, PeerIdentityError, PeerRandomSource, PeerRandomSourceError, PeerVerificationSubmission,
    PEER_VERIFICATION_CHALLENGE_BYTES, PEER_VERIFICATION_LIFETIME,
};
use dangomushi::trusted_peers::{self, TrustedPeerError};
use dangomushi::workspace;
use dangomushi::workspace_backup;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PEER_SOURCE: &str = include_str!("../src/peer_identity.rs");
const TRUST_SOURCE: &str = include_str!("../src/trusted_peers.rs");
const DEVICE_PAIRING: &str = include_str!("../src/device_pairing.rs");
const SERVER: &str = include_str!("../src/server.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const WORKSPACE_BACKUP: &str = include_str!("../src/workspace_backup.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/TRUSTED_PEER_IDENTITY.md");

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
        "dangomushi-peer-identity-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn initialize_identity(
    root: &std::path::Path,
    seed: u8,
) -> dangomushi::peer_identity::LocalPeerIdentity {
    let mut random = FixedRandom([seed; PEER_VERIFICATION_CHALLENGE_BYTES]);
    peer_identity::initialize_with_source(root, &mut random).expect("initialize peer identity")
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
        .expect("sign proof");

    peer_identity::complete_verification(
        now + Duration::from_secs(1),
        challenge,
        PeerVerificationSubmission {
            peer_name: peer_name.to_string(),
            public_key: *peer.public_key(),
            signature,
        },
    )
    .expect("verify proof")
}

fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

#[test]
fn local_peer_identity_is_create_once_stable_and_redacted() {
    let root = temporary_workspace("local");
    workspace::initialize(&root).expect("workspace");

    let identity = initialize_identity(&root, 17);
    let expected = identity.summary();
    let debug = format!("{identity:?}");
    assert!(debug.contains("[REDACTED]"));
    drop(identity);

    let loaded = peer_identity::load(&root)
        .expect("load")
        .expect("identity exists");
    assert_eq!(loaded.summary(), expected);

    let mut second = FixedRandom([18u8; PEER_VERIFICATION_CHALLENGE_BYTES]);
    assert_eq!(
        peer_identity::initialize_with_source(&root, &mut second).unwrap_err(),
        PeerIdentityError::AlreadyInitialized
    );

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn peer_trust_requires_matching_fresh_proof_then_explicit_persistence() {
    let verifier_root = temporary_workspace("verifier");
    let peer_root = temporary_workspace("peer");
    workspace::initialize(&verifier_root).expect("verifier workspace");
    workspace::initialize(&peer_root).expect("peer workspace");

    let peer = initialize_identity(&peer_root, 31);
    let now = Instant::now();

    let mut signed_random = FixedRandom([41u8; PEER_VERIFICATION_CHALLENGE_BYTES]);
    let signed_challenge =
        peer_identity::begin_verification_with_source(now, &mut signed_random).expect("signed");
    let signature = peer
        .sign_verification(signed_challenge.bytes(), "Peer B")
        .expect("signature");

    let mut other_random = FixedRandom([42u8; PEER_VERIFICATION_CHALLENGE_BYTES]);
    let other_challenge =
        peer_identity::begin_verification_with_source(now, &mut other_random).expect("other");
    assert_eq!(
        peer_identity::complete_verification(
            now + Duration::from_secs(1),
            other_challenge,
            PeerVerificationSubmission {
                peer_name: "Peer B".to_string(),
                public_key: *peer.public_key(),
                signature,
            },
        ),
        Err(PeerIdentityError::ProofRejected)
    );

    let mut expired_random = FixedRandom([43u8; PEER_VERIFICATION_CHALLENGE_BYTES]);
    let expired =
        peer_identity::begin_verification_with_source(now, &mut expired_random).expect("expired");
    let expired_signature = peer
        .sign_verification(expired.bytes(), "Peer B")
        .expect("expired signature");
    assert_eq!(
        peer_identity::complete_verification(
            now + PEER_VERIFICATION_LIFETIME,
            expired,
            PeerVerificationSubmission {
                peer_name: "Peer B".to_string(),
                public_key: *peer.public_key(),
                signature: expired_signature,
            },
        ),
        Err(PeerIdentityError::VerificationExpired)
    );

    let candidate = verified_candidate(&peer, "Peer B", 44);
    assert!(trusted_peers::load(&verifier_root)
        .expect("empty store")
        .peers()
        .is_empty());

    let trusted =
        trusted_peers::trust_verified_peer(&verifier_root, &candidate).expect("explicit trust");
    assert_eq!(trusted.peer_name(), "Peer B");
    assert!(trusted_peers::load(&verifier_root)
        .expect("trusted store")
        .contains(candidate.fingerprint()));

    assert_eq!(
        trusted_peers::trust_verified_peer(&verifier_root, &candidate).unwrap_err(),
        TrustedPeerError::AlreadyTrusted
    );

    fs::remove_dir_all(verifier_root).expect("verifier cleanup");
    fs::remove_dir_all(peer_root).expect("peer cleanup");
}

#[test]
fn generic_backup_neither_copies_peer_secret_nor_resurrects_removed_trust() {
    let root = temporary_workspace("backup");
    let peer_root = temporary_workspace("backup-peer");
    workspace::initialize(&root).expect("workspace");
    workspace::initialize(&peer_root).expect("peer workspace");
    fs::create_dir_all(root.join("state/autosave")).expect("autosave");
    fs::write(root.join("data/documents/note.txt"), "before").expect("seed document");

    let local = initialize_identity(&root, 51);
    let local_summary = local.summary();
    drop(local);

    let peer = initialize_identity(&peer_root, 52);
    let candidate = verified_candidate(&peer, "Backup Peer", 53);
    trusted_peers::trust_verified_peer(&root, &candidate).expect("trust peer");

    let identity_bytes = fs::read(root.join("state/peer_identity.v1")).expect("identity bytes");
    let trust_bytes = fs::read(root.join("state/trusted_peers.v1.json")).expect("trust bytes");

    let backup = workspace_backup::create_backup(&root).expect("create backup");
    let archive_bytes = fs::read(root.join(&backup.relative_path)).expect("archive");
    assert!(!contains_subsequence(&archive_bytes, &identity_bytes));
    assert!(!contains_subsequence(&archive_bytes, &trust_bytes));

    trusted_peers::remove_trusted_peer(&root, candidate.fingerprint()).expect("remove peer");
    fs::write(root.join("data/documents/note.txt"), "after").expect("modify document");

    workspace_backup::restore_backup(&root, &backup.relative_path).expect("restore backup");

    assert_eq!(
        fs::read_to_string(root.join("data/documents/note.txt")).expect("restored document"),
        "before"
    );
    assert!(trusted_peers::load(&root)
        .expect("trust after restore")
        .peers()
        .is_empty());
    assert_eq!(
        peer_identity::load(&root)
            .expect("identity load")
            .expect("identity")
            .summary(),
        local_summary
    );

    fs::remove_dir_all(root).expect("cleanup");
    fs::remove_dir_all(peer_root).expect("peer cleanup");
}

#[test]
fn peer_identity_is_separate_from_m3_and_transport_free() {
    let peer_production = PEER_SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("peer production source");
    let trust_production = TRUST_SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("trust production source");

    for forbidden in [
        "TcpListener",
        "TcpStream",
        "std::net",
        "server::",
        "chacha20poly1305",
        "device_permissions",
        "trusted_devices",
    ] {
        assert!(
            !peer_production.contains(forbidden),
            "peer identity core must stay transport/M3-authority free: {forbidden}"
        );
        assert!(
            !trust_production.contains(forbidden),
            "trusted peer store must stay transport/M3-authority free: {forbidden}"
        );
    }

    assert!(PEER_SOURCE.contains("DANGOMUSHI-PEER-IDENTITY-FINGERPRINT-V1"));
    assert!(DEVICE_PAIRING.contains("DANGOMUSHI-DEVICE-KEY-FINGERPRINT-V1"));
    assert!(PEER_SOURCE.contains("state"));
    assert!(TRUST_SOURCE.contains("trusted_peers.v1.json"));

    assert!(WORKSPACE_BACKUP.contains(r#"&["state", "autosave"]"#));
    assert!(!WORKSPACE_BACKUP.contains("peer_identity.v1"));
    assert!(!WORKSPACE_BACKUP.contains("trusted_peers.v1.json"));

    for forbidden in [
        "peer_private_key=",
        "peer_identity_seed=",
        "trusted_peer=",
        "peer_signature=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "generic session must not persist peer identity/trust payload: {forbidden}"
        );
    }

    assert!(SERVER.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(!SERVER.contains("0.0.0.0"));
    assert!(!SERVER.contains("peer_identity::"));
    assert!(!SERVER.contains("trusted_peers::"));
}

#[test]
fn roadmap_advances_exactly_to_encrypted_transfer() {
    for required in [
        "Trusted peer identity",
        "peer_identity.v1",
        "trusted_peers.v1.json",
        "not TOFU",
        "Generic Workspace Backup",
        "does not add a LAN listener",
    ] {
        assert!(
            ARCHITECTURE.contains(required) || DOC.contains(required),
            "missing trusted-peer architecture clause: {required}"
        );
    }

    for required in [
        "M4 PEER IDENTITY",
        "INITIALIZE PEER IDENTITY",
        "LOCAL IDENTITY READY / TRANSPORT LOCKED",
        "TRUSTED PEER(S)",
        "DIRECT PEER LAN READY / EXPLICIT ONE-SHOT",
    ] {
        assert!(UI.contains(required), "missing peer UI evidence: {required}");
    }

    assert!(ROADMAP.contains("- [x] **Trusted peer identity**"));
    assert!(ROADMAP.contains("- [x] **Encrypted text/file transfer**"));
}
