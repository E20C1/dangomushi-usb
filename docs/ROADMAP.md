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

- [x] **SHA-256 / integrity verification core**
  - read-only existing regular files below the configured Workspace root
  - relative normal-component paths only; traversal and symlink paths rejected
  - streaming SHA-256 calculation with bounded memory
  - strict 64-hex expected digest parsing with uppercase/lowercase support
  - mismatch is a normal verification result and never modifies the target
  - no UI, network, generic Backup, Repository operation, or secret-data integration
- [x] **HASH / VERIFY native UI integration**
  - Tool Launcher entry is READY and explicitly dispatches to a dedicated native view
  - accepts a Workspace-relative target path or the current regular FILES selection
  - calculate-only and expected-digest verification flows both reuse the read-only SHA-256 core
  - match, mismatch, bytes hashed and calculated digest are shown without persisting result/input content
  - session restoration stores only the HASH / VERIFY navigation view, not target/digest/result values
- [x] **Text / JSON utilities**
  - in-memory non-secret utility with no automatic source-file writes
  - LF normalization and trailing ASCII space/tab cleanup
  - JSON syntax validation with line/column-only errors
  - pretty/compact JSON formatting changes only insignificant whitespace outside strings
  - formatting preserves key order, duplicate keys, numeric spellings and string escape content
  - 4 MiB input and 16 MiB output bounds
  - Tool Launcher entry is READY; FILES selection can be loaded through the existing UTF-8 read boundary
  - session restoration stores only the active TEXT / JSON view, not utility input/output
- [x] **File comparison**
  - read-only binary-safe comparison for two existing regular Workspace files
  - 64 KiB streaming comparison with bounded memory
  - reports EQUAL / DIFFERENT, both file sizes and first differing byte offset
  - exact-prefix length differences report the first byte that exists only in the longer file
  - optional UTF-8 text detail for files up to 4 MiB reports line counts and first differing line
  - symlink, traversal, absolute-path, directory and out-of-workspace targets are rejected
  - Tool Launcher entry is READY; FILES can assign the current regular file as Compare A or B
  - session restoration stores only the active COMPARE view, not paths/results/file contents
- [x] **Local document search**
  - read-only exact literal search restricted to workspace/data/documents
  - fixed documents root must be real in-Workspace directories; recursive symlinks are skipped
  - UTF-8 regular files only, with a 4 MiB per-file hard read bound
  - query is non-empty, single-line and capped at 4 KiB
  - maximum 10,000 regular files considered and 500 matching lines returned
  - results report Workspace-relative path, one-based line/column and a bounded preview
  - oversized and non-UTF-8 files are skipped with explicit counts
  - Tool Launcher entry is READY and result OPEN reuses the existing Editor boundary
  - session restoration stores only the active SEARCH view, not query/results/previews
- [x] **Repository status and safe Git operations**
  - direct-child repositories only under workspace/repositories
  - real repository directory plus real .git directory; symlink repos and gitfiles are excluded
  - fixed read-only STATUS / RECENT COMMITS / WORKTREE DIFF STAT / STAGED DIFF STAT operations
  - no shell, arbitrary argv, remote/network operation, staging, commit, checkout, reset, clean, stash, merge or rebase
  - status uses porcelain output with optional Git locks disabled
  - diff stats disable external diff helpers and textconv execution and ignore submodules
  - Git environment redirection variables are removed; stdin/prompting are disabled
  - retained stdout/stderr is bounded to 1 MiB each and Git runtime to 15 seconds
  - Tool Launcher entry is READY; repository selection and Git output remain ephemeral
  - session restoration stores only the active REPOSITORIES view
- [x] **Backup and restore workflow**
  - versioned DGMWBK01 generic Workspace snapshot container with per-file SHA-256 and whole-archive SHA-256
  - scope is data/, repositories/, identity/, config/ and state/autosave/
  - backups/ and state/session.v1 are intentionally excluded
  - symlinks, special files and non-UTF-8 path components are rejected rather than silently omitted
  - streaming 64 KiB I/O with bounded path, entry and payload limits
  - CREATE uses hidden pending -> sync -> full verify -> create-new final copy -> sync -> full verify
  - VERIFY checks format, path bounds, duplicate/parent structure, each file digest and archive digest without mutation
  - RESTORE fully verifies/extracts to hidden staging before replacing managed roots
  - current roots are retained in hidden rollback staging and restored in reverse order on in-process replacement failure
  - RESTORE requires explicit RESTORE confirmation and refuses dirty/pending Editor state
  - successful restore resets filesystem-derived native UI state; session stores only the BACKUP navigation view
  - generic Workspace backup remains separate from M2-S Secret Backup / Restore

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

- [x] **Explicit LAN enable control**
  - process-local operator-intent gate; every application launch starts disabled
  - enable/disable requires an explicit native MOBILE action
  - control state is intentionally absent from session.v1, Workspace config and generic backups
  - enabling the control does not authorize or create a LAN listener; the gateway remains loopback-only
- [x] **Device pairing**
  - process-local pairing state machine is gated by the explicit LAN enable control
  - OS CSPRNG creates a short-lived 80-bit human pairing code and independent 256-bit challenge
  - the future device transport sends HMAC-SHA256 proof rather than the raw pairing code
  - Ed25519 strict signature verification proves possession of the submitted device private key
  - pairing expires after five minutes, allows at most five cryptographic proof failures, and is one-time
  - successful proof still requires explicit desktop approval and produces only a run-local PairedDeviceCandidate
  - no trusted-device key is persisted and no LAN listener is opened by this task
- [x] **Persistent trusted-device public keys**
  - explicit post-pairing trust action persists only device name, Ed25519 public key and derived fingerprint
  - versioned bounded store lives under Workspace identity/ and rejects symlink/special-file layouts
  - duplicate keys, malformed records, fingerprint mismatch and device-count overflow are rejected
  - pending/previous files provide verified replacement and fail-closed interrupted-write recovery
  - generic Workspace Backup includes the trust store because identity/ is a managed backup root
  - no deletion/revocation action and no LAN listener are added by this task
- [x] **Device revocation**
  - explicit native revocation writes a durable public-key fingerprint tombstone
  - effective trusted-device reads fail closed when revocation state cannot be verified
  - revoked public keys cannot be registered again; re-trust requires device key rotation and new pairing
  - Generic Workspace restore unions live and staged revocation ledgers before identity replacement
  - older backups cannot remove a locally known revocation, while post-revocation backups carry tombstones portably
  - no un-revoke action, permission scope, reconnect policy or LAN listener is added by this task
- [x] **Permission scopes**
  - fixed initial scope vocabulary: `status.read`, `files.read`, `files.write`, `session.handoff`
  - persistent trusted devices remain default-deny until scopes are explicitly saved in the native MOBILE view
  - durable policy lives at `workspace/state/device_permissions.v1.json`, outside Generic Workspace Backup restore
  - authorization requires the fingerprint to remain effectively trusted and non-revoked
  - new persistent trust clears any stale permission record for the same fingerprint before trust is committed
  - no remote request API, reconnect/session policy or LAN listener is added by this task
- [x] **Reconnection without short fixed session expiry**
  - every reconnect uses a fresh 256-bit OS-CSPRNG challenge and strict Ed25519 proof with the already trusted device key
  - the reconnect challenge is one-time and short-lived, but the authenticated in-process principal has no fixed session expiry
  - no durable bearer token, refresh token, cookie secret or reconnect credential is persisted
  - effective trust/revocation and current permission scopes are re-evaluated when the authenticated principal is used
  - reconnecting after transport loss requires a new key proof but not a new human pairing code while the key remains trusted
  - no public-network policy, remote API or LAN listener is added by this task
- [x] **Public-network safe default**
  - LAN exposure requires both explicit process-local LAN intent and a live trusted-network assessment
  - Unknown, Public, probe failure, zero connected networks and unsupported platform all fail closed
  - Windows reads all connected Network List Manager categories and denies if any connected network is Public or Unknown
  - only all-Private / DomainAuthenticated Windows evidence is exposure-eligible
  - macOS/Linux remain Unknown/deny until an equivalently trustworthy reviewed adapter exists
  - network-safety snapshots are ephemeral and never persisted as capabilities
  - a future non-loopback transport must re-probe immediately before bind and before accepting or authorizing each remote connection
  - src/server.rs remains loopback-only; no LAN listener is added by this task
- [x] **Mobile editor/session handoff**
  - transport-independent handoff core exposes validated navigation state plus existing UTF-8 Editor documents
  - session capture requires `session.handoff`; editor reads require `session.handoff` + `files.read`; saves require `session.handoff` + `files.write`
  - editor reads return a SHA-256 content revision; stale saves refuse overwrite and preserve submitted mobile text through the existing recovery-draft boundary
  - the existing Text Editor relative-path, no-symlink, UTF-8 and 4 MiB rules remain authoritative
  - handoff payloads, Editor text, revisions and authenticated principals remain ephemeral and are not added to `session.v1`
  - no LAN listener, remote HTTP/WebSocket route, mobile web UI or transport protocol is added by this task

No LAN listener should be merged before these security boundaries are implemented and tested.

M3's transport-independent security and handoff boundaries are now complete. Any future remote transport must compose them without weakening live network-safety, trust, revocation, reconnect or permission checks.

## M4 — Peer communication

- [x] **Trusted peer identity**
  - explicit create-once local Ed25519 identity generated from the OS CSPRNG
  - private peer identity lives under `workspace/state`, outside Generic Workspace Backup
  - public-key fingerprints are SHA-256 with peer-specific domain separation from M3 device identity
  - fresh 256-bit challenge plus strict Ed25519 proof-of-possession produces a verified peer candidate
  - persistent trust requires a separate explicit action; names, addresses and bare public keys never create TOFU trust
  - bounded versioned trusted-peer policy lives under `workspace/state` and is not rolled back by Generic Workspace restore
  - explicit peer removal requires fresh proof and explicit trust before the same key can be trusted again
  - no LAN listener, peer discovery, encrypted transfer or file receiver is added by this task
- [x] **Encrypted text/file transfer**
  - one-shot signed receiver offers bind transfer ID, expected sender, recipient identity and ephemeral X25519 public key
  - fresh ephemeral X25519 on both sides; long-lived Ed25519 peer identity remains authentication-only
  - non-contributory X25519 shared secrets fail closed
  - HKDF-SHA256 derives a per-transfer 256-bit key from the DH secret with transfer/identity context binding
  - XChaCha20Poly1305 encrypts one bounded text or file payload with the fixed envelope header as associated data
  - sender Ed25519 signature authenticates the envelope digest before receiver decryption
  - pending receive is process-local, five-minute bounded and consumed on one open attempt; replayed envelopes cannot bind to a new offer
  - strict versioned binary framing rejects truncation, trailing bytes, invalid lengths and over-limit payloads
  - file transfer returns only bytes plus a validated suggested filename; it never selects or writes a filesystem destination
  - no LAN listener, peer discovery, file receiver persistence or Internet P2P path is added
- [x] **Integrity verification for received files**
  - sender computes SHA-256 over raw file bytes before encryption and includes the digest plus declared byte length inside the authenticated encrypted payload
  - decrypted files become a `PendingReceivedFile`; raw bytes remain private until verification succeeds
  - verification consumes the pending file, checks authenticated length, recomputes SHA-256 and fails closed on mismatch
  - only `VerifiedReceivedFile` exposes received file bytes through the safe public API
  - verified metadata retains the trusted sender fingerprint, suggested filename, verified byte length and SHA-256
  - the digest authenticates equality with the trusted sender's signed/encrypted commitment; it is not malware scanning or content-safety approval
  - no filesystem receiver, destination selection, overwrite behavior or LAN listener is added
- [x] **Direct LAN path first**
  - first non-loopback peer transport is direct TCP over IPv4 only; no discovery, relay or NAT traversal
  - public API requires explicit process-local LAN enablement plus a fresh current-network safety check
  - Windows permits exposure only while all connected Network List Manager entries remain Private and/or DomainAuthenticated; macOS/Linux remain fail-closed
  - receiver re-probes before bind, continuously while waiting, after accept, before signed offer, before encrypted payload authorization and before final acknowledgement
  - sender re-probes before connect, after connect and before sealing/sending the encrypted envelope
  - target/accepted addresses are restricted to IPv4 loopback, RFC1918 private or IPv4 link-local; global/CGNAT/IPv6 targets are rejected
  - receive window is one-shot and bounded to five minutes; listener is dropped after the first accepted connection
  - sender hello fingerprint is only a selector; current trusted-peer policy and the existing signed one-shot offer still provide cryptographic peer authentication
  - `seal_transfer_to` binds the send to the caller's explicitly expected recipient fingerprint
  - file transfers are acknowledged only after `PendingReceivedFile::verify` produces `VerifiedReceivedFile`
  - no filesystem receive write, firewall mutation, UPnP/NAT-PMP, relay, public discovery or Internet transport is added
- [x] **Internet P2P threat review**
  - decision: Internet P2P remains DISABLED / DEFERRED in the current roadmap
  - threat model covers public listeners, global addressing, NAT traversal, automatic router/firewall mutation, rendezvous/discovery, relay metadata/operations, DoS/resource exhaustion, malicious trusted peers, key compromise, traffic analysis, downgrade/versioning and local-first availability
  - no public/global listener, rendezvous, relay, NAT traversal, UPnP/NAT-PMP/PCP, public discovery or Internet peer transport is authorized by the review
  - `internet_p2p_policy` is fail-closed with no runtime/configuration override
  - future reconsideration requires a new scoped milestone satisfying the documented security, privacy, abuse, resource and recovery prerequisites
  - M4 closes with Direct LAN as the supported peer transport; Internet P2P is explicitly not implemented

## M5 — Offline continuity

- [x] **Offline documentation packs**
  - `README.md` plus every repository `docs/*.md` file is compile-time embedded into the core binary
  - embedded paths are fixed relative-normal UTF-8 paths with duplicate/size validation and no remote lookup
  - deterministic pack summary binds format version, every path, document length and per-document SHA-256
  - CLI supports offline `docs list`, exact `docs show`, create-new `docs export` and exact `docs verify`
  - export refuses overwrite, syncs each created file and writes a pack marker with app version/count/size/digest
  - export verification rejects missing/modified/extra files, symlinks and special entries
  - native SYSTEM view provides a read-only embedded-document selector/viewer without session persistence
  - pack content is application data and is not automatically copied into Workspace, Generic Backup, Secret Backup or autosave
  - no HTTP client, DNS, Git, shell, CDN or other network dependency is introduced
- [x] **Reproducible dependency cache/vendor strategy**
  - committed `Cargo.lock` is the authoritative Rust package resolution and routine update/CI paths use `--locked`
  - reproducible Rust baseline is pinned to toolchain 1.99.1 rather than a moving stable channel
  - explicit helpers generate `vendor.pending/` with `cargo vendor --locked --versioned-dirs` and promote only after generation succeeds
  - generated `vendor/` is ignored by Git and is treated as an external cache artifact to preserve with the matching source/lockfile when offline rebuildability is required
  - dedicated `.cargo/vendor-config.toml` replaces crates.io with repository-root `vendor/` and forces Cargo offline only when explicitly selected
  - frozen verification covers metadata, core tests/build, desktop library tests and native desktop release build
  - CI pins toolchain 1.99.1, uses `--locked` on all normal Cargo paths and independently exercises a fresh vendor snapshot plus frozen offline build on Ubuntu
  - strategy does not claim hermetic OS/toolchain reproduction; system SDKs/native packages and preserved rustup distributions remain external prerequisites
  - dependency cache is build infrastructure and does not enter Workspace, Generic Backup, Secret Backup, autosave or session state
- [ ] **Recovery snapshots** — Exactly One Next Task
- [ ] Restore verification
- [ ] Portable release bundles per operating system
