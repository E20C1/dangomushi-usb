# Local Document Search

Status: **M2 Local document search implemented and READY in the native Tool Launcher.**

## Scope

SEARCH performs a local, read-only literal text search under:

~~~text
workspace/data/documents/
~~~

It deliberately does not search the rest of the Workspace. In particular, generic internal state and other functional areas such as:

~~~text
workspace/state/
workspace/backups/
workspace/repositories/
workspace/identity/
workspace/config/
~~~

remain outside this search boundary.

## Search semantics

The query is an exact, case-sensitive, single-line UTF-8 literal.

Queries must:

- contain at least one non-whitespace character,
- be at most 4 KiB,
- contain no CR or LF characters.

Matches are reported once per matching line using the first occurrence on that line.

Each result contains:

- Workspace-relative document path,
- one-based line number,
- one-based Unicode character column,
- a bounded preview around the matching position.

The preview is display-only and is capped at 240 characters.

## Resource bounds

Search is intentionally bounded:

- maximum document size: 4 MiB,
- maximum regular files considered: 10,000,
- maximum returned matching lines: 500,
- maximum preview: 240 characters.

Files are read with a 4 MiB + 1 byte hard limit so a file that grows after metadata inspection cannot cause an unbounded read.

Binary or otherwise non-UTF-8 files are skipped. Oversized files are skipped. The report exposes skipped-file counts.

If the result limit or file limit is reached, the report is explicitly marked partial/truncated.

## Filesystem boundary

The search root is resolved from the configured Workspace root through the fixed normal path:

~~~text
data/documents
~~~

Both path components must be real directories and not symlinks.

Recursive traversal uses non-symlink directory entries only. Symlink files and directories encountered below data/documents are skipped rather than followed. Canonical traversed directories and files must remain below the canonical documents root and Workspace root.

The portable filesystem validation does not claim an operating-system-level atomic no-follow guarantee against another process replacing an entry between validation and open.

## Native UI

SEARCH is READY in the Tool Registry and dispatches to a dedicated native SEARCH / DOCUMENTS view.

The UI provides:

- ephemeral query input,
- explicit SEARCH DOCUMENTS action,
- result / scanned / skipped counts,
- truncation warnings,
- result path + line + column + preview,
- OPEN action to pass a matched regular document path to the existing Editor boundary.

Opening a result uses the Editor's existing Workspace-rooted UTF-8 file boundary. Search itself never writes a source document.

## Session and secret boundaries

Search query text, previews, result paths, line/column values, and report counts are not persisted in generic session state. Session restoration stores only that the SEARCH view was active.

SEARCH is an ordinary generic non-secret utility. Passwords, Master Passphrases, Recovery Codes, decrypted Vault data, and similar secret-subsystem plaintext must not be routed through this search path.

## Exactly One Next Task

The repository-wide Exactly One Next Task is:

**Repository status and safe Git operations**
