# DANGOMUSHI

DANGOMUSHI is a portable, local-first workspace intended to keep essential tools and data usable without depending on a cloud account or public internet service.

Repository: `dangomushi-usb`

## Current milestone: v0.1 foundation

The first implementation deliberately starts small and security-first:

- Rust core with **zero third-party runtime dependencies**.
- Cross-platform CLI commands for workspace initialization and health checks.
- Embedded local control panel served from the Rust binary.
- The web gateway binds to **`127.0.0.1` only**.
- No public bind, LAN bind, UPnP, NAT-PMP, reverse tunnel, CDN or external web request.
- Workspace data lives outside the application build output.
- Windows double-click build/update entry point is included.

The native desktop shell, editor, file manager and authenticated mobile/LAN access are later milestones. They are not faked in the v0.1 implementation.

## Quick start

Requires a Rust toolchain for development builds.

```bash
cargo run -- init
cargo run -- doctor
cargo run -- serve
```

Then open:

```text
http://127.0.0.1:45821
```

A different local port or workspace can be selected explicitly:

```bash
cargo run -- serve --port 49152 --workspace D:\DANGOMUSHI\workspace
```

## Windows build/update

Close any running DANGOMUSHI executable, then double-click:

```text
UPDATE_TOOLBOX.cmd
```

The script runs tests, builds a release binary, stages it, keeps one previous application binary as a rollback copy, and installs the new binary into `app/`.

It does **not** delete or replace `workspace/`.

## Repository layout

```text
dangomushi-usb/
├─ Cargo.toml
├─ UPDATE_TOOLBOX.cmd
├─ src/                 Rust core
├─ web/                 embedded local/mobile UI
├─ docs/                architecture and roadmap
├─ scripts/             platform build helpers
├─ app/                 generated runnable application
└─ workspace/           local user data; protected from build/update
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the security and data boundaries.

## Security boundary in v0.1

The local HTTP gateway is a presentation surface only. It exposes fixed read-only routes and a read-only status endpoint. It does not currently expose filesystem writes, command execution or Git operations.

Mobile/LAN access will not be enabled until trusted-device pairing and authorization exist.

## License

No project license has been selected yet.
