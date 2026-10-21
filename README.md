# DANGOMUSHI

DANGOMUSHI is a portable, local-first workspace intended to keep essential tools and data usable without depending on a cloud account or public internet service.

Repository: dangomushi-usb

## Current milestone: M1 native desktop shell

The current implementation includes:

- Rust core with a standard-library-only headless path.
- Cross-platform CLI commands for workspace initialization and health checks.
- A native desktop shell using feature-gated eframe/egui.
- A portable workspace path that remains separate from replaceable application binaries.
- Embedded loopback-only status UI served from the Rust core.
- No public bind, LAN bind, UPnP, NAT-PMP, reverse tunnel, CDN or external web request.
- Windows double-click build/update entry point.
- CI builds for Windows, macOS and Linux.

The file browser, editor and authenticated mobile/LAN mode are later M1/M3 work. They are not faked as completed features.

## Windows quick start

With a Rust toolchain installed, double-click:

~~~text
UPDATE_TOOLBOX.cmd
~~~

After a successful build:

~~~text
app\Dangomushi.exe
~~~

is the native desktop application, and:

~~~text
app\dangomushi-cli.exe
~~~

is the headless CLI.

The update script stages the new binaries, keeps one rollback copy, and does not delete or replace workspace/.

## CLI

~~~bash
cargo run -- init
cargo run -- doctor
cargo run -- serve
~~~

The local status gateway is then available at:

~~~text
http://127.0.0.1:45821
~~~

A different loopback port or workspace can be selected explicitly:

~~~bash
cargo run -- serve --port 49152 --workspace D:\DANGOMUSHI\workspace
~~~

## Desktop development

~~~bash
cargo run --features desktop --bin dangomushi-desktop
~~~

The desktop stack is optional. Core tests and CLI builds do not require the desktop feature.

## Repository layout

~~~text
dangomushi-usb/
├─ Cargo.toml
├─ UPDATE_TOOLBOX.cmd
├─ src/
│  ├─ main.rs
│  ├─ lib.rs
│  ├─ workspace.rs
│  ├─ server.rs
│  ├─ native_ui.rs
│  └─ bin/dangomushi-desktop.rs
├─ web/                 embedded loopback/mobile UI
├─ docs/                architecture, decisions and roadmap
├─ scripts/             platform build helpers
├─ app/                 generated runnable application
└─ workspace/           durable local user data
~~~

See docs/ARCHITECTURE.md for the authority, network and data boundaries.

## Security boundary

The local HTTP gateway is a presentation surface only. It exposes fixed read-only routes and a read-only status endpoint. It does not expose filesystem writes, command execution or Git operations.

Mobile/LAN access will not be enabled until trusted-device pairing and authorization exist.

## License

No project license has been selected yet.
