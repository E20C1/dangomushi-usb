const BUILD_SCRIPT: &str = include_str!("../scripts/build-release-bundle.py");
const VERIFY_SCRIPT: &str = include_str!("../scripts/verify-release-bundle.py");
const TERMINAL_FETCH: &str = include_str!("../scripts/fetch-windows-terminal-runtime.py");
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
        "app/Dangomushi",
        "START_DANGOMUSHI.cmd",
        "runtime/windows-terminal/",
        "RUNTIME-MANIFEST.json",
        "licenses/windows-terminal/LICENSE.txt",
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
        "dangomushi-cli",
        "dangomushi-desktop",
        "Dangomushi.app",
    ] {
        assert!(
            !BUILD_SCRIPT.contains(forbidden),
            "builder must not package durable/secret state: {forbidden}"
        );
    }
}

#[test]
#[test]
fn windows_launcher_normalizes_root_before_passing_starting_directory() {
    assert!(BUILD_SCRIPT.contains(r#"for %%I in (\"%~dp0.\") do set \"DANGOMUSHI_ROOT=%%~fI\""#));
    assert!(BUILD_SCRIPT.contains(r#"--startingDirectory \"%DANGOMUSHI_ROOT%\""#));
    assert!(!BUILD_SCRIPT.contains(r#"--startingDirectory \"%~dp0\""#));
}

fn pinned_windows_terminal_runtime_is_verified_before_packaging() {
    for required in [
        "1.25.3131.0",
        "273d6f1502eca1b5bd95c43f5d4d7ec72cc503df340102f8659717e5bead3437",
        "b7e1d2a414e6368affd098d91a4cba441dcacd3ab634a6f39c8e6f5490deef89",
        ".portable",
        "WindowsTerminal.exe",
        "wt.exe",
        "NOTICE.html",
        "unsafe Windows Terminal archive path",
        "Windows Terminal asset SHA-256 mismatch",
    ] {
        assert!(TERMINAL_FETCH.contains(required), "missing terminal runtime gate: {required}");
    }
    assert!(BUILD_SCRIPT.contains("validate_windows_terminal_runtime"));
    assert!(VERIFY_SCRIPT.contains("verify_windows_terminal_runtime"));
    assert!(BUILD_SCRIPT.contains("licenses/windows-terminal/LICENSE.txt"));
    assert!(VERIFY_SCRIPT.contains("licenses/windows-terminal/LICENSE.txt"));
    assert!(CI.contains("Fetch pinned Windows Terminal portable runtime"));
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
    assert!(ROADMAP.contains("- [x] **Verified snapshot staging materialization**"));
    assert!(ROADMAP.contains("- [x] **Frozen rebuild from staged source/vendor**"));
    assert!(ROADMAP.contains("- [x] **Recovered application smoke verification**"));
    assert!(ROADMAP.contains("- [x] **Workspace restore into a fresh destination**"));
    assert!(ROADMAP.contains(
        "- [x] **Reconstruction verification**"
    ));
}
