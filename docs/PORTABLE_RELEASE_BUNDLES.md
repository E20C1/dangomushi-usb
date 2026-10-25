# Portable Release Bundles

## Scope

M5 portable release bundles turn the reviewed release binaries into an extraction-ready per-operating-system package that does not require Rust, Cargo, a source checkout, or network access to launch.

This task packages already-built binaries. It does not change Workspace backup scope, restore recovery snapshots, vendor source code into the release, publish Internet services, sign binaries, notarize macOS applications, or create an automatic updater.

## Bundle identity

Every bundle has a deterministic identity:

~~~text
dangomushi-<APP_VERSION>-<OS>-<ARCH>
~~~

Supported operating-system labels are:

- windows
- linux
- macos

Supported architecture labels are:

- x86_64
- aarch64

Windows uses a ZIP archive. Linux and macOS use tar.gz so Unix executable permission bits remain part of the release archive.

The archive contains exactly one root directory named after the bundle identity.

## Reviewed payload

Windows:

~~~text
dangomushi-<VERSION>-windows-<ARCH>/
├─ app/
│  ├─ Dangomushi.exe
│  └─ dangomushi-cli.exe
├─ START_HERE.txt
└─ RELEASE-MANIFEST.json
~~~

Linux:

~~~text
dangomushi-<VERSION>-linux-<ARCH>/
├─ app/
│  ├─ Dangomushi
│  └─ dangomushi-cli
├─ START_HERE.txt
└─ RELEASE-MANIFEST.json
~~~

macOS:

~~~text
dangomushi-<VERSION>-macos-<ARCH>/
├─ app/
│  ├─ Dangomushi.app/
│  │  └─ Contents/
│  │     ├─ Info.plist
│  │     └─ MacOS/
│  │        └─ Dangomushi
│  └─ dangomushi-cli
├─ START_HERE.txt
└─ RELEASE-MANIFEST.json
~~~

No Workspace, Generic Workspace Backup, Secret Backup, Recovery Snapshot, vendor tree, source tree, .git directory, Cargo cache, target directory, build directory, or local configuration is bundled.

The desktop and CLI binaries already contain the Offline Documentation Pack, so a separate documentation tree is not required for basic offline use.

## Portable Workspace location

Windows and Linux executables live below app/, so the existing portable Workspace rule resolves to a sibling workspace/ directory at the bundle root.

The macOS desktop executable lives inside app/Dangomushi.app/Contents/MacOS/. The Workspace resolver recognizes that reviewed .app layout and maps it to the same bundle-root workspace/ directory.

The release archive does not pre-create or populate that Workspace. The user initializes it explicitly on first use.

This keeps user data separate from replaceable release binaries.

## Manifest

RELEASE-MANIFEST.json is a canonical machine-readable inventory with:

- kind = dangomushi-portable-release
- format = 1
- application version
- operating system
- architecture
- bundle identity
- workspace layout identifier
- sorted payload records

Each payload record contains:

- relative path
- byte length
- SHA-256
- executable flag

The verifier requires the archive file set to match the manifest exactly. Missing files, unexpected files, duplicate paths, traversal paths, non-regular archive entries, oversized payloads, digest mismatches, and missing executable permission bits fail closed.

## Archive checksum

The builder also creates:

~~~text
<ARCHIVE-NAME>.sha256
~~~

The sidecar contains the SHA-256 of the complete archive and its exact filename.

Release verification checks the sidecar before inspecting the internal manifest.

SHA-256 provides transfer/integrity checking only. It does not prove publisher identity.

## Build and verification

The repository provides:

~~~text
python scripts/build-release-bundle.py
python scripts/verify-release-bundle.py --directory dist
~~~

The builder consumes only the reviewed release binaries from target/release and generated fixed metadata.

It refuses to overwrite an existing archive or checksum sidecar.

Archive metadata is normalized so packaging does not embed the current wall-clock time. This improves repeatability of packaging for identical binary inputs, but the project still does not claim bit-for-bit reproducible compiled binaries.

The verifier never extracts archive contents to the filesystem. It reads archive members directly, rejects unsafe entry types/paths, checks the canonical manifest, streams every payload through SHA-256, and validates executable mode metadata.

## CI boundary

The normal Windows, Ubuntu, and macOS CI paths build the core CLI and native desktop release binaries, then build and verify the corresponding portable bundle on the same runner.

On a push to main, CI uploads the verified archive and its SHA-256 sidecar as direct workflow artifacts.

Pull requests and feature-branch pushes still build and verify bundles but do not publish them as downloadable release artifacts.

This task does not automatically create a GitHub Release or attach files to a public release tag.

## Platform limitations

The Windows binaries are not Authenticode-signed by this task.

The macOS .app is not code-signed or notarized by this task and can therefore trigger Gatekeeper warnings.

Linux desktop portability is bounded by the native runtime/library expectations of the reviewed build environment. The bundle is not a container and does not include a Linux distribution or system graphics libraries.

These limitations are explicit so a checksum-valid bundle is not mistaken for a signed or universally self-contained platform package.

## Security and persistence boundary

Portable release packaging is build infrastructure.

The builder and verifier do not read or copy:

- live Workspace data,
- Vault or Credential plaintext,
- Secret Backup ciphertext,
- trusted-device or trusted-peer state,
- Recovery Snapshot data,
- vendor/,
- Cargo registry caches,
- source-control metadata.

No release-bundle state is written into session.v1, autosave, Workspace Backup, Secret Backup, or identity stores.

## M5 completion

Portable release bundles are the final currently-defined M5 Offline continuity task.

Completion of this task does not authorize a new milestone automatically. Any post-M5 work should be added explicitly to the roadmap with its own scoped boundary before implementation.
