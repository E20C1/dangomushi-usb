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

### Persistent trusted-device public keys

Approved run-local pairing candidates can be promoted through a second explicit native action into a versioned trust store at workspace/identity/trusted_devices.v1.json. The durable record contains only the device display name, Ed25519 public key and its domain-separated SHA-256 fingerprint. Pairing codes, HMAC proofs, signatures, private keys and session state are never written to this store.

The store is bounded to 256 devices and 256 KiB, validates UTF-8 device-name rules, exact 32-byte public keys, recomputed fingerprints and duplicate keys on every load, and rejects symlink or special-file storage layouts. Replacement uses create-new pending output, full parse verification, a previous-file rollback slot, fsync of file content and best-effort directory synchronization. Interrupted-write recovery prefers the last verified committed store and never promotes a pending-only first write.

Because identity/ is already a managed Generic Workspace Backup root, trusted public keys are intentionally included in generic snapshots and can be restored with them. Device revocation below adds a monotonic reconciliation rule so restoring an older trust set cannot remove an already-known local revocation.

The trust store does not defend against an attacker who already has arbitrary write access to the Workspace filesystem. This task establishes bounded durable public-key identity, not a local tamper-proof hardware root.

### Device revocation

The M3 revocation authority is implemented in src/device_revocation.rs as a versioned tombstone ledger at workspace/identity/device_revocations.v1.json. Each tombstone is the domain-separated SHA-256 fingerprint of a revoked Ed25519 public key. The ledger is sorted, duplicate-free, bounded to 4,096 fingerprints and 512 KiB, and uses create-new pending output, verified replacement, previous-file rollback, file sync and best-effort directory synchronization.

trusted_devices::load is an effective-trust read: both the trusted-device store and revocation ledger must verify successfully, and any trusted record whose fingerprint is tombstoned is filtered out. If revocation state is malformed or unsafe, trusted-device loading fails closed. register_candidate also checks the ledger, so the same public key cannot be trusted again after revocation. Re-adding the same physical device requires key rotation, a new pairing proof and a new explicit persistent-trust action.

Revocation is an explicit native owner action. Selecting REVOKE DEVICE does not itself mutate durable state; the operator must type the exact confirmation word REVOKE before the tombstone is committed. There is no un-revoke operation in this milestone.

Because the ledger lives under identity/, Generic Workspace Backup carries revocation state without changing the DGMWBK01 archive format. Before a verified staged restore replaces any managed root, workspace_backup calls the revocation layer to parse the live and staged ledgers, form their set union and write that union into staged identity. Therefore an older backup can restore a historical trusted-device record but cannot remove a fingerprint that has already been revoked locally. A backup created after revocation also carries that tombstone to another initialized Workspace. If either live or staged revocation state cannot be verified, restore aborts before managed-root replacement.

A backup created before a later revocation cannot contain that future fact. If both the live Workspace and every post-revocation backup are lost, there is no external revocation authority from which DANGOMUSHI can reconstruct it. The local Workspace filesystem also remains outside a hardware tamper-proof trust model.

Permission scopes are defined separately below and cannot bypass revocation. No reconnect/session policy, non-loopback listener or network authorization is added by the revocation task.

### Permission scopes

The M3 authorization policy is implemented in `src/device_permissions.rs` and stored at `workspace/state/device_permissions.v1.json`. The permission store is deliberately separate from durable public-key identity: being trusted does not itself authorize any future LAN operation.

The initial vocabulary is fixed to four canonical scope identifiers: `status.read`, `files.read`, `files.write`, and `session.handoff`. A newly trusted device starts at default deny with no scopes. Scope identifiers are necessary authorization labels only; they do not create endpoints, broaden Workspace path validation, expose secret-subsystem plaintext, allow arbitrary commands or Git mutation, or override any later endpoint-specific security check.

`device_permissions::authorize` first resolves the effective trusted-device set, which already applies revocation. An untrusted or revoked fingerprint is denied even when a stale permission record exists. If trusted-device state or permission state cannot be verified, callers do not receive an allow result.

The native MOBILE view exposes a separate DEVICE PERMISSIONS editor for each effective trusted device. Checkbox changes remain ephemeral UI draft state until the owner presses SAVE PERMISSIONS. Saving an empty set removes the record and returns the device to default deny.

Permission policy is durable across normal application restarts because it remains on the portable Workspace. Generic Workspace Backup intentionally excludes the policy by managing only `state/autosave/`, not the complete `state/` root. Consequently, restoring an older DGMWBK01 snapshot cannot re-grant a scope that was removed locally, and restoring a snapshot into another initialized Workspace does not import permission grants.

A historical restore can remove a trusted-device record while leaving the local permission file untouched. To prevent that stale policy from becoming active after a later explicit re-trust of the same non-revoked public key, `trusted_devices::register_candidate` clears the fingerprint's permission record before committing new trust. The newly trusted key starts without stale authorization. Revoked keys still require key rotation because the revocation layer rejects same-key re-trust entirely.

The permission store is versioned, sorted, duplicate-free, bounded to 256 device records and 128 KiB, and accepts only the fixed canonical scope identifiers. The `state/` root and fixed permission files reject symlink and special-file layouts. Replacement uses create-new pending output, full parse verification, a previous-file rollback slot, file sync and best-effort directory synchronization.

Permission scopes remain transport-independent authorization labels. They do not themselves create a reconnect session, public-network policy, non-loopback listener, mobile editor endpoint, or network request transport.

### Reconnection without short fixed session expiry

The M3 reconnect boundary is implemented in `src/device_reconnection.rs`. It deliberately avoids both repeatedly asking for the five-minute human pairing code and introducing a durable bearer credential.

A reconnect can begin only while explicit LAN intent is enabled and only for an effectively trusted, non-revoked fingerprint. DANGOMUSHI generates a fresh 256-bit challenge from the operating-system CSPRNG for each reconnect attempt. The device signs a domain-separated transcript that binds the reconnect protocol, the fresh challenge and the trusted fingerprint. Completion consumes the challenge object, re-reads effective trust and uses Ed25519 strict signature verification.

The reconnect challenge has a two-minute anti-staleness lifetime. That is only a handshake lifetime. A successful proof creates an in-process `AuthenticatedDeviceSession` containing the fingerprint and display name with no fixed expiry field and no access token, refresh token, cookie secret, shared secret or persisted session identifier. A future transport owns this principal only for its live connection/process lifetime.

Closing the transport discards the principal. Reconnecting later requires a fresh challenge and a new Ed25519 proof, but does not require a new pairing code while the same public key remains trusted. There is no indefinitely reusable reconnect bearer token.

`AuthenticatedDeviceSession::is_currently_valid` requires LAN intent to remain enabled and re-reads effective trusted-device state. `AuthenticatedDeviceSession::authorize` then consults the current default-deny permission store. Scopes are not snapshotted into the authenticated principal. Revocation or permission removal therefore takes effect without waiting for a clock-based session expiry, and an unreadable trust or permission state never produces an allow result.

Nothing from reconnection is written to `session.v1`, Workspace config, Generic Workspace Backup, Secret Backup / Restore or another durable store. The reconnect core is transport-free and does not add a LAN listener, HTTP route, socket binding, network framing, cookie policy or public-network classification.

### Public-network safe default

The M3 network-exposure gate is implemented in `src/network_safety.rs`. Explicit LAN control remains operator intent only; it is not sufficient authority for any future non-loopback listener.

A LAN exposure is eligible only when LAN intent is enabled for this run and a fresh current-network assessment is trusted. `Unknown` and `Public` always deny exposure. Probe failure, zero connected networks, unsupported platform and unexpected category values are also deny results.

On Windows, DANGOMUSHI uses the operating system's Network List Manager through the `windows` crate. The probe runs on a fresh COM thread, enumerates all connected networks, and reads `INetwork::GetCategory` only. It never calls `SetCategory`, never shells out to PowerShell, and never infers firewall openings from the profile.

The aggregate rule is conservative across all connected networks: any Public category makes the whole assessment Public; otherwise any Unknown makes it Unknown; only a non-empty set containing exclusively Private and/or DomainAuthenticated categories is Trusted. This avoids treating one trusted interface as permission to ignore a simultaneously connected untrusted interface.

macOS and Linux intentionally return UnsupportedPlatform / Unknown in this milestone. The project does not invent a trusted/public semantic where no reviewed equivalent platform source has been established. Consequently, future LAN exposure remains fail-closed on those platforms while loopback behavior remains available.

The native MOBILE view displays a snapshot for operator visibility, but a snapshot is never a durable capability. A future non-loopback transport must obtain a fresh assessment immediately before binding and again before accepting or authorizing each remote connection. A cached earlier Trusted result cannot authorize later work after a network transition. If a live re-check becomes Public, Unknown or fails, new remote work must be blocked.

Network-safety state is process-local evidence only. It is not written to `session.v1`, Workspace config, Generic Workspace Backup, Secret Backup / Restore, trusted-device identity, revocation state or permission policy.

This task does not create a LAN listener. `src/server.rs` remains bound to `127.0.0.1` and retains the existing loopback peer/Host restrictions. No wildcard bind, firewall mutation, UPnP/NAT-PMP mapping, public relay, TLS/cookie transport or mobile handoff endpoint is introduced.

### Mobile editor/session handoff

The M3 mobile handoff core is implemented in `src/mobile_handoff.rs` and remains transport-independent. It does not create a LAN listener, HTTP/WebSocket route, mobile web UI, browser key store or alternate persistence path.

A session snapshot requires the current authenticated device to hold `session.handoff`. The snapshot contains only the existing navigation fields from the in-process `SessionState`: active view, current FILES directory, selected FILES entry and current Editor path. Relative paths are validated again before handoff. Editor buffers, autosave drafts, secret-subsystem plaintext, authentication material and permission state are not part of the session snapshot.

Opening a mobile Editor document requires both `session.handoff` and `files.read`. The core delegates the read to the existing Text Editor boundary, so relative-path validation, symlink rejection, in-Workspace confinement, regular-file enforcement, UTF-8 decoding and the 4 MiB limit remain authoritative. A successful read returns a SHA-256 content revision together with the UTF-8 text. The revision is only a concurrency token, not a bearer credential or durable capability.

Saving a mobile Editor document requires both `session.handoff` and `files.write`. The caller must return the revision of the version it edited. The core re-reads the current disk text before save. If the revision changed, DANGOMUSHI refuses to overwrite the newer disk file and stores the submitted mobile text through the existing recovery-draft boundary. If the revision still matches, saving delegates to the existing Text Editor save path, which persists recovery before touching the source file and syncs the explicit source write.

The comparison is deliberately aligned with the current desktop Editor conflict model. It does not claim an operating-system-atomic compare-and-swap against another process changing the file in the narrow interval between revision checking and final write.

Every handoff capture, read and save reuses `AuthenticatedDeviceSession::authorize`; permissions are never snapshotted into a handoff. LAN disable, trust removal, revocation or permission removal therefore affects later operations immediately.

Handoff snapshots, Editor text, content revisions and authenticated principals remain process-local. They are not added to `session.v1`, Workspace config, Generic Workspace Backup, Secret Backup / Restore, trusted-device identity, revocation state or permission policy.

This completes M3's transport-independent mobile security and handoff boundaries. Any future non-loopback transport must still perform the live network-safety checks defined above and must not weaken the trust, reconnect or permission checks when connecting this core to a remote client.

## Trusted peer identity

M4 begins with a separate DANGOMUSHI-to-DANGOMUSHI peer identity authority implemented by `src/peer_identity.rs` and `src/trusted_peers.rs`. It does not reuse M3 trusted-device records or M3 permission scopes.

Local peer identity is an Ed25519 key generated from the operating-system CSPRNG and explicitly initialized once at `workspace/state/peer_identity.v1`. The fixed record stores the private seed, derived public key and a domain-separated SHA-256 peer fingerprint. Loads recompute both public key and fingerprint from the private seed and fail closed on mismatch. Initialization never silently rotates an existing identity; a verified pending first-write can be recovered, while ambiguous layouts are rejected.

The private peer identity deliberately lives under `state/`, not the Generic Workspace Backup managed `identity/` root. Generic Backup manages only `state/autosave/`, so it neither copies nor replaces the peer private key. The current storage is not encrypted at rest; arbitrary read access to Workspace state can recover the peer private key and impersonate that peer.

Peer proof-of-possession uses a fresh 256-bit CSPRNG challenge with a five-minute bound and strict Ed25519 verification over a domain-separated transcript containing the challenge, public key and validated peer display name. Successful proof produces a typed `VerifiedPeerCandidate`; network address, hostname, display name or a bare public key never creates trusted status.

Persistent trusted-peer policy is an explicit second step stored at `workspace/state/trusted_peers.v1.json`. Records contain only peer name, public key and the separate peer fingerprint. The bounded versioned store verifies fingerprints, rejects duplicate keys, rejects unsafe file layouts and uses verified pending/previous replacement. Trust is not TOFU.

Removing a peer deletes it from the current trusted-peer policy. Because that policy is outside Generic Workspace Backup, restoring an older generic snapshot cannot silently resurrect removed peer trust. Re-trusting the same key requires a new proof-of-possession flow and another explicit trust action.

M3 device fingerprints and M4 peer fingerprints use different domain-separation strings and separate stores. Trust in one domain grants nothing in the other.

This task remains transport-independent. `src/server.rs` is still loopback-only and no peer discovery or peer socket is introduced.

## Encrypted peer transfer

M4 encrypted text/file transfer is implemented by `src/peer_transfer.rs` as a transport-independent one-shot envelope protocol layered on the trusted-peer identity authority.

The receiver creates a fresh OS-CSPRNG transfer ID and ephemeral X25519 key pair, then signs a public offer with its existing Ed25519 peer identity. The signed offer binds the transfer ID, receiver peer fingerprint, expected sender peer fingerprint and receiver ephemeral X25519 public key. The sender verifies that the receiver remains trusted and verifies the Ed25519 offer signature before proceeding.

The sender creates its own fresh ephemeral X25519 key pair, rejects non-contributory Diffie-Hellman output, and derives a 256-bit transfer key with HKDF-SHA256. HKDF uses the transfer ID as salt and a versioned context containing both peer fingerprints and both ephemeral X25519 public keys.

The payload is encrypted with XChaCha20Poly1305 using a fresh 192-bit OS-CSPRNG nonce. The fixed versioned envelope header is associated data. The sender additionally authenticates the envelope with its Ed25519 peer identity by signing a domain-separated SHA-256 digest of header plus ciphertext. The receiver verifies that signature against the current trusted sender key before decryption.

Ed25519 remains the long-lived authentication identity. It is not converted into a static X25519 key. Both X25519 secrets are transfer-ephemeral.

A pending receive is process-local, expires after five minutes, and is consumed by value on one open attempt. Replays do not match a newly generated transfer ID/receiver ephemeral key. Offer, envelope and inner payload framing use strict version and exact-length checks with bounded text/file sizes.

Transferred text is bounded to 4 MiB UTF-8. Transferred files are bounded to 64 MiB. A file carries only a validated single-component suggested name plus bytes. The core never chooses a destination path and never writes a received file.

XChaCha20Poly1305 authenticates the encrypted envelope for the derived transfer key, while the Ed25519 signature authenticates the sender peer. This is transport-envelope integrity, not the next task's durable received-file integrity record.

Transfer IDs, ephemeral X25519 secrets, derived keys, plaintext/ciphertext transfer content and pending receive state remain ephemeral and are not added to session state, autosave, Generic Backup, Secret Backup, trusted-peer policy or M3 device state.

`src/server.rs` remains loopback-only. Direct LAN transport is still a later M4 task.

## Received file integrity

M4 received-file integrity is implemented by `src/received_file_integrity.rs` together with file-payload framing in `src/peer_transfer.rs`.

For file transfers, the sender computes SHA-256 over the raw file bytes before encryption. Payload format v2 carries the declared file byte length, a fixed SHA-256 algorithm identifier, and the 32-byte digest alongside the validated suggested filename and file bytes. This integrity record is inside the existing XChaCha20Poly1305 ciphertext and therefore is also covered by the sender-authenticated encrypted envelope.

After envelope signature verification and AEAD decryption, a file is returned as `PendingReceivedFile`, not as directly accessible bytes. The pending type exposes only the suggested filename, authenticated declared length, expected SHA-256 and trusted sender fingerprint. Its byte buffer remains private.

`PendingReceivedFile::verify(self)` consumes the pending file, first compares the actual byte length with the authenticated declared length, then recomputes SHA-256 over the received bytes and compares it with the authenticated expected digest. Mismatch fails closed. Only successful verification produces `VerifiedReceivedFile`, which exposes the file bytes plus verified length, digest, sender fingerprint and suggested filename.

This is intentionally separate from transport-envelope integrity. XChaCha20Poly1305 and the sender Ed25519 signature protect the encrypted transfer envelope; the file SHA-256 verifies the exact decrypted bytes against the trusted sender's authenticated commitment. A malicious trusted sender can still provide malicious bytes with a matching digest, so this boundary is not malware detection or content-safety approval.

No filesystem destination is selected and no received file is written by this task. A future durable receiver must verify the bytes it actually makes durable against the verified digest if any write path is introduced.

Received file bytes and verification state remain ephemeral and are not added to session state, autosave, Generic Workspace Backup, Secret Backup, trusted-peer policy or M3 device state.

`src/server.rs` remains loopback-only.

## Direct LAN peer transport

M4 direct LAN transport is implemented in `src/peer_lan.rs`. It is the first non-loopback peer path, but it remains deliberately narrower than a general network service.

The transport is direct TCP over IPv4 only. It has no discovery, multicast, broadcast, firewall mutation, UPnP/NAT-PMP, relay, rendezvous, NAT traversal or global-address support.

Production bind/connect operations require the process-local `LanEnableControl` to be enabled and call the live `network_safety::current_exposure_decision` rather than accepting a cached assessment. On Windows, all connected Network List Manager entries must therefore still be Private and/or DomainAuthenticated. Public, Unknown, probe-failure or empty evidence denies exposure. macOS/Linux remain fail-closed while their network-safety adapter is UnsupportedPlatform.

The receiver uses a one-shot nonblocking listener with a maximum five-minute receive window. It re-probes network safety immediately before bind, every 50 ms while waiting, immediately after accept, before issuing the signed receive offer, before authenticating/opening the encrypted payload, and before the final success acknowledgement. After the first accepted connection the listener is dropped before that connection is processed.

The sender re-probes before connect, immediately after connect and again before sealing/sending the encrypted envelope.

The first transport accepts only IPv4 loopback, RFC1918 private and IPv4 link-local addresses. Global IPv4, CGNAT and IPv6 targets are rejected by the transport API. Loopback remains permitted for same-host use/testing but does not bypass the public API's explicit LAN-intent and live-safety checks.

Wire framing is a small bounded layer around the existing M4 encrypted-transfer protocol. The sender first identifies its trusted-peer fingerprint. The receiver treats that fingerprint only as a selector, re-reads trusted-peer policy, creates the existing signed one-shot offer bound to that expected sender, and returns the offer. The sender uses `peer_transfer::seal_transfer_to`, which additionally requires the signed offer's recipient fingerprint to match the caller's explicitly expected recipient fingerprint. This prevents a target-address mix-up from silently redirecting a transfer to another trusted peer.

All text/file content remains inside the existing ephemeral X25519 + HKDF-SHA256 + XChaCha20Poly1305 + Ed25519 envelope. The receiver performs another live network check before `peer_transfer::open_transfer`.

File transfers are not acknowledged until `PendingReceivedFile::verify` succeeds and produces `VerifiedReceivedFile`. No filesystem destination is selected and no received file is written.

TCP connect timeout is five seconds; connected read/write operations are bounded to ten seconds. Length-prefixed frames are rejected before allocation when they exceed the existing encrypted-transfer maximum.

The default peer LAN port is 45822. The listener binds IPv4 wildcard only after the live safety gate succeeds. DANGOMUSHI does not modify OS firewall policy.

Direct LAN transport state is process-local and is not persisted into session state, autosave, Generic Backup, Secret Backup, trusted-peer policy or M3 device state.

The local HTTP gateway in `src/server.rs` remains independently loopback-only on `127.0.0.1`.

Internet P2P remains disabled after a separate completed threat review.

## Internet P2P threat-review decision

The M4 Internet P2P threat review is recorded in `docs/INTERNET_P2P_THREAT_REVIEW.md` and codified by `src/internet_p2p_policy.rs`.

The decision is **DEFER / DISABLE Internet P2P** for the current roadmap. The review does not authorize a public listener, global peer address, rendezvous/discovery service, relay, NAT traversal, automatic router/firewall mutation, or Internet peer transport.

The review treats Internet P2P as a separate security and operations problem rather than an address-range extension of Direct LAN. New risks include unsolicited scanning, public endpoint exposure, denial of service, metadata/traffic correlation, NAT/interface ambiguity, rendezvous/relay trust, abuse handling, relay cost/availability, identity-key compromise from anywhere, protocol downgrade and central-service dependency.

The repository-level policy is fail-closed: `internet_p2p_policy::internet_p2p_enabled()` always returns false, there is no runtime/configuration override, Direct LAN continues to reject global/CGNAT/IPv6 targets, and the local HTTP gateway remains loopback-only.

M4 therefore closes with Direct LAN as its supported peer transport. A future Internet P2P implementation must return as a separately scoped milestone only after the threat-review prerequisites are satisfied and reviewed.

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


## Offline documentation packs

M5 offline continuity begins with a compile-time embedded documentation pack implemented in `src/offline_docs.rs`.

The pack contains `README.md` plus every Markdown document under `docs/`. Pack entries use fixed relative-normal paths and are capped at 4 MiB per document. The runtime does not discover arbitrary filesystem documentation and does not fetch documentation from GitHub, a CDN, DNS, HTTP, Git or another external service.

A deterministic pack SHA-256 is calculated over a domain-separated sequence containing the pack format, each path, each document length and each document SHA-256. This identifies the exact embedded documentation set but is not a publisher signature or executable-authenticity mechanism.

The CLI can list and show embedded documents without filesystem or network access. Export is explicit and create-new only: an existing destination is never overwritten. Export writes the fixed files, syncs each created file, and writes a `.dangomushi-offline-docs` marker containing pack format, ID, application version, document count, total bytes and pack digest.

Export verification compares the destination exactly against the current executable's embedded pack. The root, marker and documents must be real non-symlink files/directories; missing, modified, extra, truncated, special or symlink entries fail closed.

The native SYSTEM surface exposes the embedded pack through a read-only selector/viewer. Selection is ephemeral and is not added to `session.v1`.

Embedded documentation is application content, not Workspace state. DANGOMUSHI does not automatically copy it into `workspace/data`, autosave, Generic Workspace Backup, Secret Backup or trusted-device/peer state. An explicitly exported directory becomes ordinary user-controlled filesystem content after export.

This task does not vendor Rust dependencies, create recovery snapshots, verify recovery restores, or construct portable operating-system release bundles. Those remain later M5 boundaries.

## Reproducible dependency cache

M5 dependency continuity uses Cargo's native lock/vendor model instead of defining a DANGOMUSHI-specific package cache format.

`Cargo.lock` is committed and is the authoritative Rust dependency resolution. Normal CI plus the Windows/Unix application update paths pass `--locked`, so routine builds fail rather than silently rewriting dependency versions.

The reviewed rebuild baseline is pinned by `rust-toolchain.toml` to Rust 1.99.1 with the minimal rustup profile. CI installs the same exact toolchain instead of a moving stable channel. This pin is the rebuild baseline; it is separate from any lower MSRV declaration in `Cargo.toml`.

The generated Rust source cache lives at repository-root `vendor/`. `scripts/vendor-dependencies.sh` and `scripts/vendor-dependencies.cmd` require the lockfile, refuse to overwrite an existing cache, run `cargo vendor --locked --versioned-dirs` into `vendor.pending/`, then promote it to `vendor/` and verify frozen metadata through the offline source replacement.

`.cargo/vendor-config.toml` is opt-in rather than the default Cargo config. When explicitly selected it replaces crates.io with `vendor/` and sets Cargo offline. This avoids stale vendor content silently affecting ordinary online development while still providing a strict offline mode.

`scripts/verify-vendored-dependencies.*` run metadata, core tests/build, desktop library tests and native desktop release build through the vendor config with `--frozen`. CI contains a separate Ubuntu offline-vendor job that generates a new cache from the committed lockfile and exercises that complete frozen path.

The generated vendor tree is ignored by Git. It is third-party build material and can be preserved on offline media alongside the matching source tree and lockfile or incorporated by a later explicit recovery/release artifact. Existing Workspace/Backup scopes are unchanged.

This is not a fully hermetic operating-system build. Rust toolchain installation, Linux native development packages, Windows/macOS SDKs, system linkers and other platform prerequisites are not cached by this task. A valid Cargo checksum also proves source consistency, not that a dependency is secure or independently audited.

The next M5 boundary is Recovery snapshots.

## Recovery snapshots

M5 recovery snapshots are implemented by `src/recovery_snapshot.rs` as a create-only, higher-level offline recovery directory.

A snapshot copies a fixed allowlist of first-party source/build inputs, the generated repository-root `vendor/` tree, and one selected Generic Workspace Backup that must pass `workspace_backup::verify_backup` before destination creation. The copied Workspace artifact is stored at `recovery/workspace.dgmwbk`.

The source allowlist is intentionally narrower than the entire repository root. It contains Cargo/build metadata plus `.github/workflows/`, `docs/`, `scripts/`, `src/`, `tests/`, `web/` and `vendor/`. It does not recursively copy `.git/`, `target/`, `build/`, `app/`, live `workspace/` or arbitrary root files.

The live Workspace is never copied directly. This preserves the existing Generic Workspace Backup scope and avoids bypassing security-sensitive exclusions such as session state, peer private identity and permission policy. M2-S Secret Backup also remains separate; recovery snapshot creation does not discover Vault storage, Master Passphrases, Credentials, Recovery Codes or Secret Backup ciphertext.

Recovery snapshot destinations are create-new, must have a real non-symlink parent, and must be outside both source and Workspace roots. Copied trees reject symlinks, special entries and non-UTF-8 components.

Files are streamed through 64 KiB buffers. Source length/modification metadata is sampled around the copy, source bytes are SHA-256 hashed while copying, destination files are synced and independently re-hashed, and the complete destination payload is re-enumerated before completion.

The completion marker `.dangomushi-recovery-snapshot` is written last. It records format/app version, fixed Workspace Backup location, file count, total bytes, sorted hex-encoded path records with length/SHA-256, and a domain-separated deterministic snapshot SHA-256. This is integrity identification, not publisher authenticity.

The CLI exposes only `recovery snapshot create`. There is deliberately no restore/extract or public verify command in this task. Restore Verification is the next M5 boundary.

A snapshot contains the Rust source/vendor materials required by the existing frozen Cargo workflow but still does not preserve the Rust toolchain installer, Linux package payloads, Windows/macOS SDKs, system linkers or platform runtime dependencies.


## Dependency policy

The TEXT / JSON utility uses serde_json 1.0.154 for complete JSON syntax validation and root-type classification. Formatting remains DANGOMUSHI-owned whitespace transformation after validation so the tool does not silently rewrite JSON token content.

The HASH / VERIFY core uses RustCrypto sha2 0.11.1 with default features disabled for SHA-256. The file/path boundary and streaming I/O remain in DANGOMUSHI; the cryptographic hash primitive is not reimplemented locally.

The core CLI, workspace model, filesystem browser, text editor storage layer and loopback server remain standard-library based.

The Password Generator Core introduces `getrandom` as a small unconditional dependency for production access to the operating system's preferred cryptographically secure random source. Generator logic itself remains independent from UI and storage through an injectable `RandomSource` boundary.

Encrypted Vault storage adds RustCrypto `chacha20poly1305` for XChaCha20Poly1305 AEAD and `zeroize` for best-effort clearing of directly owned Vault key/plaintext buffers. This does not change the project's explicit limitation that allocator, GUI, OS, crash, clipboard, or other copies cannot be guaranteed erased. RustCrypto `argon2` 0.6 is used by the Vault unlock layer with the `alloc` and `zeroize` features for Argon2id raw-key derivation. New Vaults use the current portable baseline of 19 MiB memory, 2 iterations, and parallelism 1; per-Vault KDF metadata remains bounded and authenticated.

The native desktop shell introduces eframe/egui as an optional feature-gated dependency. Building without the desktop feature does not require the desktop stack.

Clipboard hygiene uses optional `arboard` 3.7.0 only with the desktop feature. Image clipboard support is disabled; the Linux `wayland-data-control` feature is enabled in addition to the default X11/XWayland backend path. The OS clipboard adapter is kept behind the injectable `ClipboardBackend` boundary so core tests do not access the real clipboard.

Third-party crates may be introduced when they provide clear value, but offline rebuildability, maintenance status, platform support and future vendoring must be considered before they become foundational dependencies. Security-sensitive dependencies require an implementation-time review from official documentation.
