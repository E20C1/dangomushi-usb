const CORE: &str = include_str!("../src/image_optimizer/mod.rs");
const WORKER: &str = include_str!("../src/image_optimizer/worker.rs");
const TUI: &str = include_str!("../src/tui.rs");
const TOOLS: &str = include_str!("../src/tool_registry.rs");
const CARGO: &str = include_str!("../Cargo.toml");

#[test]
fn image_optimizer_is_registered_as_a_ready_tui_tool() {
    for required in [
        "ToolId::ImageOptimizer",
        r#"key: "image-optimizer""#,
        r#"label: "IMAGE OPTIMIZER""#,
        "ToolStatus::Ready",
    ] {
        assert!(TOOLS.contains(required), "missing tool registry contract: {required}");
    }

    assert!(TUI.contains("View::ImageOptimizer"));
    assert!(TUI.contains("ControlId::FileImageOptimize"));
    assert!(TUI.contains("OPTIMIZE IMAGE"));
    assert!(TUI.contains("self.image_preview.render_pair"));
}

#[test]
fn codecs_are_native_rust_dependencies_without_web_runtime() {
    for required in [
        r#"oxipng = { version = "10.2.1""#,
        r#"jpeg-encoder = { version = "0.7.1""#,
        r#"webp-rust = "0.3.2""#,
    ] {
        assert!(CARGO.contains(required), "missing codec dependency: {required}");
    }

    for forbidden in ["node", "wasm-bindgen", "webview", "electron"] {
        assert!(
            !CORE.to_ascii_lowercase().contains(forbidden),
            "optimizer core must not depend on a web runtime: {forbidden}"
        );
    }
}

#[test]
fn preview_and_save_share_one_candidate_artifact() {
    assert!(CORE.contains("pub struct CandidateArtifact"));
    assert!(CORE.contains("pub output_bytes: Vec<u8>"));
    assert!(CORE.contains("export_candidate"));
    assert!(CORE.contains("file.write_all(&candidate.output_bytes)"));
    assert!(CORE.contains("SourceChanged"));

    assert!(TUI.contains("candidate: Option<CandidateArtifact>"));
    assert!(TUI.contains("image_optimizer::export_candidate(&self.workspace_root, candidate)"));
    assert!(!TUI.contains("optimize_workspace_image(&self.workspace_root"));
}

#[test]
fn optimizer_is_latest_generation_wins_and_off_the_tui_thread() {
    assert!(WORKER.contains("thread::spawn"));
    assert!(WORKER.contains("while let Ok(next) = requests.try_recv()"));
    assert!(TUI.contains(
        "if completion.generation != self.image_optimizer.generation"
    ));
    assert!(TUI.contains("self.image_optimizer_worker.submit(job)"));
}

#[test]
fn optimizer_fails_closed_on_destructive_or_unbounded_inputs() {
    for required in [
        "MAX_SOURCE_BYTES",
        "MAX_OUTPUT_BYTES",
        "MAX_PIXELS",
        "AnimatedUnsupported",
        "AlphaToJpeg",
        "InvalidPath",
        "create_new(true)",
        "detect_animation",
        "path resolves",
    ] {
        assert!(CORE.contains(required), "missing optimizer safety boundary: {required}");
    }

    assert!(!CORE.contains("truncate(true)"));
    assert!(!CORE.contains("write(root.join(&candidate.source_relative"));
}

#[test]
fn privacy_metadata_and_color_profile_are_explicit() {
    assert!(CORE.contains("icc_profile"));
    assert!(CORE.contains("exif_metadata"));
    assert!(CORE.contains("xmp_metadata"));
    assert!(CORE.contains("private_metadata_stripped"));
    assert!(TUI.contains("COLOR PROFILE WILL NOT BE PRESERVED"));
    assert!(TUI.contains("EXIF/XMP private metadata stripped"));
}
