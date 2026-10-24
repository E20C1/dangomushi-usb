# DANGOMUSHI Architecture

## Goal

DANGOMUSHI is a portable workspace whose useful state remains local and whose core functions do not require a vendor account, cloud service, or public internet connection.

The application is not intended to become a general-purpose web browser.

## Authority and data model

The Rust core is the authority for privileged operations.

Desktop and browser-delivered UI are client surfaces. Neither is the source of truth for files, identity, repository state, backups or permissions.

The intended durable layout is:

~~~text
DANGOMUSHI/
├─ app/          replaceable program output
├─ workspace/    durable user data
├─ build/        temporary staging and rollback material
└─ source tree   development source
~~~

Build and update operations may replace app/. They must not erase workspace/.

The installed desktop executable resolves a sibling workspace/ from the portable root. Development runs fall back to ./workspace. DANGOMUSHI_WORKSPACE can explicitly override the path.

## Filesystem boundary

The native file browser accepts only relative paths composed of normal path components.

Before a directory is traversed, both the workspace root and requested directory are canonicalized and the resolved directory must remain below the canonical workspace root.

Symlinks are listed as links but are not treated as traversable directories.

The text editor applies a stricter file boundary:

- only existing regular files can be opened,
- the path must remain below the canonical workspace root,
- symlinks in the edited path are rejected,
- content must be valid UTF-8,
- files are limited to 4 MiB in the initial editor,
- the editor does not create, rename or delete user files.

## Integrity verification boundary

The M2 HASH / VERIFY core is implemented in src/hash_verify.rs. It accepts only non-empty relative Workspace paths composed of normal path components, rejects symlink components, canonicalizes the target, requires the resolved file to remain below the canonical Workspace root, and requires a regular file.

Hashing is read-only and streaming. Files are read in 64 KiB chunks and are not copied into the Text Editor, autosave, session state, Vault, network gateway, or generic Backup path. The core returns the SHA-256 digest and the number of bytes actually read.

Expected SHA-256 values are accepted only as exactly 64 hexadecimal characters after surrounding whitespace is trimmed. Verification returns the calculated digest plus match/mismatch state; a mismatch is not treated as an I/O failure.

SHA-256 comparison provides integrity only relative to a trusted expected digest. It does not authenticate the producer of a file when an attacker can replace both the file and its expected digest.

The portable path validation does not claim an operating-system-level atomic no-follow guarantee if another process replaces filesystem entries concurrently between validation and open.

The native HASH / VERIFY view now exposes this core through the Tool Launcher and selected regular files in FILES. Its target, expected digest, calculated digest and verification result remain ephemeral UI state; generic session restoration stores only the active HASH / VERIFY navigation view.

## Text / JSON utility boundary

The M2 TEXT / JSON utility core is implemented in src/text_json.rs and is intentionally in-memory and non-secret.

The core accepts at most 4 MiB of UTF-8 input and bounds transform output to 16 MiB. Text operations normalize CRLF/bare-CR line endings to LF and can additionally remove only trailing ASCII space/tab characters from each line.

JSON syntax validation uses serde_json 1.0.154. Validation errors expose line and column without echoing input content. Pretty/compact transforms first validate the complete document, then change only insignificant JSON whitespace outside string literals rather than deserializing and reserializing the document. This preserves object-key order, duplicate object keys, number spelling, escape spelling, and string content.

The native TEXT / JSON view may load the currently selected regular FILES entry through the existing UTF-8 Text Editor read boundary. The utility does not automatically write output back to the source file, create recovery drafts, use Clipboard transport, or send content over the network.

Utility input/output and validation state are ephemeral. Generic session restoration stores only the active TEXT / JSON navigation view.

TEXT / JSON is a generic utility and is not a secret-processing surface. Passwords, Master Passphrases, Recovery Codes, decrypted Vault data, and similar secret values remain outside this boundary.

## File comparison boundary

The M2 COMPARE core is implemented in src/file_compare.rs and is read-only.

Both targets must be existing regular files below the configured Workspace root. Relative normal-component paths are required; symlink path components are rejected; canonical targets must remain below the canonical Workspace root.

The authoritative equality result is binary-safe and byte-exact. Files are compared incrementally with 64 KiB buffered reads so large files are not loaded completely into memory. The result contains both byte lengths and the first differing zero-based byte offset when files differ.

When both files are at most 4 MiB and both decode as UTF-8, COMPARE may additionally report line counts and the first differing one-based line. This text detail is supplementary and does not normalize or replace the byte-exact comparison.

The native COMPARE view accepts two Workspace-relative paths and can adopt the current regular FILES selection as either side. It never writes source files. Compare paths, contents and results remain ephemeral; generic session restoration stores only the active COMPARE navigation view.

COMPARE is a generic non-secret file utility and must not be used as a route for decrypted Vault data or other secret-subsystem plaintext.

## Local document search boundary

The M2 SEARCH core is implemented in src/document_search.rs and is read-only.

Search is restricted to the fixed Workspace subtree data/documents. The data and documents path components must be real directories rather than symlinks, recursive symlink entries are skipped, and canonical traversed directories/files must remain below both the canonical documents root and Workspace root.

The query is an exact, case-sensitive, single-line UTF-8 literal capped at 4 KiB. Search considers at most 10,000 regular files and returns at most 500 matching lines.

Only UTF-8 regular files up to 4 MiB are searched. Reads use a 4 MiB + 1 byte hard limit so a file that grows after metadata inspection cannot cause an unbounded search read. Oversized and non-UTF-8 files are skipped and counted.

Each result contains a Workspace-relative path, one-based line and Unicode-character column, plus a preview capped at 240 characters. The native SEARCH / DOCUMENTS view can pass a result path to the existing Editor boundary through an explicit OPEN action; the search core itself never writes a document.

Search query text, previews and result/report state remain ephemeral. Generic session restoration stores only the active SEARCH navigation view.

SEARCH is a generic non-secret utility and must not be used as a route for decrypted Vault data or other secret-subsystem plaintext.

## Repository Git boundary

The M2 REPOSITORIES core is implemented in src/repository_git.rs.

It inspects only direct child working trees under workspace/repositories. The repositories root, selected repository directory, and .git entry must be real directories rather than symlinks; linked worktrees represented by a .git file, bare repositories, nested arbitrary paths, and repositories outside this fixed root are excluded from this first boundary.

Repository operations are intentionally read-only and fixed by the Rust core: STATUS, RECENT COMMITS, WORKTREE DIFF STAT, and STAGED DIFF STAT. The UI cannot provide an arbitrary shell command, Git subcommand, argv list, revision expression, pathspec, remote, or executable.

Git is launched directly with std::process::Command and no shell. Stdin and terminal prompting are disabled. Environment variables that can redirect Git repository/index/object paths, external diff, pagers/editors, askpass/SSH helpers, config parameters, or Windows standard handles are removed before launch.

STATUS uses machine-oriented porcelain output and disables optional locks to avoid Git's optional index-refresh write. Submodule recursion is disabled. Diff-stat operations explicitly disable external diff helpers and textconv execution, ignore submodules, disable rename detection and color output. No clone/fetch/pull/push or other network operation is exposed.

Each Git process has a 15-second runtime limit. stdout and stderr are drained concurrently while retaining at most 1 MiB of each, preventing the UI from retaining unbounded command output. stderr content is not echoed through ordinary UI errors.

DANGOMUSHI does not override Git safe.directory ownership checks. A repository that Git itself refuses as unsafe remains unavailable through this surface.

The portable validation-before-launch sequence does not claim an operating-system-level atomic no-follow guarantee against another process replacing a repository directory between validation and Git process launch.

The native REPOSITORIES view can enumerate up to 512 supported repositories, render up to 500 status rows, show up to 20 recent commits, and show bounded worktree/staged diff statistics. Repository selection and Git outputs remain ephemeral; generic session restoration stores only the active REPOSITORIES navigation view.

REPOSITORIES is a generic non-secret utility and must not be used as a route for decrypted Vault data or other secret-subsystem plaintext.

## Generic Workspace backup / restore boundary

The M2 BACKUP core is implemented in src/workspace_backup.rs and is separate from the M2-S encrypted Secret Backup / Restore path.

Generic snapshots cover data/, repositories/, identity/, config/, and state/autosave/. The backups/ directory itself and state/session.v1 are excluded so snapshots do not recursively contain snapshots and restore does not replace the current session record.

The versioned DGMWBK01 archive stores bounded relative UTF-8 path components, directory records, regular-file lengths, raw file bytes, a SHA-256 for each file, and a final SHA-256 over the complete hashed archive region. DGMEND01 marks the footer. These hashes detect corruption; they do not authenticate the producer of a backup.

Backup creation rejects symlinks, special files, and non-UTF-8 path components instead of silently omitting them. File content is streamed with 64 KiB buffers. The first format bounds the archive to 250,000 records, 512 GiB per file and 2 TiB total regular-file payload.

CREATE writes a hidden create-new pending archive, syncs it, fully verifies it, copies it into a create-new final .dgmwbk file, syncs and fully verifies the final file, then removes the pending file and syncs the backups directory. A dirty Editor buffer is first written through the existing generic recovery-draft boundary so that state/autosave can capture it.

VERIFY is read-only and checks format, managed-root membership, component bounds, duplicate paths, parent ordering, payload bounds, every per-file SHA-256, every required managed-root record, the archive SHA-256 and absence of trailing bytes.

RESTORE requires a direct final backup file below workspace/backups. The selected archive is fully extracted into hidden staging and verified before any managed Workspace root is replaced. Existing data/, repositories/, identity/, config/, and state/autosave/ roots are moved to hidden rollback staging; verified staged roots are then renamed into place. If an in-process replacement step fails, previously replaced roots are processed in reverse order and the core attempts to restore the original roots from rollback staging.

The multi-root replacement is not one OS-atomic transaction. A crash, OS failure, power loss or removable-device interruption can leave .restore-staging-* / .restore-rollback-* artifacts and a partially replaced Workspace. A later restore refuses to proceed while such artifacts exist rather than guessing which side is authoritative. This M2 boundary provides verified staging and in-process rollback, not crash-atomic multi-root restore.

Generic backup creation is also not a filesystem-wide frozen snapshot. The core detects common source-file changes during a file read by checking length and modification metadata, but concurrent external changes across multiple files can still produce a cross-file state that was not present at one single instant.

The native BACKUP view requires exact RESTORE confirmation and refuses restore while the currently open Editor has dirty content or an unresolved recovery choice. After successful restore, filesystem-derived native UI state is reset before the user continues.

Backup selection, verification summary and restore confirmation remain ephemeral. Generic session restoration stores only the active BACKUP navigation view.

BACKUP is a generic non-secret utility. Decrypted Vault payloads, passwords, Master Passphrases, Recovery Codes and other secret-subsystem plaintext must not be routed into ordinary Workspace files for this snapshot path. The existing M2-S Secret Backup / Restore remains the dedicated Vault-ciphertext backup boundary.

## Save and recovery model

Typing never automatically overwrites the source file.

After editing pauses for approximately two seconds, the current buffer is written to a recovery draft below:

~~~text
workspace/state/autosave/
~~~

Recovery filenames are unique per write. A newly durable recovery is created before older drafts for the same path are removed, avoiding in-place replacement of the only recovery copy.

An explicit SAVE first persists a recovery draft, then truncates and writes the original file and calls sync_all. The recovery is cleared only after the explicit file write has completed successfully.

If the original file changes outside DANGOMUSHI after opening, SAVE refuses to overwrite it and stores the current editor buffer as recovery instead.

When a file is opened and a recovery differs from disk, the UI requires the user to choose RESTORE RECOVERY or DISCARD RECOVERY before editing continues.

## Session restoration

Workspace navigation state is stored separately from editor recovery under:

~~~text
workspace/state/session.v1
~~~

The session contains only UI/work position:

- active native view,
- current FILES directory,
- selected FILES entry,
- currently open editor path.

It never stores editor buffer contents. Unsaved text remains the responsibility of the independent recovery-draft mechanism under workspace/state/autosave.

Session paths are relative-only, reject parent traversal, and are revalidated against the live workspace before the UI adopts them. Invalid or missing directories fall back to the workspace root, invalid selections are dropped, and an editor path that can no longer be opened is ignored.

Session writes use a newly synced temporary file, preserve the previous session while replacing the current file, and restore the previous file if final replacement fails. The state directory itself must be a real in-workspace directory and must not be a symlink.

## Secret data boundary

Password, Credential, Master Passphrase, Recovery Code, decrypted Vault payloads and similar secret values are governed by the stricter contract in [SECRET_DATA_BOUNDARY.md](SECRET_DATA_BOUNDARY.md).

The generic Text Editor, `workspace/state/autosave/`, `workspace/state/session.v1`, ordinary plaintext Workspace files, and diagnostic/logging paths are explicitly **not** secret-storage mechanisms.

Future encrypted Vault persistence must use a dedicated API boundary that is not exposed through the normal File Browser or Text Editor. Its persistence design must support durable temporary ciphertext, sync, verification, replacement and rollback suitable for removable storage.

The in-memory Credential model is source-agnostic and owns Service, Username or Email, URL, Password and Notes through a redacted `SecretString` wrapper. Neither `SecretString` nor `Credential` implements value-revealing Display/Debug behavior, generic dereference helpers, Clone, or serialization. This is an accidental-disclosure reduction boundary, not a guarantee of complete memory erasure.

Existing credential input uses a separate in-memory boundary that accepts owned Strings and immediately wraps them in `SecretString`. Converting that input into `Credential` consumes the boundary object and moves its owned secret values without a required clone. The boundary has no filesystem, session, autosave, Clipboard, serialization, or Vault behavior.

Generated credential input uses a parallel in-memory boundary. `GeneratedPassword` exposes a crate-private consuming ownership transfer that moves its owned String directly into `SecretString`, then into the same source-agnostic `Credential` model. The production path does not require a generated-password clone and does not route through generic persistence or formatting helpers.


Encrypted Vault persistence is implemented by the dedicated `VaultStore` API in `src/vault_storage.rs`. It does not import the generic File Browser, Text Editor, autosave, or session modules. The store uses fixed filenames inside an explicitly supplied dedicated Vault directory and rejects symlink roots/fixed-file entries.

Format v1 uses RustCrypto XChaCha20Poly1305 AEAD. The fixed cleartext header, including bounded future Argon2id KDF metadata and the 24-byte nonce, is authenticated as associated data. No unauthenticated plaintext is returned.

Save durability follows create-new pending ciphertext -> file sync -> reopen/authenticate/decrypt verification -> preserve current as previous -> promote pending -> sync -> reopen/authenticate current -> delete previous. Interrupted layouts can promote a valid pending Vault or roll back to a valid previous Vault.

The Vault directory must remain outside the generic Workspace browser root before user-facing Vault functionality is enabled.

Vault unlock/lock is implemented by `src/vault_unlock.rs`. `MasterPassphrase` consumes an owned String, stores it in a redacted core-owned byte buffer, and zeroizes that directly owned buffer on Drop. Argon2id version 19 derives the 32-byte VaultKey from the Vault header's bounded KDF metadata.

`UnlockedVault` owns the VaultStore, VaultKey, and KDF metadata for a deliberate lifetime. `lock(self)` consumes the unlocked object and drops the VaultKey before returning the non-secret store. Wrong-passphrase and AEAD-authentication failures are intentionally reduced to the same generic unlock failure.

Recovery Codes use a dedicated `RecoveryCode` / `RecoveryCodeSet` model in `src/recovery_codes.rs`. Directly owned code Strings are redacted in Debug and zeroized on Drop as best effort. Recovery Codes are encoded only into a bounded, versioned, length-delimited `RecoveryCodeVaultPayload` plaintext section intended for authenticated Vault data; they are never ordinary Workspace JSON/TOML/CSV/text, generic autosave, or session state. The complete combined Vault document schema remains intentionally undefined so Credential and Recovery Code sections can be composed later without making the Recovery Code task define unrelated Credential serialization.

Clipboard hygiene is implemented in `src/clipboard_hygiene.rs`. `ClipboardSecret` makes each DANGOMUSHI-owned clipboard copy explicit, redacts Debug output, and best-effort zeroizes its directly owned String on Drop. `SecretClipboard` owns both a long-lived clipboard backend and at most one active delayed-clear lease. When the configured deadline is reached, DANGOMUSHI reads the current clipboard text into a temporary zeroized buffer and clears the clipboard only if that observed text still matches the secret DANGOMUSHI placed there. If the clipboard is empty, non-text, or contains different text, the content is preserved and the lease is discarded. Backend failures remain generic and do not include clipboard contents. This compare-before-clear check is not atomic with the OS clear operation, so a narrow external-change race remains possible and is documented rather than hidden.

Secret Backup / Restore is implemented in `src/secret_backup.rs`. Backup creation asks an unlocked Vault to export its already-authenticated `vault.dgmv` ciphertext and wraps that ciphertext in a small versioned `DGMBAK01` container. It does not decrypt Credential or Recovery Code content into the generic Workspace backup/export layer and does not introduce another encryption primitive. Backup creation uses create-new files, a same-directory pending file, file sync, reread verification, and refuses to overwrite an existing backup path.

Restore parses the backup container, reads the inner Vault KDF metadata, derives the Vault key from the supplied Master Passphrase, and authenticates the encrypted Vault payload before mutating the destination. A destination with leftover `vault.pending.dgmv` or `vault.previous.dgmv` is rejected as ambiguous rather than guessed at. For a stable destination, the existing Vault is preserved as the normal rollback file while the authenticated backup ciphertext is promoted and verified; the previous Vault is removed only after the restored current Vault authenticates successfully.






Cryptographic primitives must come from maintained existing Rust crates selected after an implementation-time review of official documentation and security/maintenance properties. DANGOMUSHI does not claim guaranteed complete memory erasure of secret values, especially across GUI toolkit, allocator, OS clipboard and operating-system boundaries.



## Network boundary

The current implementation intentionally supports only:

~~~text
127.0.0.1:<port>
~~~

The listener is not bound to a LAN interface. There is no public listener and no automatic port opening.

The project must not silently add:

- 0.0.0.0 listening,
- UPnP or NAT-PMP port mapping,
- reverse tunnels,
- public relay dependencies,
- externally hosted JavaScript, CSS or fonts.

LAN/mobile mode is a future capability and requires an explicit security design before implementation. At minimum that design must include device pairing, persistent trusted-device identity, revocation, authorization boundaries, and safe behavior on public networks.

### Explicit LAN enable control

M3 begins with a process-local LanEnableControl in src/lan_access_control.rs. It is an operator-intent gate, not a network-listener authorization.

Every application process starts with the control disabled. Only an explicit action in the native MOBILE view can enable it, and the same view can disable it again. The state is intentionally not written to `workspace/state/session.v1`, Workspace `config/`, generic backups, or another durable store. Restarting DANGOMUSHI therefore returns the control to disabled, and restoring an older Workspace snapshot cannot silently re-enable it.

Enabling the control does not modify the current gateway. src/server.rs remains bound to `127.0.0.1`, retains its loopback peer check and Host restriction, and exposes no new state-changing HTTP route. A future LAN listener must treat this explicit control as necessary operator intent but still remain blocked until the later M3 device-pairing, trusted-device, revocation, permission and public-network safety boundaries are implemented and tested.

### Device pairing

The M3 device-pairing core is implemented in src/device_pairing.rs and remains process-local. A pairing session can start only while LanEnableControl is enabled. Turning LAN control off clears the native run-local pairing state.

A session creates two independent values from the operating-system CSPRNG: a 16-character base32-style human code carrying 80 bits of entropy and a 32-byte challenge. The session lasts at most five minutes and allows at most five failed cryptographic proofs.

The raw human code is not designed to be a future LAN request field. The client derives an HMAC-SHA256 proof over a domain-separated transcript containing the random challenge, the claimed Ed25519 public key and the exact UTF-8 device name. The device also signs the proof-bound transcript with the corresponding Ed25519 private key. The core verifies HMAC and then uses ed25519-dalek strict verification so a successful submission proves both knowledge of the short-lived code and possession of the submitted private key.

Successful cryptographic proof does not itself create durable trust. The native owner must explicitly approve the candidate, after which the core returns a process-local PairedDeviceCandidate containing the device name, Ed25519 public key and SHA-256 fingerprint. Writing that public key under Workspace identity storage belongs to the following Persistent trusted-device public keys task.

Pairing state, pairing codes, pending candidates and run-local approval are not written to session.v1, config/, generic backups, Secret Backup / Restore, or the embedded web gateway. No non-loopback listener or pairing HTTP route is added in this task.

## Web gateway

The embedded gateway currently serves only fixed assets and a read-only status endpoint.

Security properties:

- loopback peer check,
- Host header restriction,
- GET/HEAD only,
- fixed route table,
- no arbitrary filesystem path mapping,
- no CORS opt-in,
- Content Security Policy,
- no external assets,
- no state-changing HTTP API.

This deliberately avoids turning localhost into a privileged unauthenticated command API.

## Portability

The Rust core is intended to compile separately for Windows, macOS and Linux while sharing the same workspace format.

Executable files are platform-specific. Workspace data should remain platform-neutral wherever practical.

The first target is Windows because the USB workflow is being developed there. Cross-platform behavior remains an architectural requirement rather than an assumption that one binary can run on every OS.

## UI boundary

The native desktop UI uses eframe/egui behind the Cargo desktop feature. The choice is recorded in UI_LAYER_DECISION.md.

The UI uses a compact, square-edged, pixel-oriented visual system rather than a browser-like SaaS dashboard.

The embedded web UI remains a separate client surface for local status and future authenticated mobile access. It is not used to grant the desktop application filesystem authority.

## Native UI system

The desktop shell uses a reusable pixel-oriented visual system in src/native_ui_theme.rs.

The system defines:

- a shared dark palette and semantic OK/WARN/BAD colors,
- square zero-radius widget states,
- one-pixel panel borders,
- shared spacing tokens,
- reusable panel, metric, status-badge and pixel-button components,
- original 5x5 monochrome icons drawn directly through egui's painter.

The icon set is generated from source-code bit patterns. No external icon pack, CDN, image asset, or font file is required for these icons.

The native navigation, Tool Launcher and FILES entry types use the shared icon vocabulary so later tools can extend the same visual language without duplicating screen-specific styling.


## Dependency policy

The TEXT / JSON utility uses serde_json 1.0.154 for complete JSON syntax validation and root-type classification. Formatting remains DANGOMUSHI-owned whitespace transformation after validation so the tool does not silently rewrite JSON token content.

The HASH / VERIFY core uses RustCrypto sha2 0.11.1 with default features disabled for SHA-256. The file/path boundary and streaming I/O remain in DANGOMUSHI; the cryptographic hash primitive is not reimplemented locally.

The core CLI, workspace model, filesystem browser, text editor storage layer and loopback server remain standard-library based.

The Password Generator Core introduces `getrandom` as a small unconditional dependency for production access to the operating system's preferred cryptographically secure random source. Generator logic itself remains independent from UI and storage through an injectable `RandomSource` boundary.

Encrypted Vault storage adds RustCrypto `chacha20poly1305` for XChaCha20Poly1305 AEAD and `zeroize` for best-effort clearing of directly owned Vault key/plaintext buffers. This does not change the project's explicit limitation that allocator, GUI, OS, crash, clipboard, or other copies cannot be guaranteed erased. RustCrypto `argon2` 0.6 is used by the Vault unlock layer with the `alloc` and `zeroize` features for Argon2id raw-key derivation. New Vaults use the current portable baseline of 19 MiB memory, 2 iterations, and parallelism 1; per-Vault KDF metadata remains bounded and authenticated.

The native desktop shell introduces eframe/egui as an optional feature-gated dependency. Building without the desktop feature does not require the desktop stack.

Clipboard hygiene uses optional `arboard` 3.7.0 only with the desktop feature. Image clipboard support is disabled; the Linux `wayland-data-control` feature is enabled in addition to the default X11/XWayland backend path. The OS clipboard adapter is kept behind the injectable `ClipboardBackend` boundary so core tests do not access the real clipboard.

Third-party crates may be introduced when they provide clear value, but offline rebuildability, maintenance status, platform support and future vendoring must be considered before they become foundational dependencies. Security-sensitive dependencies require an implementation-time review from official documentation.
