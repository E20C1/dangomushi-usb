# Workspace Backup and Restore

Status: **M2 generic Workspace Backup / Restore implemented and READY in the native Tool Launcher.**

This feature is separate from M2-S Secret Backup / Restore.

## Scope

The generic Workspace snapshot includes:

~~~text
workspace/data/
workspace/repositories/
workspace/identity/
workspace/config/
workspace/state/autosave/
~~~

It deliberately excludes:

~~~text
workspace/backups/
workspace/state/session.v1
workspace/state/session.v1.previous
~~~

The encrypted Vault and other M2-S secret-storage paths are not part of this generic snapshot flow.

The optional state/autosave root is represented in every snapshot. If it did not exist when the snapshot was created, restoring that snapshot produces an empty state/autosave directory.

## Archive format

Workspace snapshots use the versioned binary container magic:

~~~text
DGMWBK01
~~~

The first version is intentionally simple and streaming-friendly.

Each archive contains:

- directory records,
- file records,
- UTF-8 path components encoded individually,
- exact file lengths,
- raw file bytes,
- one SHA-256 digest per file,
- an archive terminator,
- a final SHA-256 over the complete hashed archive region.

The footer uses:

~~~text
DGMEND01
~~~

The archive digest detects accidental corruption of paths, metadata records, file contents and per-file digests.

This is integrity checking, not authenticity. An attacker who can replace a backup can also construct a new valid SHA-256 archive.

## Path and filesystem boundary

The archive never stores raw absolute paths.

Every path is represented as bounded relative UTF-8 components. Components:

- must be non-empty,
- may not be . or ..,
- may not contain / or backslash separators,
- must remain within one of the managed snapshot roots,
- are limited to 256 components,
- are limited to 1024 UTF-8 bytes per component.

Backup creation rejects symlinks and non-file/non-directory entries anywhere under a managed root rather than silently omitting them.

This first version also rejects non-UTF-8 source path components.

A repository containing a working-tree symlink therefore causes generic Workspace backup creation to fail. This is deliberate: a successful backup must not silently omit an entry.

## Resource bounds

The first archive version is bounded to:

- at most 250,000 directory/file records,
- at most 512 GiB for one file,
- at most 2 TiB total regular-file payload,
- 64 KiB streaming I/O buffers,
- at most 512 final snapshots shown in the native backup list.

File contents are streamed and are not loaded wholesale into memory.

## Backup creation

CREATE BACKUP automatically chooses a unique final file under:

~~~text
workspace/backups/
~~~

The process is:

~~~text
scan fixed managed roots
  ->
create hidden pending archive with create_new
  ->
stream records + file hashes + archive hash
  ->
sync pending archive
  ->
fully verify pending archive
  ->
copy into create_new final archive
  ->
sync final archive
  ->
fully verify final archive
  ->
remove pending archive
  ->
sync backups directory
~~~

Final backup files use the .dgmwbk extension.

A failed or interrupted pending archive is not shown as a usable final snapshot.

If the Editor currently contains dirty text, the native UI first writes the existing generic recovery draft into state/autosave so that draft becomes part of the snapshot.

Backup creation is not a filesystem-wide atomic snapshot. DANGOMUSHI attempts to detect a source file whose length or modification metadata changes while that file is being read, but it cannot freeze the complete Workspace across all files. Concurrent external modifications can therefore prevent a backup or, in races below filesystem timestamp resolution, produce a cross-file state that did not exist at one single instant.

## Verification

VERIFY BACKUP is read-only.

It checks before reporting success:

- format magic,
- path bounds and managed-root membership,
- entry count bounds,
- directory ordering/parent structure,
- duplicate paths,
- file size/total-size bounds,
- every file SHA-256,
- presence of every managed root record,
- complete archive SHA-256,
- absence of trailing data.

Verification never restores or rewrites Workspace data.

## Restore staging

Restore is destructive and requires the user to type:

~~~text
RESTORE
~~~

The native UI also refuses restore while the currently open Editor has dirty content or an unresolved pending recovery choice.

Before any managed Workspace root is replaced, the selected archive is completely extracted into a hidden staging directory below workspace/backups and all file/archive integrity checks must succeed.

Current managed roots are then moved into a hidden rollback directory and staged roots are moved into their final Workspace locations.

The replacement units are:

~~~text
data/
repositories/
identity/
config/
state/autosave/
~~~

backups/ and state/session.v1 are never replaced by generic restore.

If an in-process replacement step fails, already replaced roots are processed in reverse order and DANGOMUSHI attempts to restore the prior roots from rollback staging.

After a successful restore, the native UI resets filesystem-derived Editor, FILES, HASH / VERIFY, TEXT / JSON, COMPARE, SEARCH and REPOSITORIES state so stale in-memory file state is not later written over restored data.

## Restore crash boundary

The multi-root replacement is not one operating-system atomic transaction.

A process crash, OS crash, device removal or power loss during the rename sequence can leave hidden:

~~~text
.restore-staging-...
.restore-rollback-...
~~~

artifacts under workspace/backups and may leave the managed roots in an ambiguous partially restored state.

A later restore refuses to begin while such artifacts exist. DANGOMUSHI does not guess which side is authoritative.

This first M2 implementation therefore provides verified staging and in-process rollback, but does not claim crash-atomic multi-root restore.

## Session boundary

The native session stores only that the BACKUP view was active.

It does not persist:

- selected backup,
- verification summary,
- restore confirmation text,
- restore result,
- backup path.

state/session.v1 itself is intentionally outside generic snapshot restore, so the normal session writer remains authoritative for current navigation state.

## Secret boundary

Generic Workspace backups are not a password/Vault export feature.

Passwords, Master Passphrases, Recovery Codes, decrypted Vault payloads and other secret-subsystem plaintext must not be intentionally routed into ordinary Workspace files merely to make them appear in a generic backup.

M2-S Secret Backup / Restore remains the only implemented backup path for encrypted Vault ciphertext.

## Exactly One Next Task

M2 Local Tools are complete through generic Backup / Restore.

The repository-wide Exactly One Next Task is:

**Explicit LAN enable control**

This M3 task must not introduce a LAN listener yet. The existing rule remains: no LAN listener may be merged before pairing, persistent trusted-device identity, revocation and permission boundaries exist.
