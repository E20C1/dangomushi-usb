# M6 Offline Reconstruction Boundary

## Purpose

M5 established that DANGOMUSHI can preserve its documentation, Rust dependency sources, reviewed recovery inputs, and portable release artifacts without relying on a public service.

M6 addresses the remaining recovery gap: turning an already-verified Recovery Snapshot into a fresh reconstructed environment without mutating the currently running source tree or Workspace.

This document defines the M6 authority boundary only. It does not add a staging command, filesystem reconstruction, Cargo execution, Workspace restore, Secret Backup restore, or network access by itself.

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

The first M6 implementation task will copy a Recovery Snapshot into a new staging root suitable for later reconstruction.

Before any destination mutation, the source Recovery Snapshot must pass the existing read-only verify_snapshot boundary.

The staging destination must:

- be explicit,
- not already exist,
- have a real non-symlink parent,
- remain outside the source Recovery Snapshot,
- remain outside any configured live Workspace,
- remain outside the currently running source tree when that tree is separately supplied,
- reject path aliasing or containment ambiguity.

Only paths already authorized by the Recovery Snapshot format may be materialized.

The staging task may copy:

- reviewed first-party source/build inputs,
- the matching vendor/ dependency snapshot,
- the already-verified recovery/workspace.dgmwbk file as an inert artifact.

It must not:

- restore workspace.dgmwbk,
- read or restore Secret Backup ciphertext,
- discover Vault/Credential/Recovery Code state,
- mutate an existing Workspace,
- run Cargo or another child process,
- install Rust or operating-system packages,
- open network sockets,
- create or trust new identities,
- enable LAN or Internet transport.

The source snapshot remains read-only.

The staging copy must retain the fail-closed path, symlink, special-file, count, size, and SHA-256 semantics already established by Recovery Snapshot verification. The implementation task must define how destination bytes are independently rechecked before the staging result can be accepted.

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

The next authorized implementation task is **Verified snapshot staging materialization**.

No Cargo execution, Workspace restore, Secret restore, release signing, mobile transport, or other post-M5 authority is authorized by this boundary task.
