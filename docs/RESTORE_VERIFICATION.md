# Restore Verification

## Scope

M5 Restore Verification adds a read-only trust gate for an existing Recovery Snapshot before any reconstruction, extraction, build attempt, or Workspace restore is accepted.

This task verifies recovery inputs only. It does not write a source tree, restore a Workspace, extract files, replace application binaries, restore Secret Backup data, or modify trusted-device / trusted-peer state.

The public entry point is:

~~~text
recovery_snapshot::verify_snapshot(<SNAPSHOT-ROOT>)
~~~

and the CLI exposes:

~~~text
dangomushi recovery snapshot verify --snapshot <PATH>
~~~

## Root and marker boundary

The snapshot root must be a real non-symlink directory.

The completion marker must be a real regular file named:

~~~text
.dangomushi-recovery-snapshot
~~~

The marker is read with the existing 32 MiB maximum and must use the exact version-1 schema written by Recovery Snapshot creation.

Verification rejects:

- missing or non-regular markers,
- unsupported format versions,
- malformed or non-canonical integer fields,
- malformed lowercase hexadecimal fields,
- invalid UTF-8 entry paths,
- duplicate or unsorted entry paths,
- entry count overflow,
- per-file or aggregate size overflow,
- unknown or malformed header fields,
- a missing final newline.

The application version recorded in the marker is parsed from the snapshot and is used when recomputing the snapshot digest. Verification therefore does not require the snapshot to have been created by exactly the currently running application version.

## Reviewed layout verification

A self-consistent marker is not sufficient.

Every payload path must still be inside the reviewed Recovery Snapshot layout:

~~~text
.gitignore
.cargo/vendor-config.toml
.github/workflows/**
Cargo.toml
Cargo.lock
rust-toolchain.toml
README.md
UPDATE_TOOLBOX.cmd
docs/**
scripts/**
src/**
tests/**
web/**
vendor/**
recovery/workspace.dgmwbk
~~~

All fixed files must be present.

Every reviewed source directory must exist as a real non-symlink directory and must contribute at least one payload file. The .cargo, .github, recovery, reviewed source directories, and descendants under reviewed directory roots are the only accepted directory layout.

Unexpected empty directories are rejected as well as unexpected files. A caller cannot add a new payload, rewrite the marker to include it, and have the directory accepted as a normal DANGOMUSHI Recovery Snapshot.

Symlink and special-file entries fail closed.

## Exact payload verification

After marker parsing and semantic layout validation, the snapshot payload is enumerated again from disk.

The verifier requires the actual regular-file set, excluding only the marker itself, to exactly match the marker entry set.

Every payload file is streamed and SHA-256 hashed. Its actual byte length and digest must exactly match the marker record.

The verifier then recomputes the domain-separated aggregate Recovery Snapshot digest using:

- snapshot format version,
- the marker's application version,
- record count,
- sorted relative path,
- byte length,
- per-file SHA-256

for every payload file.

The aggregate digest must match the marker.

This detects ordinary missing, modified, truncated, replaced, or additional snapshot payloads.

The marker and SHA-256 records provide integrity identification, not publisher authenticity. An attacker able to replace the entire snapshot and marker can construct a different internally consistent snapshot. Distribution authenticity remains a separate release/signing concern.

## Embedded Workspace Backup verification

The fixed:

~~~text
recovery/workspace.dgmwbk
~~~

is not trusted merely because its outer snapshot SHA-256 matches.

After outer payload verification, Restore Verification calls the existing Generic Workspace Backup archive verifier directly on the embedded backup file.

That existing verifier checks:

- DGMWBK01 format,
- managed-path structure,
- entry and size limits,
- every file SHA-256,
- whole-archive SHA-256,
- required managed roots,
- absence of trailing archive data.

After archive verification completes, Restore Verification hashes the embedded .dgmwbk again and compares it to its outer Recovery Snapshot record. This catches an observable replacement while the nested backup is being inspected.

The embedded backup is not extracted and no Workspace is modified.

## Mutation boundary

Restore Verification is read-only.

It does not:

- create or remove snapshot files,
- create staging or rollback directories,
- copy the source tree,
- write into a Workspace,
- invoke Generic Workspace restore,
- invoke Secret Backup restore,
- run Cargo,
- start a process,
- open a network socket.

No snapshot verification state is persisted to session.v1, autosave, Generic Workspace Backup, Secret Backup, trusted-device state, or trusted-peer state.

## Concurrency limitation

Verification is not a filesystem-wide atomic snapshot operation.

Each file is checked against the authenticated marker, and the embedded Workspace Backup is re-hashed after nested archive verification. A hostile process with concurrent write access could still race unrelated files after they were checked.

Long-term recovery media should therefore be treated as immutable/read-only while verification and later reconstruction are performed.

Any future restore/extraction implementation must either consume the exact verified immutable snapshot or repeat the required integrity checks before committing writes.

## Relationship to Recovery Snapshot creation

Recovery Snapshot creation and Restore Verification use the same format and digest rules.

Creation still verifies its destination before writing the completion marker. Public Restore Verification adds the inverse read path for an already-existing snapshot and adds semantic layout validation plus nested Workspace Backup validation before the artifact is accepted as recovery input.

## Secret and security-state boundary

Restore Verification does not add Secret Backup ciphertext to the snapshot and does not inspect Vault plaintext, Credentials, Master Passphrases, Recovery Codes, Clipboard contents, live peer private keys, or permission policy.

The embedded Generic Workspace Backup remains authoritative for its existing managed scope. Restore Verification does not broaden that scope.

## Next boundary

The next M5 task is **Portable release bundles per operating system**.

Restore Verification does not add recovery snapshot restore/extract behavior. It establishes the read-only verification gate that a future reconstruction or release workflow must pass before trusting snapshot content.
