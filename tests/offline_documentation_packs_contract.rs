use dangomushi::offline_docs::{self, OfflineDocsError, OFFLINE_DOC_MARKER};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SOURCE: &str = include_str!("../src/offline_docs.rs");
const MAIN: &str = include_str!("../src/main.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/OFFLINE_DOCUMENTATION_PACKS.md");

fn temporary_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dangomushi-offline-doc-contract-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn repository_markdown_docs() -> BTreeSet<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs_root = root.join("docs");
    let mut paths = BTreeSet::new();
    paths.insert("README.md".to_string());

    for entry in fs::read_dir(docs_root).expect("read docs") {
        let entry = entry.expect("docs entry");
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("UTF-8 docs filename");
        paths.insert(format!("docs/{name}"));
    }

    paths
}

#[test]
fn embedded_pack_contains_readme_and_every_repository_markdown_doc() {
    let expected = repository_markdown_docs();
    let actual: BTreeSet<String> = offline_docs::documents()
        .iter()
        .map(|document| document.path.to_string())
        .collect();

    assert_eq!(actual, expected);
    assert!(actual.contains("docs/OFFLINE_DOCUMENTATION_PACKS.md"));

    for document in offline_docs::documents() {
        assert!(!document.content.is_empty(), "empty document: {}", document.path);
    }
}

#[test]
fn summary_is_deterministic_and_exact_document_lookup_is_offline() {
    let first = offline_docs::summary().expect("summary");
    let second = offline_docs::summary().expect("repeat summary");

    assert_eq!(first, second);
    assert_eq!(first.pack_id, "dangomushi-core-docs");
    assert_eq!(first.format_version, 1);
    assert_eq!(first.sha256_hex().len(), 64);
    assert!(first.document_count >= 30);
    assert!(first.total_bytes > 100_000);

    let architecture = offline_docs::document("docs/ARCHITECTURE.md").expect("architecture");
    assert!(architecture.content.contains("# DANGOMUSHI Architecture"));
    assert!(matches!(
        offline_docs::document("../README.md"),
        Err(OfflineDocsError::UnknownDocument)
    ));
}

#[test]
fn export_is_create_new_exact_and_detects_modification_and_extra_files() {
    let destination = temporary_path("export");
    let exported = offline_docs::export(&destination).expect("export");
    let verified = offline_docs::verify_export(&destination).expect("verify");
    assert_eq!(exported, verified);
    assert!(destination.join(OFFLINE_DOC_MARKER).is_file());
    assert!(destination.join("README.md").is_file());
    assert!(destination.join("docs/ARCHITECTURE.md").is_file());

    assert!(matches!(
        offline_docs::export(&destination),
        Err(OfflineDocsError::DestinationExists)
    ));

    fs::write(destination.join("docs/ARCHITECTURE.md"), "tampered").expect("tamper");
    assert!(matches!(
        offline_docs::verify_export(&destination),
        Err(OfflineDocsError::InvalidExport(_))
    ));

    fs::remove_dir_all(&destination).expect("cleanup");
    offline_docs::export(&destination).expect("re-export");
    fs::write(destination.join("unexpected.txt"), "unexpected").expect("extra");
    assert!(matches!(
        offline_docs::verify_export(&destination),
        Err(OfflineDocsError::InvalidExport(_))
    ));

    fs::remove_dir_all(destination).expect("cleanup");
}

#[test]
fn offline_pack_has_no_network_process_or_workspace_persistence_authority() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    for forbidden in [
        "TcpListener",
        "TcpStream",
        "UdpSocket",
        "std::net",
        "reqwest",
        "hyper::",
        "Command::new",
        "std::process",
        "workspace::",
        "session_state",
        "workspace_backup",
        "secret_backup",
    ] {
        assert!(
            !production.contains(forbidden),
            "offline docs must not gain network/process/workspace authority: {forbidden}"
        );
    }

    for forbidden in [
        "offline_doc_index=",
        "offline_doc_content=",
        "offline_doc_pack=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "offline viewer state/content must not enter session persistence: {forbidden}"
        );
    }
}

#[test]
fn cli_and_native_system_view_expose_embedded_pack_without_remote_fetch() {
    for required in [
        r#"Some("docs") => run_docs(args.collect())"#,
        r#""list""#,
        r#""show""#,
        r#""export""#,
        r#""verify""#,
        "dangomushi docs list|show|export|verify",
    ] {
        assert!(MAIN.contains(required), "missing CLI evidence: {required}");
    }

    for required in [
        "OFFLINE DOCUMENTATION",
        "EMBEDDED / VERIFIED INVENTORY",
        "offline_docs::documents()",
        "offline-doc-selector",
        "Read-only content is compiled into this executable",
    ] {
        assert!(UI.contains(required), "missing native viewer evidence: {required}");
    }
}

#[test]
fn docs_and_roadmap_define_narrow_m5_boundary_and_advance_once() {
    for required in [
        "compile-time embedded",
        "create-new only",
        "does not automatically copy",
        "does not vendor Rust dependencies",
        "not a publisher signature",
    ] {
        assert!(
            DOC.contains(required) || ARCHITECTURE.contains(required),
            "missing offline-doc boundary: {required}"
        );
    }

    assert!(ROADMAP.contains("- [x] **Offline documentation packs**"));
    assert!(ROADMAP.contains("- [x] **Reproducible dependency cache/vendor strategy**"));
    assert!(ROADMAP.contains(
        "- [ ] **Recovery snapshots** — Exactly One Next Task"
    ));
}
