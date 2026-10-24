# Reproducible Dependency Cache / Vendor Strategy

## Scope

M5 fixes the Rust dependency-resolution and local source-cache strategy required to rebuild DANGOMUSHI when crates.io or GitHub are unavailable.

This task covers:

- a committed Cargo.lock,
- an exact Rust toolchain baseline,
- a deterministic Cargo vendor procedure,
- an explicit offline source-replacement configuration,
- a frozen offline build/test verification path,
- CI enforcement that normal builds do not silently rewrite dependency resolution.

It does not claim a fully hermetic operating-system build. The Rust toolchain distribution itself, Linux development packages, system linkers/SDKs, and platform runtime libraries remain external prerequisites.

## Authoritative dependency resolution

Cargo.lock is committed and is the authoritative Rust package resolution for this application.

Routine CI and update scripts use --locked. If Cargo.toml requires a dependency-resolution change that is not represented in Cargo.lock, the operation must fail rather than silently rewriting the lockfile.

Dependency updates are explicit maintenance operations:

1. intentionally change Cargo.toml or update the selected package,
2. regenerate and review Cargo.lock,
3. run the normal three-platform CI,
4. regenerate the vendor snapshot,
5. run the frozen offline verification path,
6. keep the reviewed lockfile change with the dependency change.

Routine application updates must not run an unrestricted cargo update.

## Rust toolchain baseline

rust-toolchain.toml pins the reproducible build baseline to Rust 1.99.1 with the minimal rustup profile.

CI installs the same exact toolchain rather than the moving stable channel.

Cargo.toml may continue to state a lower language/MSRV floor separately. The pinned toolchain is the reviewed rebuild baseline for this dependency snapshot, not a claim that every older compiler is continuously tested.

The toolchain binaries are not vendored by this task. An offline machine must already have the pinned toolchain installed, or a separately preserved rustup/toolchain distribution must be provided later.

## Vendor snapshot

The generated source snapshot lives at repository-root vendor/.

It is generated with:

~~~text
cargo vendor --locked --versioned-dirs vendor.pending
~~~

and is promoted to vendor/ only after the Cargo command succeeds.

The versioned-directory option keeps every vendored crate directory explicitly versioned, making snapshot inspection and later refreshes less ambiguous.

The helper refuses to overwrite either an existing vendor/ or vendor.pending/. Replacing a dependency cache therefore requires an explicit removal of the previous generated snapshot.

The repository provides:

~~~text
scripts/vendor-dependencies.sh
scripts/vendor-dependencies.cmd
~~~

The scripts require Cargo.lock, create the vendor snapshot, then immediately verify that Cargo metadata resolves through the offline source replacement.

vendor/ is deliberately ignored by Git. It can be large and is a generated cache artifact rather than reviewed first-party source. For long-term offline storage, preserve the generated directory together with the matching source tree and Cargo.lock, or include it in a later reviewed recovery/release artifact.

## Offline Cargo configuration

.cargo/vendor-config.toml is committed as a dedicated opt-in configuration:

~~~toml
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"

[net]
offline = true
~~~

It is intentionally not named .cargo/config.toml. Normal development therefore does not silently switch to a stale vendor tree.

Offline commands opt into the configuration explicitly:

~~~text
cargo --config .cargo/vendor-config.toml ...
~~~

The vendor directory path resolves to repository-root vendor/ under Cargo's config-relative path rules.

## Frozen verification

The repository provides:

~~~text
scripts/verify-vendored-dependencies.sh
scripts/verify-vendored-dependencies.cmd
~~~

The verification path requires both Cargo.lock and vendor/ and runs Cargo with the vendor config plus --frozen.

It verifies:

1. dependency metadata resolution,
2. core tests,
3. core release build,
4. desktop library tests,
5. native desktop release build.

The vendor configuration sets Cargo offline, while --frozen additionally requires the lockfile to remain unchanged. A missing crate, changed resolution, invalid vendor checksum, or attempted network dependency must fail instead of falling back to a registry fetch.

## CI enforcement

Normal Windows, Ubuntu, and macOS CI use the exact Rust 1.99.1 toolchain and pass --locked to every Cargo test/build command.

A separate Ubuntu offline-vendor job:

1. creates a fresh vendor snapshot from the committed lockfile,
2. switches to the explicit vendor source replacement,
3. runs the full frozen core/desktop verification path.

This makes the vendor strategy executable policy rather than documentation only.

The offline-vendor job still installs Linux system development packages before the Cargo network boundary is tested. Those packages are outside the Rust crate cache and remain a separate portability concern.

## Integrity and trust boundary

Cargo registry package checksums recorded through the lock/vendor workflow help detect crate-source corruption and cargo vendor creates Cargo's vendored checksum metadata.

This does not mean DANGOMUSHI independently audits or endorses every dependency. A valid checksum proves consistency with the selected package source, not that the dependency is free of vulnerabilities or malicious logic.

The committed Cargo.lock, pinned toolchain file, vendor configuration, helper scripts, and CI policy are first-party reviewed source. The generated vendor/ directory is third-party source material.

## Persistence boundary

Dependency cache state is build infrastructure, not Workspace user data.

DANGOMUSHI does not copy vendor/, Cargo registry caches, rustup toolchains, or build outputs into:

- workspace/data,
- workspace/state,
- Generic Workspace Backup,
- Secret Backup / Restore,
- autosave,
- session state,
- trusted-device or trusted-peer state.

A later M5 Recovery Snapshot task may define an explicit larger recovery artifact, but this task does not silently change existing backup scope.

## What remains outside this task

This strategy does not yet provide:

- a preserved Rust toolchain installer/distribution,
- cached Linux package-manager repositories or native development libraries,
- Windows SDK or macOS SDK preservation,
- deterministic or bit-for-bit identical final binaries,
- Recovery Snapshots,
- Restore Verification,
- portable per-OS release bundles.

The next M5 task is Recovery snapshots.
