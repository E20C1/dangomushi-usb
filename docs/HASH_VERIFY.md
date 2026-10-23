# HASH / VERIFY Core

Status: **SHA-256 / integrity verification core implemented. Native UI integration remains separate and is not READY yet.**

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

## UI boundary

HASH / VERIFY remains PLANNED in the Tool Registry during this core task. No native UI activation path is added here.

The repository-wide Exactly One Next Task is:

**HASH / VERIFY native UI integration**
