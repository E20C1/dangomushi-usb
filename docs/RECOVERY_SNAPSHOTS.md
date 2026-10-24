# Recovery Snapshots

## Scope

M5 Recovery Snapshots create a self-contained offline recovery directory that combines the reviewed DANGOMUSHI source/build inputs, the generated Rust vendor cache, and one already-verified Generic Workspace Backup.

The recovery snapshot is deliberately a higher-level recovery artifact. It does not replace Generic Workspace Backup or Secret Backup / Restore.

Snapshot creation does not restore from the artifact. Existing snapshots can now be checked through the separate read-only Restore Verification boundary.

## Snapshot contents

A recovery snapshot copies a fixed allowlist of first-party build inputs:

~~~text
.gitignore
.cargo/vendor-config.toml
.github/workflows/
Cargo.toml
Cargo.lock
rust-toolchain.toml
README.md
UPDATE_TOOLBOX.cmd
docs/
scripts/
src/
tests/
web/
vendor/
~~~

It also includes exactly one verified Generic Workspace Backup as:

~~~text
recovery/workspace.dgmwbk
~~~

The selected .dgmwbk must first pass the existing Workspace Backup verification boundary before any recovery snapshot destination is created.

The generated vendor/ directory is required. A recovery snapshot therefore captures the exact Rust crate-source cache corresponding to the source tree and Cargo.lock rather than silently depending on crates.io later.

## Deliberate exclusions

Recovery Snapshot creation does not copy arbitrary repository-root files or the complete source directory recursively.

The following are not automatically included:

~~~text
.git/
target/
build/
app/
workspace/
arbitrary root files
live workspace/state/
live Vault storage
Secret Backup files
session.v1
Cargo registry caches outside vendor/
rustup/toolchain distributions
OS SDKs and native package caches
~~~

The live Workspace is never copied directly. Only the selected already-verified Generic Workspace Backup is included, so the existing backup scope remains authoritative.

That means the recovery snapshot does not bypass Generic Workspace Backup exclusions such as session state or M4/M3 state that was intentionally kept outside that backup. Restoring old trust or permission state by silently copying the live state directory would violate existing rollback protections.

M2-S Secret Backup remains separate. A Secret Backup is encrypted but is still a high-value offline passphrase-guessing asset. Recovery Snapshot creation therefore does not discover or bundle it automatically.

## Destination boundary

The destination:

- must not already exist,
- must have an existing real non-symlink parent directory,
- must be outside both the source root and Workspace root,
- is created directly with create-new directory semantics,
- is removed on an in-process creation failure where cleanup succeeds.

The destination is forbidden inside the source tree to avoid recursively capturing the snapshot itself or modifying the source being preserved.

It is also forbidden inside the Workspace to prevent recovery infrastructure from becoming ordinary Workspace data or entering later Generic Workspace backups.

## Source filesystem boundary

Only the fixed allowlist is traversed.

Every required fixed file or directory must exist as the expected real filesystem type.

Within copied directory trees:

- symlinks are rejected,
- special files are rejected,
- path components must be UTF-8,
- parent traversal and separator injection are rejected,
- paths are bounded,
- directory enumeration is sorted before traversal.

A generated vendor cache containing an unsupported entry therefore fails closed instead of silently omitting it.

## Streaming and bounds

Files are copied with 64 KiB streaming buffers rather than loading the complete snapshot into memory.

The first format is bounded to:

- at most 100,000 copied files,
- at most 4 KiB for one normalized snapshot-relative path,
- at most 512 GiB for one copied file,
- at most 3 TiB total copied payload,
- at most 32 MiB for the final snapshot marker.

These limits cover the source/vendor tree plus the embedded Workspace Backup artifact without creating an unbounded archive parser surface.

## Source-change detection

For every copied regular file, DANGOMUSHI records source length and modification metadata before copying, streams and hashes the source bytes, syncs the destination file, checks the source metadata again, and re-hashes the destination.

A changed length or observable modification timestamp causes creation to fail.

This is not a filesystem-wide atomic source snapshot. Concurrent modifications across different files can still produce a cross-file state that did not exist at one single instant. A clean/stable source tree should be used when preparing long-term recovery media.

## Snapshot marker and digest

The completion marker is:

~~~text
.dangomushi-recovery-snapshot
~~~

It is written only after every payload file has been copied and the destination payload has been re-enumerated and re-hashed exactly.

The marker records:

- snapshot kind,
- format version,
- DANGOMUSHI application version,
- fixed Workspace Backup location,
- file count,
- total payload bytes,
- deterministic snapshot SHA-256,
- one path/length/SHA-256 record for every payload file.

Paths in marker entries are hex-encoded UTF-8 bytes so delimiter characters cannot create ambiguous records.

The deterministic snapshot digest is domain-separated and binds:

- format version,
- application version,
- sorted relative path,
- file length,
- per-file SHA-256

for every payload file.

The marker itself is not included in its own digest.

This provides integrity identification, not publisher authenticity. An attacker able to replace the complete snapshot can also construct a new valid SHA-256 marker.

## CLI

The headless CLI exposes creation, while the separate Restore Verification path exposes read-only verification:

~~~text
dangomushi recovery snapshot create \
  --backup backups/<NAME>.dgmwbk \
  --destination <PATH> \
  [--source <SOURCE-ROOT>] \
  [--workspace <WORKSPACE-ROOT>]
~~~

Defaults are:

~~~text
source: .
workspace: ./workspace
~~~

Both --backup and --destination are explicit and required.

Read-only verification is:

~~~text
dangomushi recovery snapshot verify --snapshot <PATH>
~~~

The selected Workspace Backup remains read-only during snapshot creation, and verification does not restore/extract files or mutate a Workspace.

## Buildability boundary

The snapshot layout keeps the copied first-party source and vendor/ at the snapshot root. The copied .cargo/vendor-config.toml therefore continues to point at the snapshot's vendor directory.

With the pinned Rust 1.99.1 toolchain and required operating-system native build prerequisites already installed, the snapshot has the source inputs needed for the existing frozen vendor build path.

This task still does not preserve:

- the Rust toolchain installer/distribution itself,
- Linux native package-manager payloads,
- Windows SDKs,
- macOS SDKs,
- system linkers,
- platform runtime dependencies.

Portable per-operating-system release bundles remain a later M5 task.

## Secret and security-state boundary

Recovery Snapshot does not read Vault plaintext, Master Passphrases, Credentials, Recovery Codes, Clipboard secrets, or Secret Backup ciphertext.

It also does not copy live trusted-peer identity, device permission state, session state, or other Workspace state outside the existing Generic Workspace Backup scope.

This preserves the security decisions already encoded in the separate backup systems.

## Next boundary

Existing recovery snapshots are validated by **Restore Verification** before they are trusted as recovery input. That verifier strictly parses the completion marker, enforces the reviewed layout, re-hashes the exact payload and independently validates the embedded Generic Workspace Backup.

Recovery Snapshot creation still does not add a restore/extract command. Portable per-operating-system release bundles are the next M5 boundary.
