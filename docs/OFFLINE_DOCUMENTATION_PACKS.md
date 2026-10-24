# Offline Documentation Packs

## Scope

M5 begins by making DANGOMUSHI's own operational and security documentation available without GitHub, a source checkout, CDN, account, or public Internet access.

The current pack is compiled directly into the Rust binary from `README.md` and every Markdown file under `docs/`.

The pack is read-only at runtime. It does not fetch remote content, render external assets, execute scripts, or depend on a browser.

## Embedded pack

The embedded pack has a fixed ID and format version:

- pack ID: `dangomushi-core-docs`
- format version: `1`
- source: compile-time `include_str!` content
- content: `README.md` plus every repository `docs/*.md` file
- per-document size limit: 4 MiB

Every embedded path must be a unique relative normal-component path. Absolute paths, parent traversal and backslash paths are rejected by pack validation.

The pack summary computes a deterministic SHA-256 over a domain-separated sequence containing format version, every document path, document length and each document SHA-256.

The pack digest identifies this exact embedded documentation set. It is an integrity identifier, not a publisher signature and not a substitute for verifying the executable itself.

## Offline access

The headless CLI exposes:

- `dangomushi docs list`
- `dangomushi docs show <PATH>`
- `dangomushi docs export [DESTINATION]`
- `dangomushi docs verify [DESTINATION]`

List and show read only the bytes already embedded in the executable.

The native SYSTEM view also exposes the same embedded pack through a read-only selector/viewer. Viewer selection remains process-local and is not written to `session.v1`.

## Explicit export

Export is opt-in and create-new only.

The destination directory must not already exist. DANGOMUSHI creates the new directory, writes the fixed embedded files, syncs each file, then writes a `.dangomushi-offline-docs` marker containing:

- pack format version,
- pack ID,
- application version,
- document count,
- total embedded bytes,
- deterministic pack SHA-256.

If export fails, DANGOMUSHI attempts to remove the newly-created destination rather than leave it looking complete.

Export never overwrites an existing documentation directory.

## Export verification

`verify_export` checks the exported directory against the current executable's embedded pack.

Verification requires:

- a real non-symlink pack root,
- the exact marker contents,
- every expected document as a regular non-symlink file,
- exact document byte equality with the embedded copy,
- no unexpected files,
- no special files or symlinks.

A modified, missing, extra, truncated or replaced document fails verification.

This verification answers only whether the exported directory exactly matches the documentation embedded in the current executable. It is not the later M5 Recovery Snapshot / Restore Verification feature.

## Persistence boundary

The embedded pack is application content, not Workspace user data.

DANGOMUSHI does not automatically copy the pack into `workspace/data`, `workspace/state`, Generic Workspace Backup, Secret Backup, autosave or session state.

A user may explicitly export a pack to a chosen destination. That exported directory is ordinary user-controlled filesystem content after export.

## Network boundary

The offline documentation module has no socket, HTTP client, DNS, Git, shell or process-launch authority.

It does not alter Direct LAN, Internet P2P policy, the loopback status server, firewall configuration or network-safety state.

## M5 boundary

This task provides offline access to DANGOMUSHI documentation only.

It does not vendor Rust dependencies, create recovery snapshots, verify restores, or build portable operating-system release bundles. Those remain later M5 tasks.
