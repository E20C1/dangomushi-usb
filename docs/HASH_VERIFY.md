# HASH / VERIFY Core

Status: **SHA-256 / integrity verification core and native UI integration implemented; HASH / VERIFY is READY.**

## Scope

The core provides two read-only operations:

~~~text
hash_workspace_file(workspace_root, relative_path)
verify_workspace_file(workspace_root, relative_path, expected_sha256)
~~~

Both operate only on an existing regular file below the configured Workspace root.

The target path must be non-empty, relative, and composed only of normal path components. Symlink path components are rejected. The resolved target must remain below the canonical Workspace root and must be a regular file.

The implementation does not create, modify, rename, delete, autosave, or otherwise persist user files.

## SHA-256 calculation

Files are read incrementally in 64 KiB chunks and fed to RustCrypto SHA-256. This avoids loading an entire large file into memory merely to calculate its digest.

The result includes the 32-byte SHA-256 digest, canonical lowercase 64-character hexadecimal formatting, and the number of bytes actually hashed.

## Verification

The expected digest is parsed before the file is hashed.

Accepted input is exactly 64 hexadecimal characters after trimming surrounding whitespace. Uppercase and lowercase hexadecimal are both accepted. Checksum-file syntax, filenames appended to a digest, multiple algorithms, and remote hashes are intentionally outside this core task.

Verification returns the calculated digest and a boolean match result. A mismatch is a normal verification result, not an I/O error.

## Security meaning

SHA-256 can detect whether file bytes match a trusted expected digest. It does not establish who produced the file.

If an attacker can replace both a file and the expected digest, a successful comparison does not authenticate the file. Signature verification, trust distribution, downloaded manifests, and repository verification are separate concerns.

## Filesystem race boundary

Path components are checked for symlinks and the canonical target is checked against the canonical Workspace root before the file is opened. As with the existing portable filesystem APIs, this does not claim an OS-level atomic no-follow guarantee against a path being concurrently replaced between checks and open.

## Native UI

HASH / VERIFY is READY in the Tool Registry and is launched through explicit Rust dispatch into a dedicated native view.

The native view supports:

- a manually entered Workspace-relative target path,
- adopting the currently selected regular file from FILES,
- SHA-256 calculation without an expected digest,
- verification against an expected 64-hex SHA-256 value,
- visible calculated digest, byte count, MATCH / MISMATCH state, and bounded operation errors.

The FILES screen also exposes a HASH / VERIFY action for selected regular files. Symlinks, directories, absolute paths, traversal and out-of-workspace paths remain rejected by the core even if a UI path is manipulated.

HASH / VERIFY input, expected digest, calculated digest and verification result are intentionally not added to the generic session payload. Session restoration remembers only that the HASH / VERIFY native view was active.

No clipboard copy action, remote checksum lookup, checksum-file parser, signature verification, multi-algorithm selection, or background hashing is introduced by this task.

The repository-wide Exactly One Next Task is:

**Text / JSON utilities**
