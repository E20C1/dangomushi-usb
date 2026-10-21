# Native UI Layer Decision

Date: 2026-10-21

Status: adopted for M1

## Candidates

The M1 desktop shell considered eframe/egui and Slint.

### eframe / egui

- Rust-native application code without a separate UI language.
- Immediate-mode UI is a good fit for DANGOMUSHI's compact tool panels, custom pixel-oriented chrome and rapidly changing workspace state.
- The upstream project is available under MIT OR Apache-2.0.
- eframe 0.37.0 supports native desktop applications and requires Rust 1.95.
- The dependency can be feature-gated so the headless CLI and loopback server remain buildable without the desktop stack.

### Slint

- Strong declarative UI model and desktop/mobile support.
- Introduces a separate .slint UI language and build integration.
- Its licensing is suitable for multiple use cases, but is more nuanced than the permissive MIT/Apache model used by egui.
- Its mobile support is not a deciding advantage because DANGOMUSHI's planned phone/tablet client remains a local web surface served by the Rust core.

## Decision

Adopt **eframe / egui** for the native desktop shell.

DANGOMUSHI does not adopt eframe as the authority for files, identity, backups or permissions. It is a presentation layer over the Rust core.

The desktop dependency remains behind the Cargo feature named desktop. The CLI, workspace model and loopback gateway continue to compile without that feature.

The initial renderer is eframe's glow path with bundled default fonts and Linux X11/Wayland support. Features should be revisited once executable size and real USB launch behavior are measured on target machines.

## Revisit conditions

Re-evaluate this decision if one of these becomes true:

- the immediate-mode model materially blocks editor or accessibility work,
- portable executable size becomes unacceptable after release optimization,
- Linux support becomes unreliable,
- mobile requirements change from web-client access to a shared native UI,
- a licensing or maintenance change affects the chosen stack.
