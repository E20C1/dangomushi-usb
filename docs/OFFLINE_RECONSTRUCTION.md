# M6 Offline Reconstruction Boundary

## Purpose

M5 established that DANGOMUSHI can preserve its documentation, Rust dependency sources, reviewed recovery inputs, and portable release artifacts without relying on a public service.

M6 addresses the remaining recovery gap: turning an already-verified Recovery Snapshot into a fresh reconstructed environment without mutating the currently running source tree or Workspace.

M6 now completes the full bounded offline reconstruction chain: verified snapshot staging, frozen rebuild, recovered-application smoke verification, fresh Workspace restore, and final reconstruction verification. Cargo execution remains authorized only through the fixed staged-source rebuild path, and rebuilt application execution remains authorized only through the fixed smoke-verification entrypoints. Secret Backup restore, release signing/provenance, automatic updates, and additional network authority remain out of scope.

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

## Completed M6 boundaries

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

The rebuild output remains outside the verified staging tree, and the implementation re-verifies the staged Recovery Snapshot after Cargo/build.rs execution. The rebuild itself does not execute either rebuilt application. Recovered application smoke verification is a separate later boundary in the M6 sequence. The rebuild also does not restore `recovery/workspace.dgmwbk`, inspect Secret Backup data, mutate an existing Workspace, alter identity/trust state, or enable LAN/Internet transport.

This boundary does not provide an OS-level network sandbox. Cargo/build.rs execution is executable-code authority; `--frozen`, the staged vendor config and `CARGO_NET_OFFLINE=true` constrain Cargo dependency fetching, but arbitrary build scripts or proc-macros could still attempt direct network access through the operating system.

Cargo/build.rs execution is executable-code authority. `--frozen`, the staged vendor config and `CARGO_NET_OFFLINE=true` prevent Cargo dependency resolution from falling back to registries, but this boundary does not provide an OS-level network sandbox for arbitrary build scripts or proc-macros. A malicious source/vendor snapshot could contain executable build logic. Recovery Snapshot SHA-256 proves equality with the accepted snapshot, not publisher authenticity.

The current external-prerequisite limitation also remains: Rust toolchain installers, OS SDKs, native development libraries, linkers, and platform runtime components are not contained in the Recovery Snapshot.

### Recovered application smoke verification

Recovered application smoke verification is implemented by:

~~~text
offline_reconstruction::smoke_verify_rebuild(...)
~~~

and the CLI:

~~~text
dangomushi recovery snapshot smoke \
  --staging <PATH> \
  --rebuild <PATH>
~~~

The staged tree must first pass the complete Recovery Snapshot verifier again. The frozen rebuild completion marker is then read as a bounded real regular file and strictly parsed. Its source-snapshot SHA-256 must equal the newly verified staging summary; its toolchain must remain Rust 1.99.1; and its OS/architecture must match the host that is about to execute the binaries.

The marker may name only the fixed release artifact paths produced by the previous task:

~~~text
target/release/dangomushi[.exe]
target/release/dangomushi-desktop[.exe]
~~~

Both artifacts are required to be real non-symlink regular files and are re-hashed before execution. Their exact byte lengths and SHA-256 values must match the frozen rebuild marker.

The staged Cargo.toml supplies the expected rebuilt application version. Smoke execution is fixed and caller arguments cannot select another command:

- the CLI is executed only as `dangomushi version`,
- the desktop binary is executed only as `dangomushi-desktop --recovery-smoke-check`.

The desktop smoke flag exits before eframe/native GUI initialization. It prints one version-bound success line and opens no application window.

Before process launch, a fixed create-new `.dangomushi-recovery-smoke` sandbox is created under the rebuild root. The child current directory is that sandbox. HOME, USERPROFILE, XDG config/cache/data, APPDATA/LOCALAPPDATA and temporary-directory environment variables are redirected there, while inherited DANGOMUSHI_* variables are removed. stdin is null.

stdout and stderr go to create-new files inside the smoke sandbox, are bounded to 64 KiB each, and are validated against exact expected output. Each child has a 15-second timeout. Non-zero exit, timeout, stderr output or unexpected stdout fails closed. The sandbox is removed after the attempt and is never used as a Workspace.

After execution, the rebuild completion marker and both artifact hashes are checked again. The complete staged Recovery Snapshot is also independently re-verified and must equal its pre-execution summary. Observable mutation of staged source/vendor, the rebuild marker or either executable therefore fails smoke verification.

This task does not restore `recovery/workspace.dgmwbk`, create or merge a live Workspace, restore Secret Backup ciphertext, execute arbitrary caller-supplied commands/arguments, start the normal desktop GUI, open a LAN listener, or enable Internet P2P.

Smoke verification intentionally executes recovered native code and therefore is executable-code authority. Environment redirection, fixed entrypoints and timeouts narrow expected side effects, but this is not an operating-system sandbox. A malicious but hash-consistent reconstructed binary could still attempt direct filesystem/network/system actions. Recovery Snapshot and rebuild SHA-256 values prove equality with the accepted recovery inputs, not publisher authenticity.

### Workspace restore into a fresh destination

Fresh Workspace reconstruction restore is implemented by:

~~~text
offline_reconstruction::restore_workspace_into_fresh_destination(...)
~~~

and the CLI:

~~~text
dangomushi recovery snapshot restore-workspace \
  --staging <STAGING-PATH> \
  --rebuild <REBUILD-PATH> \
  --destination <FRESH-WORKSPACE> \
  [--source <ACTIVE-SOURCE-ROOT>] \
  [--workspace <LIVE-WORKSPACE-ROOT>]
~~~

The destination must not exist, must have a real non-symlink parent, and must remain outside the verified staging tree, rebuild tree, and any supplied active source or live Workspace roots. Existing Workspace overwrite or merge is rejected.

Before destination mutation, the implementation reruns the complete recovered-application smoke gate. This re-verifies the staged Recovery Snapshot, rebuild marker, artifact hashes and fixed CLI/desktop smoke entrypoints.

Only the fixed staged `recovery/workspace.dgmwbk` is accepted. It is independently checked through the Generic Workspace Backup verifier and its summary must equal the Workspace Backup summary authenticated by the verified Recovery Snapshot.

The fresh destination is then initialized only after backup verification. Restoration reuses the existing Generic Workspace Backup extraction, restore staging/rollback, managed-root replacement and device-revocation merge path rather than defining another archive or restore format.

The external backup bytes are streamed and hashed before and after restore and the backup is re-verified afterward. The entire staged Recovery Snapshot is also re-verified after restoration. Observable mismatch removes the newly created destination and fails closed.

The restored scope is exactly Generic Workspace Backup scope: data, repositories, identity, config and autosave return. State excluded by Generic Backup does not become reconstructed implicitly; session state, local permission policy and peer-private state remain absent/default. The embedded recovery archive is not silently copied into the new Workspace backups directory.

Secret Backup remains separate and is not auto-discovered or restored. This task does not authorize restore into an existing Workspace, Vault/Credential/Recovery Code recovery, LAN/Internet transport or release signing.

### Reconstruction verification

Final reconstruction verification is implemented by:

~~~text
offline_reconstruction::verify_reconstruction(...)
~~~

and the CLI:

~~~text
dangomushi recovery snapshot verify-reconstruction \
  --staging <STAGING-PATH> \
  --rebuild <REBUILD-PATH> \
  --restored-workspace <FRESH-WORKSPACE>
~~~

The command accepts only evidence that already exists. It does not stage a snapshot, rebuild binaries, or create/restore a Workspace. The staging, rebuild, and restored Workspace roots must each be real non-symlink directories and must remain pairwise separate.

Verification reuses `smoke_verify_rebuild` rather than adding another executable-validation path. The existing gate therefore re-verifies the Recovery Snapshot, frozen-rebuild marker, source-snapshot binding, host OS/architecture, fixed CLI/desktop artifact paths and SHA-256 values, and then executes only the already-authorized `version` and `--recovery-smoke-check` entrypoints.

After that bounded executable check, the final verifier independently validates the fixed staged `recovery/workspace.dgmwbk` and requires its Generic Workspace Backup summary to equal the backup summary authenticated by the Recovery Snapshot.

The restored Workspace is then compared read-only against that archive. The existing Generic Backup parser remains authoritative: every managed directory and file must exist with the expected type, every file must have the exact recorded length and byte content, and a recursive bounded enumeration rejects any extra managed entry. The verifier does not create a temporary replacement backup and does not rewrite the Workspace.

The non-managed fresh-reconstruction boundary is checked separately. The Workspace must have only the initialized top-level layout, `backups/` must remain empty, and `state/` may contain only the restored `autosave/` root. This prevents excluded local state such as session data, device permission policy, or peer-private identity from being mistaken for reconstructed Generic Backup state.

Because the smoke gate executes recovered native code and is not an OS sandbox, the restored Workspace and its exact managed contents are checked after smoke execution as well. Finally the complete staged Recovery Snapshot is re-verified once more and must equal the summary bound by the smoke gate.

A successful final result therefore binds four claims: the staged source/vendor snapshot still verifies, the rebuild artifacts still match their frozen marker and pass fixed smoke entrypoints, the embedded Generic Backup still matches the authenticated snapshot metadata, and the fresh Workspace managed contents exactly match that backup.

This remains integrity/reconstruction evidence, not publisher authenticity. Secret Backup recovery, release signing/provenance, automatic updates, mobile transport, and Internet P2P remain separate authorities.

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

Verified snapshot staging materialization, frozen staged rebuild, recovered application smoke verification, fresh Workspace restoration, and final reconstruction verification are complete.

**M6 Offline reconstruction is complete.**

No post-M6 implementation task is authorized by the current roadmap. Selecting a later milestone must remain a separate decision; Secret restore, release signing/provenance, automatic updates, mobile transport, and Internet P2P are not implicitly authorized by completing M6.
