const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/OFFLINE_RECONSTRUCTION.md");
const README: &str = include_str!("../README.md");
const MAIN: &str = include_str!("../src/main.rs");
const LIB: &str = include_str!("../src/lib.rs");
const RECOVERY: &str = include_str!("../src/recovery_snapshot.rs");

#[test]
fn m6_is_explicitly_authorized_after_m5_completion() {
    assert!(ROADMAP.contains("M5 Offline continuity is complete"));
    assert!(ROADMAP.contains("## M6 — Offline reconstruction"));
    assert!(ROADMAP.contains("- [x] **Verified snapshot staging materialization**"));
    assert!(ROADMAP.contains(
        "- [ ] **Frozen rebuild from staged source/vendor** — Exactly One Next Task"
    ));
    assert!(README.contains("## Current milestone: M6 offline reconstruction"));
}

#[test]
fn first_m6_task_is_staging_only_and_does_not_pull_later_authority_forward() {
    for required in [
        "Before any destination mutation",
        "must pass the existing read-only verify_snapshot boundary",
        "not already exist",
        "outside any configured live Workspace",
        "Cargo execution",
        "Secret Backup / Restore is intentionally not folded into M6 reconstruction",
    ] {
        assert!(
            DOC.contains(required) || ARCHITECTURE.contains(required),
            "missing M6 staging boundary: {required}"
        );
    }

    for later in [
        "- [ ] Frozen rebuild from staged source/vendor",
        "- [ ] Recovered application smoke verification",
        "- [ ] Workspace restore into a fresh destination",
        "- [ ] Reconstruction verification",
    ] {
        assert!(ROADMAP.contains(later), "missing later M6 task: {later}");
    }

    assert!(ROADMAP.contains(
        "- [ ] **Frozen rebuild from staged source/vendor** — Exactly One Next Task"
    ));
    assert!(!ROADMAP.contains(
        "- [ ] **Recovered application smoke verification** — Exactly One Next Task"
    ));
}

#[test]
fn staging_runtime_authority_is_present_but_later_reconstruction_authority_is_not() {
    assert!(MAIN.contains(r#""stage" =>"#));
    assert!(MAIN.contains("recovery snapshot stage --snapshot <PATH> --destination <PATH>"));
    assert!(RECOVERY.contains("pub fn stage_verified_snapshot"));
    assert!(RECOVERY.contains("verify_snapshot(snapshot_root)?"));
    assert!(RECOVERY.contains("verify_snapshot(&destination_path)?"));

    assert!(!LIB.contains("pub mod offline_reconstruction"));
    for forbidden in [
        r#""rebuild" =>"#,
        "Command::new",
        "workspace_backup::restore_backup",
        "secret_backup::restore",
    ] {
        assert!(
            !MAIN.contains(forbidden) && !RECOVERY.contains(forbidden),
            "later reconstruction authority appeared early: {forbidden}"
        );
    }
}

#[test]
fn unrelated_post_m5_security_and_network_work_remains_separate() {
    for required in [
        "Authenticode signing",
        "Apple code-signing or notarization",
        "automatic application updates",
        "mobile editor/session LAN transport",
        "Internet P2P",
    ] {
        assert!(DOC.contains(required), "missing separated future boundary: {required}");
    }

    assert!(DOC.contains("Internet P2P remains disabled"));
}
