# File Comparison

Status: **M2 File comparison implemented and READY in the native Tool Launcher.**

## Scope

COMPARE performs a read-only comparison between two existing regular files below the configured Workspace root.

Both target paths must be non-empty relative Workspace paths composed only of normal path components. Symlink path components are rejected, canonical targets must remain below the canonical Workspace root, and both targets must be regular files.

The comparison never creates, modifies, renames, deletes, autosaves, or otherwise persists either input file.

## Binary-safe comparison

The core is implemented in src/file_compare.rs.

Files are compared incrementally through 64 KiB buffered reads. The complete files do not need to be loaded into memory for the authoritative equality result.

The result reports:

- EQUAL or DIFFERENT,
- File A byte length,
- File B byte length,
- the first differing byte offset using zero-based indexing.

If one file is an exact prefix of the other, the first differing byte is the first byte that exists only in the longer file.

Comparison is byte-exact. Line-ending, Unicode-normalization, whitespace, JSON-formatting, and other semantic normalization are not applied.

## Optional UTF-8 text detail

When both files are at most 4 MiB and both decode as UTF-8, COMPARE additionally reports:

- File A line count,
- File B line count,
- the first differing line using one-based indexing.

This text detail is supplementary. EQUAL / DIFFERENT always comes from the byte-exact streaming comparison.

Binary/non-UTF-8 files and files above the text-detail limit still compare normally; they simply omit line detail.

## Native UI

COMPARE is READY in the Tool Registry and uses a dedicated native view.

File A and File B can be entered as Workspace-relative paths. The current regular FILES selection can also be assigned to either side, and FILES exposes COMPARE AS A / COMPARE AS B actions.

The UI can swap A and B, clear its ephemeral state, and run comparison explicitly.

## Session and secret boundaries

Compare paths, file contents, byte offsets, line counts, and comparison results are not persisted in generic session state. Session restoration stores only that the COMPARE view was active.

COMPARE is an ordinary generic file utility and is not a secret-processing surface. Passwords, Master Passphrases, Recovery Codes, decrypted Vault data, and similar secret values remain outside this path.

## Filesystem race boundary

The portable path validation rejects symlink components and verifies canonical targets before opening files. As with the existing portable filesystem APIs, this does not claim an operating-system-level atomic no-follow guarantee against another process replacing a filesystem entry between validation and open.

## Exactly One Next Task

The repository-wide Exactly One Next Task is:

**Local document search**
