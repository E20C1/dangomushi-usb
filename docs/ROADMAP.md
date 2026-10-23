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

- [ ] **SHA-256 / integrity verification** — Exactly One Next Task
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
- [x] **Credential model**
  - Service / Username or Email / URL / Password / Notes
  - secret-aware ownership and redacted formatting
  - no Vault persistence
- [x] **Add Existing Credential input boundary**
  - owned String input is immediately wrapped in SecretString
  - existing passwords are preserved without Generator policy rewrite
  - consumes into the same source-agnostic Credential model
  - no UI, Clipboard, generic autosave/session or persistence
- [x] **Encrypted Vault storage**
  - dedicated VaultStore API outside generic File Browser/Text Editor
  - XChaCha20Poly1305 authenticated encryption with fresh OS nonce
  - authenticated format header with bounded future Argon2id metadata
  - temp -> sync -> reopen/verify -> replace -> rollback/recovery
  - redacted/zeroized storage-owned key and plaintext buffers
- [x] **Vault unlock / lock**
  - Master Passphrase is consumed into a redacted/zeroized core-owned buffer
  - Argon2id v=19 derives the 32-byte VaultKey from authenticated KDF metadata
  - default new-Vault profile: 19 MiB / 2 iterations / p=1
  - explicit UnlockedVault lifetime and consuming lock()
  - wrong passphrase / AEAD failure maps to a generic unlock failure
  - no UI, session secret restoration, Credential payload format, Recovery Code, or Clipboard behavior
- [x] **Generator -> Credential integration**
  - GeneratedPassword is consumed into SecretString without a required clone
  - generated and existing password paths converge on the same Credential model
  - integration remains in-memory and outside Text Editor/autosave/session/Vault serialization
- [x] **Recovery Codes**
  - dedicated RecoveryCode / RecoveryCodeSet secret-aware model
  - exact text preservation with bounded count/length
  - versioned length-delimited binary Vault section payload
  - redacted Debug and best-effort zeroization of directly owned secret buffers
  - VaultPlaintext decode boundary; no generic files/autosave/session/Clipboard
- [x] **Clipboard hygiene**
  - explicit `ClipboardSecret` ownership with redacted Debug and best-effort zeroization
  - configurable delayed clearing with a 30-second default policy
  - compare-before-clear: changed, empty, or non-text clipboard content is preserved
  - temporary clipboard readback is secret-owned and zeroized on Drop
  - injectable backend for deterministic core tests; desktop OS adapter is feature-gated
  - no PASSWORDS UI, generic autosave/session persistence, or Secret Backup / Restore integration
- [x] **Secret Backup / Restore**
  - versioned `DGMBAK01` container wraps existing authenticated Vault ciphertext
  - backup creation requires an authenticated unlocked source Vault
  - no decrypted Credential / Recovery Code / Vault plaintext passes through generic backup/export helpers
  - create-new pending/write/sync/reread flow refuses silent overwrite of an existing backup
  - restore derives the backup Vault key from embedded KDF metadata and Master Passphrase
  - backup ciphertext is authenticated before destination mutation
  - stable destination replacement preserves rollback until restored ciphertext re-authenticates
  - ambiguous pending/previous destination layouts are rejected instead of guessed at

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
