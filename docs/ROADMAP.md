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
- [ ] Tool launcher
- [ ] Workspace/session restoration
- [ ] Pixel-oriented UI system and original icon set

## M2 — Local tools

- [ ] SHA-256 / integrity verification
- [ ] Text / JSON utilities
- [ ] File comparison
- [ ] Local document search
- [ ] Repository status and safe Git operations
- [ ] Backup and restore workflow

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
