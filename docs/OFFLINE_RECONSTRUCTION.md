# M6 Offline Reconstruction Boundary

## Purpose

M5 established that DANGOMUSHI can preserve its documentation, Rust dependency sources, reviewed recovery inputs, and portable release artifacts without relying on a public service.

M6 addresses the remaining recovery gap: turning an already-verified Recovery Snapshot into a fresh reconstructed environment without mutating the currently running source tree or Workspace.

M6 now includes the first implementation boundary: verified snapshot staging materialization. It still does not authorize Cargo execution, Workspace restore, Secret Backup restore, or network access.

## Why this milestone comes next

The current Recovery Snapshot can be created and verified, but verification is intentionally read-only.

The repository explicitly treats that verifier as a trust gate for a later reconstruction path. The next milestone therefore completes the offline-recovery story before opening unrelated authorities such as remote mobile transport, Internet P2P, automatic update distribution, or platform code-signing services.

## M6 sequence

M6 is ordered as follows.

1. **Verified snapshot staging materialization**
2. **Frozen rebuild from staged source/vendor**
3. **Recovered application smoke verification**
4. **Workspace restore into a fresh destination**
5. **Reconstruction verification**

Each item is a separate Exactly One Task. Later items must not be pulled into an earlier task.

## First boundary: verified snapshot staging materialization

Verified snapshot staging is implemented by:

~~~text
recovery_snapshot::stage_verified_snapshot(...)
~~~

and the CLI:

~~~text
dangomushi recovery snapshot stage \
  --snapshot <PATH> \
  --destination <PATH> \
  [--source <ACTIVE-SOURCE-ROOT>] \
  [--workspace <LIVE-WORKSPACE-ROOT>]
~~~

The source Recovery Snapshot must pass the existing read-only `verify_snapshot` boundary before creating the destination. Verification therefore completes before the staging path is mutated.

The staging destination:

- is explicit,
- must not already exist,
- must have a real non-symlink parent,
- must remain outside the source Recovery Snapshot,
- must remain outside any supplied live Workspace,
- must remain outside any supplied active source tree,
- is resolved through its canonical parent before containment checks.

`--source` and `--workspace` are optional because a clean reconstruction machine may have neither live root. When such roots do exist, callers should supply them so staging cannot be placed inside them.

After initial verification, the authenticated snapshot marker is read again and staging remains bound to the initially verified file count, total bytes and aggregate SHA-256. Only marker records already accepted by the Recovery Snapshot allowlist are materialized.

Every payload file is copied with the existing bounded 64 KiB streaming copy path. The copier checks the source regular-file type, size limit, observable length/modification changes, streamed SHA-256 and the resulting destination SHA-256. The copied record must exactly equal the corresponding initially verified marker record.

The completion marker is copied last. An interrupted staging directory therefore lacks the marker or otherwise fails the normal verifier rather than being presented as complete.

After all payload plus marker bytes exist, the destination is independently verified again by the existing full Recovery Snapshot verifier. That second verification repeats exact layout checks, symlink/special-file rejection, all payload hashes, aggregate snapshot digest, and independent Generic Workspace Backup archive verification. The staged summary must exactly equal the initially verified source summary.

The staged tree contains reviewed first-party source/build inputs, the matching `vendor/` dependency snapshot, and the already-verified `recovery/workspace.dgmwbk` as an inert file.

Staging does not restore `workspace.dgmwbk`, read or restore Secret Backup ciphertext, discover Vault/Credential/Recovery Code state, mutate an existing Workspace, run Cargo or another child process, install packages, open network sockets, create identities, or enable LAN/Internet transport.

The source snapshot remains read-only.

## Later M6 boundaries

### Frozen rebuild from staged source/vendor

A later task may run the existing pinned Rust/Cargo build path from the staged source using the staged vendor/ cache and the existing offline Cargo source replacement.

That task must not silently download missing crates or alter Cargo.lock.

The current limitation remains: Rust toolchain installers, OS SDKs, native development libraries, linkers, and platform runtime components are not contained in the Recovery Snapshot.

### Recovered application smoke verification

A later task will define a bounded smoke test for rebuilt CLI/desktop artifacts before they are treated as recovered application binaries.

Smoke verification must use temporary or staging-local state rather than a live Workspace.

### Workspace restore into a fresh destination

A later task may use the embedded Generic Workspace Backup to create a new Workspace destination.

The first restore target must be create-new. Overwriting or merging an existing live Workspace is not authorized by this milestone definition.

The Generic Workspace Backup verifier/restore boundary remains authoritative.

### Reconstruction verification

The final M6 task will define what evidence is required to declare a reconstruction complete: staged source integrity, frozen dependency resolution, rebuilt application evidence, and restored Workspace verification.

## Secrets remain separate

Secret Backup / Restore is intentionally not folded into M6 reconstruction.

A Recovery Snapshot does not contain Secret Backup ciphertext and must not auto-discover it. If secret recovery is needed, the existing authenticated Secret Backup / Restore flow remains a separate explicit user action with its own passphrase boundary.

## Other post-M5 work remains separate

M6 does not authorize:

- Authenticode signing,
- Apple code-signing or notarization,
- release-key management,
- automatic application updates,
- mobile editor/session LAN transport,
- public/global peer networking,
- rendezvous or relay infrastructure,
- NAT traversal,
- Internet P2P.

Release publisher provenance is a separate future security milestone because it introduces signing-key custody and platform/vendor-service decisions.

M3 mobile handoff transport is also a separate future network milestone because it must compose device trust, revocation, permissions, reconnect proof, and live network-safety gates.

Internet P2P remains disabled under the M4 threat-review decision.

## Exactly One Next Task

Verified snapshot staging materialization is complete.

The next authorized implementation task is **Frozen rebuild from staged source/vendor**.

That task may add bounded Cargo execution against the staged source and staged `vendor/`, but it must preserve offline/frozen dependency resolution and must not pull smoke verification, Workspace restore, Secret restore, release signing, mobile transport, or network authority forward.
