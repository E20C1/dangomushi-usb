const BUILD_SCRIPT: &str = include_str!("../scripts/build-release-bundle.py");
const VERIFY_SCRIPT: &str = include_str!("../scripts/verify-release-bundle.py");
const CI: &str = include_str!("../.github/workflows/ci.yml");
const WORKSPACE: &str = include_str!("../src/workspace.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/PORTABLE_RELEASE_BUNDLES.md");
const README: &str = include_str!("../README.md");

#[test]
fn builder_packages_only_reviewed_runtime_payloads() {
    for required in [
        "app/Dangomushi.exe",
        "app/dangomushi-cli.exe",
        "app/Dangomushi",
        "app/dangomushi-cli",
        "app/Dangomushi.app/Contents/MacOS/Dangomushi",
        "app/Dangomushi.app/Contents/Info.plist",
        "START_HERE.txt",
        "RELEASE-MANIFEST.json",
        "workspace-sibling-of-app",
    ] {
        assert!(BUILD_SCRIPT.contains(required), "missing bundle layout evidence: {required}");
    }

    for forbidden in [
        "workspace/data",
        "workspace/state",
        "recovery/workspace.dgmwbk",
        "Secret Backup",
    ] {
        assert!(
            !BUILD_SCRIPT.contains(forbidden),
            "builder must not package durable/secret state: {forbidden}"
        );
    }
}

#[test]
fn release_manifest_and_archive_verification_fail_closed() {
    for required in [
        "dangomushi-portable-release",
        "MAX_ARCHIVE_FILES",
        "MAX_MANIFEST_BYTES",
        "MAX_PAYLOAD_BYTES",
        "duplicate archive path",
        "forbidden release payload path",
        "release archive checksum mismatch",
        "release payload integrity mismatch",
        "release executable permission missing",
        "only regular files are permitted in release archives",
    ] {
        assert!(VERIFY_SCRIPT.contains(required), "missing verifier boundary: {required}");
    }

    for forbidden_prefix in [
        "workspace/",
        "vendor/",
        "recovery/",
        ".git/",
        "target/",
        "build/",
    ] {
        assert!(VERIFY_SCRIPT.contains(forbidden_prefix));
    }
}

#[test]
fn ci_builds_and_verifies_every_os_bundle_before_main_upload() {
    for required in [
        "Build portable release bundle",
        "python scripts/build-release-bundle.py",
        "Verify portable release bundle",
        "python scripts/verify-release-bundle.py --directory dist",
        "runner.os == 'Windows'",
        "runner.os != 'Windows'",
        "github.ref == 'refs/heads/main'",
        "actions/upload-artifact@v7",
        "archive: false",
        "dist/*.sha256",
    ] {
        assert!(CI.contains(required), "CI missing release boundary: {required}");
    }
}

#[test]
fn macos_app_layout_preserves_the_same_portable_workspace_root() {
    for required in [
        "portable_workspace_root_from_executable",
        "MacOS",
        "Contents",
        ".app",
        "app_dir",
        "portable_root.join(\"workspace\")",
        "macos_app_bundle_maps_to_same_portable_workspace",
    ] {
        assert!(WORKSPACE.contains(required), "missing macOS portable path evidence: {required}");
    }
}

#[test]
fn docs_are_explicit_about_unsigned_and_non_hermetic_release_limits() {
    for required in [
        "does not require Rust, Cargo",
        "does not prove publisher identity",
        "not Authenticode-signed",
        "not code-signed or notarized",
        "does not claim bit-for-bit reproducible compiled binaries",
        "does not automatically create a GitHub Release",
    ] {
        assert!(
            DOC.contains(required) || ARCHITECTURE.contains(required),
            "missing portable release limitation: {required}"
        );
    }

    assert!(README.contains("portable per-OS release bundles"));
}

#[test]
fn m5_closes_and_m6_starts_only_after_an_explicit_boundary() {
    assert!(ROADMAP.contains("- [x] **Portable release bundles per operating system**"));
    assert!(ROADMAP.contains("M5 Offline continuity is complete"));
    assert!(ROADMAP.contains("## M6 — Offline reconstruction"));
    assert!(ROADMAP.contains(
        "- [ ] **Verified snapshot staging materialization** — Exactly One Next Task"
    ));
}
