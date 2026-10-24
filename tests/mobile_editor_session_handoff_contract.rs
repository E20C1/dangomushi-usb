use dangomushi::device_pairing::{
    pairing_proof, pairing_signature_message, pairing_transcript, DevicePairing,
    PairedDeviceCandidate, PairingSubmission,
};
use dangomushi::device_permissions::{self, PermissionScope};
use dangomushi::device_reconnection::{
    self, AuthenticatedDeviceSession, ReconnectProof, ReconnectRandomSource,
    ReconnectRandomSourceError, RECONNECT_CHALLENGE_BYTES,
};
use dangomushi::lan_access_control::LanEnableControl;
use dangomushi::mobile_handoff::{self, MobileHandoffError};
use dangomushi::session_state::{SessionState, SessionView};
use dangomushi::text_editor;
use dangomushi::trusted_devices;
use dangomushi::workspace;
use ed25519_dalek::{Signer, SigningKey};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SOURCE: &str = include_str!("../src/mobile_handoff.rs");
const SERVER: &str = include_str!("../src/server.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/MOBILE_EDITOR_SESSION_HANDOFF.md");

struct FixedRandom([u8; RECONNECT_CHALLENGE_BYTES]);

impl ReconnectRandomSource for FixedRandom {
    fn fill_bytes(
        &mut self,
        destination: &mut [u8],
    ) -> Result<(), ReconnectRandomSourceError> {
        destination.copy_from_slice(&self.0);
        Ok(())
    }
}

fn temporary_workspace(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dangomushi-mobile-handoff-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn approved_candidate(seed: u8, name: &str) -> (PairedDeviceCandidate, SigningKey) {
    let mut lan = LanEnableControl::default();
    lan.enable_for_this_run();
    let now = Instant::now();

    let mut pairing = DevicePairing::default();
    let invitation = pairing.begin(&lan, now).expect("begin pairing");
    let signing_key = SigningKey::from_bytes(&[seed; 32]);
    let public_key = signing_key.verifying_key().to_bytes();

    let transcript =
        pairing_transcript(invitation.challenge(), name, &public_key).expect("transcript");
    let proof = pairing_proof(
        invitation.code().expose_secret(),
        invitation.challenge(),
        name,
        &public_key,
    )
    .expect("proof");
    let signature = signing_key
        .sign(&pairing_signature_message(&transcript, &proof))
        .to_bytes();

    pairing
        .submit(
            &lan,
            now + Duration::from_secs(1),
            PairingSubmission {
                device_name: name.to_string(),
                public_key,
                code_proof: proof,
                signature,
            },
        )
        .expect("submit");

    (
        pairing
            .approve(&lan, now + Duration::from_secs(2))
            .expect("approve"),
        signing_key,
    )
}

fn reconnect_session(
    root: &Path,
    lan: &LanEnableControl,
    fingerprint: &[u8; 32],
    signing_key: &SigningKey,
) -> AuthenticatedDeviceSession {
    let now = Instant::now();
    let mut random = FixedRandom([91u8; RECONNECT_CHALLENGE_BYTES]);
    let challenge =
        device_reconnection::begin_with_source(root, lan, fingerprint, now, &mut random)
            .expect("challenge");
    let signature = signing_key
        .sign(&device_reconnection::reconnect_signature_message(
            challenge.challenge(),
            challenge.fingerprint(),
        ))
        .to_bytes();

    device_reconnection::complete(
        root,
        lan,
        now + Duration::from_secs(1),
        challenge,
        ReconnectProof {
            fingerprint: *fingerprint,
            signature,
        },
    )
    .expect("reconnect")
}

fn trusted_session(label: &str) -> (PathBuf, LanEnableControl, AuthenticatedDeviceSession, [u8; 32]) {
    let root = temporary_workspace(label);
    workspace::initialize(&root).expect("workspace");

    let (candidate, signing_key) = approved_candidate(101, "Phone");
    let trusted = trusted_devices::register_candidate(&root, &candidate).expect("trust");
    let fingerprint = *trusted.fingerprint();

    let mut lan = LanEnableControl::default();
    lan.enable_for_this_run();
    let session = reconnect_session(&root, &lan, &fingerprint, &signing_key);
    (root, lan, session, fingerprint)
}

#[test]
fn session_handoff_and_editor_operations_use_independent_current_scopes() {
    let (root, lan, session, fingerprint) = trusted_session("scopes");
    fs::write(root.join("data/documents/note.txt"), "before").expect("seed");

    device_permissions::set_scopes(&root, &fingerprint, &[PermissionScope::SessionHandoff])
        .expect("handoff grant");

    let current = SessionState {
        view: SessionView::Editor,
        browser_dir: PathBuf::from("data/documents"),
        browser_selected: Some(PathBuf::from("data/documents/note.txt")),
        editor_path: Some(PathBuf::from("data/documents/note.txt")),
    };

    let snapshot =
        mobile_handoff::capture_session(&root, &lan, &session, &current).expect("capture");
    assert_eq!(snapshot.view(), SessionView::Editor);
    assert_eq!(snapshot.editor_path(), Some(Path::new("data/documents/note.txt")));

    assert_eq!(
        mobile_handoff::open_editor_document(
            &root,
            &lan,
            &session,
            Path::new("data/documents/note.txt")
        ),
        Err(MobileHandoffError::PermissionDenied(PermissionScope::FilesRead))
    );

    device_permissions::set_scopes(
        &root,
        &fingerprint,
        &[PermissionScope::SessionHandoff, PermissionScope::FilesRead],
    )
    .expect("read grant");
    let opened = mobile_handoff::open_editor_document(
        &root,
        &lan,
        &session,
        Path::new("data/documents/note.txt"),
    )
    .expect("open");
    assert_eq!(opened.text(), "before");

    assert_eq!(
        mobile_handoff::save_editor_document(
            &root,
            &lan,
            &session,
            opened.path(),
            opened.revision(),
            "after"
        ),
        Err(MobileHandoffError::PermissionDenied(PermissionScope::FilesWrite))
    );

    device_permissions::set_scopes(
        &root,
        &fingerprint,
        &[PermissionScope::SessionHandoff, PermissionScope::FilesWrite],
    )
    .expect("write grant");
    let new_revision = mobile_handoff::save_editor_document(
        &root,
        &lan,
        &session,
        opened.path(),
        opened.revision(),
        "after",
    )
    .expect("save");

    assert_eq!(
        text_editor::read_text(&root, Path::new("data/documents/note.txt")).expect("read"),
        "after"
    );
    assert_ne!(&new_revision, opened.revision());

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn stale_revision_refuses_overwrite_and_preserves_mobile_text_as_recovery() {
    let (root, lan, session, fingerprint) = trusted_session("conflict");
    let path = Path::new("data/documents/note.txt");
    fs::write(root.join(path), "initial").expect("seed");

    device_permissions::set_scopes(
        &root,
        &fingerprint,
        &[
            PermissionScope::SessionHandoff,
            PermissionScope::FilesRead,
            PermissionScope::FilesWrite,
        ],
    )
    .expect("grant");

    let opened =
        mobile_handoff::open_editor_document(&root, &lan, &session, path).expect("open");
    fs::write(root.join(path), "external change").expect("external change");

    assert_eq!(
        mobile_handoff::save_editor_document(
            &root,
            &lan,
            &session,
            path,
            opened.revision(),
            "mobile draft"
        ),
        Err(MobileHandoffError::RevisionConflict)
    );
    assert_eq!(
        text_editor::read_text(&root, path).expect("disk"),
        "external change"
    );
    let recovery = text_editor::latest_recovery(&root, path)
        .expect("recovery lookup")
        .expect("recovery exists");
    assert_eq!(recovery.text, "mobile draft");

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn handoff_core_is_transport_free_and_does_not_persist_mobile_payloads() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    for forbidden in [
        "TcpListener",
        "TcpStream",
        "std::net",
        "server::",
        "session_state::save",
        "OpenOptions",
        "serde_json",
    ] {
        assert!(
            !production.contains(forbidden),
            "handoff core must not create transport or a second persistence path: {forbidden}"
        );
    }

    for forbidden in [
        "mobile_text=",
        "mobile_revision=",
        "authenticated_device=",
        "handoff_payload=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "generic session must not persist mobile handoff payload: {forbidden}"
        );
    }

    assert!(SERVER.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(!SERVER.contains("0.0.0.0"));
    assert!(!SERVER.contains("mobile_handoff::"));
}

#[test]
fn roadmap_closes_m3_and_advances_exactly_to_trusted_peer_identity() {
    for required in [
        "Mobile editor/session handoff",
        "session.handoff",
        "files.read",
        "files.write",
        "revision",
        "recovery",
        "transport-independent",
        "does not create a LAN listener",
    ] {
        assert!(
            ARCHITECTURE.contains(required) || DOC.contains(required),
            "missing mobile handoff architecture clause: {required}"
        );
    }

    assert!(ROADMAP.contains("- [x] **Mobile editor/session handoff**"));
    assert!(ROADMAP.contains(
        "- [x] **Trusted peer identity**"
    ));
    assert!(ROADMAP.contains(
        "No LAN listener should be merged before these security boundaries are implemented and tested."
    ));
}
