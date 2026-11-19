use std::path::Path;

const TUI: &str = include_str!("../src/tui.rs");
const IMAGE: &str = include_str!("../src/tui_image.rs");
const MAIN: &str = include_str!("../src/main.rs");
const CARGO: &str = include_str!("../Cargo.toml");

#[test]
fn tui_is_the_only_interactive_native_ui() {
    assert!(MAIN.contains("None | Some(\"tui\") => tui::run()"));
    assert!(CARGO.contains(r#"ratatui = { version = "0.30.2""#));
    assert!(CARGO.contains(r#"crossterm = "0.29.0""#));
    assert!(!CARGO.contains("eframe"));
    assert!(!CARGO.contains(r#"desktop = ["#));

    assert!(!Path::new("src/native_ui.rs").exists());
    assert!(!Path::new("src/native_ui_theme.rs").exists());
    assert!(!Path::new("src/bin/dangomushi-desktop.rs").exists());
}

#[test]
fn keyboard_enter_and_mouse_click_share_the_same_activation_path() {
    let enter = TUI
        .find("KeyCode::Enter => self.activate_focused()")
        .expect("Enter must activate the focused control");
    let mouse = TUI
        .find("self.focus_index = hit.control_index;")
        .expect("mouse click must select the hit control");
    let mouse_activate = TUI[mouse..]
        .find("self.activate_focused();")
        .map(|offset| mouse + offset)
        .expect("mouse click must use the same focused activation path");

    assert!(enter < mouse_activate);
    assert!(TUI.contains("ControlId::Nav"));
    assert!(TUI.contains("fn activate(&mut self, id: ControlId)"));
}

#[test]
fn terminal_state_is_guarded_and_restored() {
    for required in [
        "enable_raw_mode()",
        "EnterAlternateScreen",
        "EnableMouseCapture",
        "DisableMouseCapture",
        "LeaveAlternateScreen",
        "disable_raw_mode()",
        "impl Drop for TerminalGuard",
    ] {
        assert!(TUI.contains(required), "missing terminal guard step: {required}");
    }

    let alternate = TUI
        .find("execute!(out, EnterAlternateScreen)")
        .expect("alternate screen entry");
    let picker = TUI
        .find("ImagePreviewRuntime::detect()")
        .expect("image capability query");
    let mouse = TUI
        .find("execute!(out, EnableMouseCapture)")
        .expect("mouse capture");
    let event_loop = TUI
        .find("event::read()")
        .expect("event loop");

    assert!(alternate < picker);
    assert!(picker < mouse);
    assert!(mouse < event_loop);
}

#[test]
fn before_after_preview_is_protocol_detected_and_nonblocking() {
    assert!(CARGO.contains(
        r#"ratatui-image = { version = "11.1.0", default-features = false, features = ["crossterm"] }"#
    ));
    assert!(CARGO.contains(
        r#"image = { version = "0.25.10", default-features = false, features = ["png", "jpeg", "webp"] }"#
    ));

    for required in [
        "Picker::from_query_stdio()",
        "Picker::halfblocks()",
        "ProtocolType::Sixel",
        "ProtocolType::Kitty",
        "ProtocolType::Iterm2",
        "ProtocolType::Halfblocks",
        "ThreadProtocol",
        "ResizeRequest",
        "request.resize_encode()",
        "StatefulImage::new().resize(Resize::Fit(None))",
        "\" BEFORE \"",
        "\" AFTER \"",
        "area.width >= 80",
    ] {
        assert!(IMAGE.contains(required), "missing image preview contract: {required}");
    }

    assert!(TUI.contains("Image preview: {}{}"));
    assert!(TUI.contains("APPROXIMATE FALLBACK"));
}

#[test]
fn password_clipboard_hygiene_survives_gui_removal() {
    assert!(TUI.contains("SecretClipboard<SystemClipboard>"));
    assert!(TUI.contains("ClipboardSecret::from_generated_password"));
    assert!(TUI.contains("ClipboardPolicy::default()"));
    assert!(TUI.contains("ClipboardPoll::PreservedExternalContent"));
    assert!(TUI.contains("COPY / AUTO-CLEAR 30s"));
}
