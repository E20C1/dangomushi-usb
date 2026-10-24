# DANGOMUSHI

DANGOMUSHI is a portable, local-first workspace intended to keep essential tools and data usable without depending on a cloud account or public internet service.

Repository: dangomushi-usb

## Current milestone: M5 offline continuity

The current implementation includes:

- Rust core with a small dependency surface; the Password Generator uses the OS random source through `getrandom`.
- Cross-platform CLI commands for workspace initialization and health checks.
- A native desktop shell using feature-gated eframe/egui.
- A read-only native file browser constrained to the configured workspace root.
- A workspace-rooted UTF-8 text editor with explicit save, dirty tracking and recovery autosave.
- A central built-in tool registry and native Tool Launcher.
- A READY native HASH / VERIFY tool that streams existing Workspace-rooted regular files through the SHA-256 core, accepts the current FILES selection or a relative target path, and reports calculated / MATCH / MISMATCH state without modifying the target.
- A READY native TEXT / JSON utility for LF normalization, trailing-space cleanup, JSON validation, and whitespace-only pretty/compact formatting with no automatic source-file writes.
- A READY native COMPARE tool for binary-safe streaming comparison of two Workspace files, with first-difference byte reporting and optional UTF-8 line detail.
- A READY native SEARCH tool for bounded literal UTF-8 search under `workspace/data/documents`, with line/column previews and direct OPEN into the existing Editor boundary.
- A READY native REPOSITORIES tool for bounded read-only Git status, recent log, and worktree/staged diff-stat inspection under `workspace/repositories`, with no shell, network, or mutating Git commands.
- A READY native BACKUP / RESTORE tool for verified generic Workspace snapshots under `workspace/backups`, with per-file/archive SHA-256, hidden staging, in-process rollback, explicit RESTORE confirmation, and no Vault-secret export path.
- Restores the previous native view, FILES location/selection and open editor file from workspace-local session state.
- A reusable square-edged pixel UI system with original 5x5 vector-drawn icons and no external icon/font assets.
- An M2-S Password Generator Core with OS CSPRNG input, configurable character groups, rejection-sampled selection, deterministic injected-source tests, and no UI/Vault persistence.
- An M2-S in-memory Credential model with redacted secret-aware ownership and no persistence/UI integration.
- An M2-S existing-Credential input boundary that consumes owned input into the same Credential model without persistence or UI integration.
- An M2-S encrypted Vault storage core using XChaCha20Poly1305 AEAD, authenticated metadata, verified durable replacement, and interrupted-write recovery.
- An M2-S Vault unlock/lock core using Argon2id-derived Vault keys, redacted Master Passphrase ownership, and an explicit unlocked-key lifetime.
- An M2-S generated-Credential integration boundary that consumes GeneratedPassword into the same source-agnostic Credential model without a required secret clone.
- An M2-S Recovery Code core with redacted/zeroized secret ownership and a bounded versioned Vault section payload.
- An M2-S Clipboard hygiene boundary with explicit secret-copy ownership, configurable delayed clearing, compare-before-clear behavior, and a feature-gated desktop OS clipboard adapter.
- An M2-S Secret Backup / Restore core that packages authenticated Vault ciphertext without decrypting it into generic backup/export flows and restores only after passphrase authentication.
- A portable workspace path that remains separate from replaceable application binaries.
- Embedded loopback-only status UI served from the Rust core.
- An M3 process-local explicit LAN enable control in the native MOBILE view; it defaults OFF every launch, is not persisted, and does not open a LAN listener.
- An M3 bounded device-pairing core with short-lived CSPRNG invitation, HMAC-SHA256 code proof, strict Ed25519 proof-of-possession and explicit run-local owner approval.
- An M3 persistent trusted-device public-key store under workspace/identity with bounded versioned JSON, derived fingerprint verification, duplicate rejection and verified replacement/recovery.
- An M3 durable device-revocation ledger that tombstones revoked Ed25519 fingerprints, rejects same-key re-trust, and unions revocation state across Generic Workspace restore.
- An M3 default-deny device-permission policy with four fixed future authorization scopes, explicit native editing, effective-trust checks, and backup-restore isolation under workspace/state.
- An M3 reconnect proof boundary using fresh CSPRNG challenges and strict Ed25519 proof-of-possession, with no fixed authenticated-session expiry or persisted bearer credential.
- An M3 fail-closed public-network safety gate: Windows reads all connected Network List Manager categories, while unknown/unsupported evidence denies future LAN exposure by default.
- An M3 transport-independent mobile editor/session handoff core with live scope checks, existing Editor path/UTF-8/size enforcement, SHA-256 revision conflict detection, and recovery-draft preservation on stale saves.
- An M4 trusted-peer identity core with a create-once local Ed25519 identity, fresh proof-of-possession challenges, explicit non-TOFU trusted-peer persistence, and backup-isolated peer state.
- An M4 transport-independent encrypted text/file transfer core using signed one-shot offers, ephemeral X25519, HKDF-SHA256 and XChaCha20Poly1305; received files remain in memory and no peer socket is opened.
- An M4 received-file integrity gate that embeds a sender-authenticated SHA-256/length commitment and exposes file bytes only after `PendingReceivedFile::verify` produces `VerifiedReceivedFile`.
- An M4 direct IPv4 LAN peer transport with live LAN-intent/network-safety gating, trusted-peer recipient binding, bounded one-shot receive windows, encrypted transfer reuse, and file verification before acknowledgement.
- An M4 Internet P2P threat review whose current decision is fail-closed: Internet/global peer transport remains disabled, with no runtime/configuration override, rendezvous, relay, NAT traversal or automatic port mapping.
- An M5 embedded offline documentation pack containing README plus all docs/*.md, with deterministic SHA-256 inventory, offline CLI list/show, create-new export/verify, and a read-only native SYSTEM viewer.
- An M5 reproducible Cargo dependency-cache strategy with committed Cargo.lock, Rust 1.99.1 pinning, locked normal builds, generated versioned vendor snapshots, explicit offline source replacement, and frozen vendor CI verification.
- An M5 recovery snapshot creator that packages reviewed source/build inputs, vendor/ and one verified Generic Workspace Backup into a create-new integrity-manifested offline directory without copying live Workspace/secret state.
- No public/global peer address support, peer discovery, firewall mutation, UPnP, NAT-PMP, reverse tunnel, relay, CDN or external web request.
- Windows double-click build/update entry point.
- CI builds for Windows, macOS and Linux.

M3's transport-independent mobile security and handoff foundation is complete. M4 is complete with trusted-peer identity, encrypted transfer, received-file verification and fail-closed Direct LAN. M5 Offline continuity now includes embedded documentation, reproducible Cargo dependencies and create-only recovery snapshots; Restore verification is the next task.

## Editor safety model

The first editor intentionally supports existing UTF-8 regular files only.

- The editor cannot escape the configured workspace root.
- Symlink paths are rejected.
- Files larger than 4 MiB are rejected.
- Typing does not overwrite the original file.
- After an editing pause, a recovery draft is stored under workspace/state/autosave.
- SAVE is the operation that changes the source file.
- SAVE refuses to overwrite a file that changed externally after it was opened.
- A detected crash recovery must be restored or discarded explicitly.
- Switching files preserves dirty content as a recovery draft first.

The editor does not yet create, rename or delete files.

## Session restoration

DANGOMUSHI stores non-content session state at workspace/state/session.v1. It remembers the active view, FILES directory and selection, and the editor file path.

Unsaved editor text is not stored in the session file. It continues to use the independent recovery-draft mechanism, so a damaged session record cannot become the only copy of unsaved work.

All restored paths are relative and are validated again before use. Missing or invalid locations degrade safely instead of escaping the workspace.

## Secret subsystem

The secret-management roadmap is intentionally separate from ordinary M2 Local Tools. Its core sequence is now complete through Secret Backup / Restore: Password Generator, source-agnostic Credential inputs, encrypted Vault storage/unlock, Recovery Codes, Clipboard hygiene, and encrypted backup/restore.

Secret values are not routed through the Text Editor, generic autosave or session restoration, and are not exposed through the native UI yet. Vault ciphertext uses a dedicated storage API and is not a normal Workspace file. See docs/SECRET_DATA_BOUNDARY.md, docs/PASSWORD_GENERATOR_CORE.md, and docs/VAULT_STORAGE.md.

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
cargo run -- docs list
cargo run -- docs show docs/ARCHITECTURE.md
cargo run -- docs export
cargo run -- docs verify
cargo run -- recovery snapshot create --backup backups/<NAME>.dgmwbk --destination <PATH>
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

## Offline dependency cache

Create a local Cargo source snapshot while registry access is available:

~~~text
scripts\\vendor-dependencies.cmd
~~~

or on Unix:

~~~text
sh scripts/vendor-dependencies.sh
~~~

Then verify the complete Cargo path with registry access disabled:

~~~text
scripts\\verify-vendored-dependencies.cmd
~~~

or:

~~~text
sh scripts/verify-vendored-dependencies.sh
~~~

The generated `vendor/` directory is intentionally not committed to Git. Preserve it with the matching source tree and `Cargo.lock` when preparing long-term offline media. The Rust 1.99.1 toolchain and required OS-native build packages remain separate prerequisites.

## Recovery snapshots

After creating and verifying a normal Generic Workspace Backup and generating `vendor/`, create a higher-level offline recovery directory outside both the source tree and Workspace:

~~~text
cargo run -- recovery snapshot create \
  --backup backups/<NAME>.dgmwbk \
  --destination <PATH>
~~~

Use `--source` and `--workspace` when those roots are not the defaults. Recovery snapshots intentionally do not bundle live Workspace state or Secret Backup ciphertext. Restore/verification is a separate later M5 boundary.

## Repository layout

~~~text
dangomushi-usb/
├─ Cargo.toml
├─ Cargo.lock
├─ rust-toolchain.toml
├─ .cargo/vendor-config.toml
├─ UPDATE_TOOLBOX.cmd
├─ src/
│  ├─ main.rs
│  ├─ clipboard_hygiene.rs
│  ├─ credential.rs
│  ├─ existing_credential_input.rs
│  ├─ generated_credential_input.rs
│  ├─ hash_verify.rs
│  ├─ lib.rs
│  ├─ workspace.rs
│  ├─ workspace_backup.rs
│  ├─ file_browser.rs
│  ├─ document_search.rs
│  ├─ file_compare.rs
│  ├─ password_generator.rs
│  ├─ peer_identity.rs
│  ├─ recovery_codes.rs
│  ├─ repository_git.rs
│  ├─ secret_backup.rs
│  ├─ text_editor.rs
│  ├─ text_json.rs
│  ├─ trusted_peers.rs
│  ├─ tool_registry.rs
│  ├─ vault_storage.rs
│  ├─ vault_unlock.rs
│  ├─ server.rs
│  ├─ session_state.rs
│  ├─ native_ui.rs
│  ├─ native_ui_theme.rs
│  └─ bin/dangomushi-desktop.rs
├─ web/                 embedded loopback/mobile UI
├─ docs/                architecture, decisions and roadmap
├─ scripts/             platform build helpers
├─ app/                 generated runnable application
└─ workspace/           durable local user data
~~~

See docs/ARCHITECTURE.md for the authority, filesystem, editor, network and data boundaries.

## Security boundary

The local HTTP gateway is a presentation surface only. It exposes fixed read-only routes and a read-only status endpoint. It does not expose filesystem writes, command execution or Git operations.

The local HTTP gateway remains loopback-only. M4 peer communication has a separate one-shot direct IPv4 LAN transport that requires explicit run-local LAN intent, live trusted-network assessment and trusted-peer cryptographic authentication. Internet/global peer transport remains unavailable.

## License

No project license has been selected yet.
