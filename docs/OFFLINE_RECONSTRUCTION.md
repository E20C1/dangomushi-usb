# M6 Offline Reconstruction Boundary

## Purpose

M5 established that DANGOMUSHI can preserve its documentation, Rust dependency sources, reviewed recovery inputs, and portable release artifacts without relying on a public service.

M6 addresses the remaining recovery gap: turning an already-verified Recovery Snapshot into a fresh reconstructed environment without mutating the currently running source tree or Workspace.

M6 now includes verified snapshot staging materialization and a frozen rebuild boundary. Cargo execution is authorized only through the fixed staged-source rebuild path; rebuilt application execution, Workspace restore, Secret Backup restore, and network transport remain out of scope.

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

Before any destination mutation, the source Recovery Snapshot must pass the existing read-only verify_snapshot boundary. Verification therefore completes before the staging path is created or otherwise mutated.

The staging destination:

- is explicit,
- must not already exist,
- must have a real non-symlink parent,
- must remain outside the source Recovery Snapshot,
- must remain outside any supplied live Workspace,
- must remain outside any supplied active source tree,
- is resolved through its canonical parent before containment checks.

`--source` and `--workspace` are optional because a clean reconstruction machine may have neither live root. When such roots do exist, callers should supply them so staging cannot be placed inside them. In this boundary, the destination must remain outside any configured live Workspace, where a configured Workspace means the live root supplied through `--workspace` (or the equivalent library argument).

After initial verification, the authenticated snapshot marker is read again and staging remains bound to the initially verified file count, total bytes and aggregate SHA-256. Only marker records already accepted by the Recovery Snapshot allowlist are materialized.

Every payload file is copied with the existing bounded 64 KiB streaming copy path. The copier checks the source regular-file type, size limit, observable length/modification changes, streamed SHA-256 and the resulting destination SHA-256. The copied record must exactly equal the corresponding initially verified marker record.

The completion marker is copied last. An interrupted staging directory therefore lacks the marker or otherwise fails the normal verifier rather than being presented as complete.

After all payload plus marker bytes exist, the destination is independently verified again by the existing full Recovery Snapshot verifier. That second verification repeats exact layout checks, symlink/special-file rejection, all payload hashes, aggregate snapshot digest, and independent Generic Workspace Backup archive verification. The staged summary must exactly equal the initially verified source summary.

The staged tree contains reviewed first-party source/build inputs, the matching `vendor/` dependency snapshot, and the already-verified `recovery/workspace.dgmwbk` as an inert file.

Staging does not restore `workspace.dgmwbk`, read or restore Secret Backup ciphertext, discover Vault/Credential/Recovery Code state, mutate an existing Workspace, run Cargo or another child process, install packages, open network sockets, create identities, or enable LAN/Internet transport.

The source snapshot remains read-only.

## Later M6 boundaries

### Frozen rebuild from staged source/vendor

Frozen rebuild is implemented by:

~~~text
offline_reconstruction::frozen_rebuild(...)
~~~

and the CLI:

~~~text
dangomushi recovery snapshot rebuild \
  --staging <PATH> \
  --destination <PATH> \
  [--source <ACTIVE-SOURCE-ROOT>] \
  [--workspace <LIVE-WORKSPACE-ROOT>]
~~~

Before any rebuild destination mutation, the staged tree must pass the existing complete Recovery Snapshot verifier. The staged `rust-toolchain.toml` and `.cargo/vendor-config.toml` are then required to exactly match the reviewed Rust 1.99.1 / crates.io-to-vendor offline baseline.

The rebuild destination is create-new, must have a real non-symlink parent, and must remain outside the staging tree plus any supplied live source or Workspace roots. Cargo output is forced into that separate destination rather than allowing `target/` to modify the verified staging tree.

The core runs exactly two fixed Cargo release builds:

~~~text
dangomushi
dangomushi-desktop --features desktop
~~~

Both use the staged manifest and vendor configuration with `--frozen`. No arbitrary Cargo subcommand, package, feature set, command line, target directory or child executable is accepted from the caller.

Cargo-affecting environment overrides are removed before launch, including `CARGO_*`, compiler/wrapper/RUSTFLAGS variables and `RUSTUP_TOOLCHAIN`. The child receives an isolated `CARGO_HOME` under the create-new rebuild destination and `CARGO_NET_OFFLINE=true`. Parent-directory `.cargo/config` and `.cargo/config.toml` files outside the staging tree are rejected so ambient Cargo configuration is not silently merged into reconstruction.

After both Cargo invocations complete, the entire staged Recovery Snapshot is verified again. Any source/vendor/lock/marker mutation observable through that verifier fails the rebuild and the incomplete destination is removed.

The expected CLI and desktop release artifacts must be real non-symlink regular files and are bounded to 2 GiB each. Their exact byte length and SHA-256 are recorded in a final `.dangomushi-frozen-rebuild` marker written only after both builds and post-build staging verification succeed. The temporary isolated Cargo home is removed before completion.

The rebuild output remains outside the verified staging tree, and the implementation re-verifies the staged Recovery Snapshot after Cargo/build.rs execution. The rebuild does not execute either rebuilt application. **Recovered application smoke verification** remains the next separate task. It also does not restore `recovery/workspace.dgmwbk`, inspect Secret Backup data, mutate an existing Workspace, alter identity/trust state, or enable LAN/Internet transport.

This boundary does not provide an OS-level network sandbox. Cargo/build.rs execution is executable-code authority; `--frozen`, the staged vendor config and `CARGO_NET_OFFLINE=true` constrain Cargo dependency fetching, but arbitrary build scripts or proc-macros could still attempt direct network access through the operating system.

Cargo/build.rs execution is executable-code authority. `--frozen`, the staged vendor config and `CARGO_NET_OFFLINE=true` prevent Cargo dependency resolution from falling back to registries, but this boundary does not provide an OS-level network sandbox for arbitrary build scripts or proc-macros. A malicious source/vendor snapshot could contain executable build logic. Recovery Snapshot SHA-256 proves equality with the accepted snapshot, not publisher authenticity.

The current external-prerequisite limitation also remains: Rust toolchain installers, OS SDKs, native development libraries, linkers, and platform runtime components are not contained in the Recovery Snapshot.

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

Verified snapshot staging materialization and frozen staged rebuild are complete.

The next authorized implementation task is **Recovered application smoke verification**.

That task may execute only the already-produced, integrity-recorded rebuilt application artifacts through a bounded temporary/staging-local smoke path. Workspace restore, Secret restore, release signing, mobile transport, and Internet P2P remain separate later authorities.
