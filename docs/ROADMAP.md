# DANGOMUSHI Roadmap

This roadmap records implementation order, not release dates.

## M0 — Core bootstrap

- [x] Rust project foundation
- [x] Cross-platform workspace layout
- [x] init and doctor commands
- [x] Embedded local status UI
- [x] Loopback-only HTTP boundary
- [x] Windows double-click build/update path
- [x] CI build/test matrix

## M1 — Native desktop shell

- [x] Select the native Rust UI layer after a small prototype comparison
- [x] Bootstrap the native desktop shell
- [x] File browser rooted in the configured workspace
- [x] Text editor with explicit save and autosave recovery
- [x] Tool launcher
- [x] Workspace/session restoration
- [x] Pixel-oriented UI system and original icon set

## M2 — Local tools

- [ ] SHA-256 / integrity verification
- [ ] Text / JSON utilities
- [ ] File comparison
- [ ] Local document search
- [ ] Repository status and safe Git operations
- [ ] Backup and restore workflow

## M2-S — Secret management subsystem

This is a security-sensitive subsystem separate from ordinary M2 Local Tools. Normal M2 work continues independently.

- [x] Secret Data Boundary
- [x] **Password Generator Core**
  - production randomness from the operating system CSPRNG
  - configurable length / uppercase / lowercase / digits / symbol set
  - optional minimum one character from every selected group
  - ambiguous-character exclusion option
  - sampling that avoids modulo bias
  - injectable randomness source for deterministic core tests
  - no UI or Vault dependency
- [ ] **Credential model** — Exactly One Next Task
  - Service / Username or Email / URL / Password / Notes
  - secret-aware ownership and redacted formatting
  - no Vault persistence yet
- [ ] Add Existing Credential input boundary
- [ ] Encrypted Vault storage
- [ ] Vault unlock / lock
- [ ] Generator -> Credential integration
- [ ] Recovery Codes
- [ ] Clipboard hygiene
- [ ] Secret Backup / Restore

Both `Generate new password` and `Enter existing password` must converge on the same Credential persistence flow before encrypted Vault storage.

## M3 — Authenticated mobile LAN access

- [ ] Explicit LAN enable control
- [ ] Device pairing
- [ ] Persistent trusted-device public keys
- [ ] Device revocation
- [ ] Permission scopes
- [ ] Reconnection without short fixed session expiry
- [ ] Public-network safe default
- [ ] Mobile editor/session handoff

No LAN listener should be merged before these security boundaries are implemented and tested.

## M4 — Peer communication

- [ ] Trusted peer identity
- [ ] Encrypted text/file transfer
- [ ] Integrity verification for received files
- [ ] Direct LAN path first
- [ ] Internet P2P only after a separate threat review

## M5 — Offline continuity

- [ ] Offline documentation packs
- [ ] Reproducible dependency cache/vendor strategy
- [ ] Recovery snapshots
- [ ] Restore verification
- [ ] Portable release bundles per operating system
