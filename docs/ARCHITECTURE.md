# DANGOMUSHI Architecture

## Goal

DANGOMUSHI is a portable workspace whose useful state remains local and whose core functions do not require a vendor account, cloud service, or public internet connection.

The application is not intended to become a general-purpose web browser.

## Authority and data model

The Rust core is the authority for privileged operations.

Desktop and browser-delivered UI are client surfaces. Neither is the source of truth for files, identity, repository state, backups or permissions.

The intended durable layout is:

~~~text
DANGOMUSHI/
├─ app/          replaceable program output
├─ workspace/    durable user data
├─ build/        temporary staging and rollback material
└─ source tree   development source
~~~

Build and update operations may replace app/. They must not erase workspace/.

The installed desktop executable resolves a sibling workspace/ from the portable root. Development runs fall back to ./workspace. DANGOMUSHI_WORKSPACE can explicitly override the path.

## Filesystem boundary

The native file browser accepts only relative paths composed of normal path components.

Before a directory is traversed, both the workspace root and requested directory are canonicalized and the resolved directory must remain below the canonical workspace root.

Symlinks are listed as links but are not treated as traversable directories.

The text editor applies a stricter file boundary:

- only existing regular files can be opened,
- the path must remain below the canonical workspace root,
- symlinks in the edited path are rejected,
- content must be valid UTF-8,
- files are limited to 4 MiB in the initial editor,
- the editor does not create, rename or delete user files.

## Save and recovery model

Typing never automatically overwrites the source file.

After editing pauses for approximately two seconds, the current buffer is written to a recovery draft below:

~~~text
workspace/state/autosave/
~~~

Recovery filenames are unique per write. A newly durable recovery is created before older drafts for the same path are removed, avoiding in-place replacement of the only recovery copy.

An explicit SAVE first persists a recovery draft, then truncates and writes the original file and calls sync_all. The recovery is cleared only after the explicit file write has completed successfully.

If the original file changes outside DANGOMUSHI after opening, SAVE refuses to overwrite it and stores the current editor buffer as recovery instead.

When a file is opened and a recovery differs from disk, the UI requires the user to choose RESTORE RECOVERY or DISCARD RECOVERY before editing continues.

## Network boundary

The current implementation intentionally supports only:

~~~text
127.0.0.1:<port>
~~~

The listener is not bound to a LAN interface. There is no public listener and no automatic port opening.

The project must not silently add:

- 0.0.0.0 listening,
- UPnP or NAT-PMP port mapping,
- reverse tunnels,
- public relay dependencies,
- externally hosted JavaScript, CSS or fonts.

LAN/mobile mode is a future capability and requires an explicit security design before implementation. At minimum that design must include device pairing, persistent trusted-device identity, revocation, authorization boundaries, and safe behavior on public networks.

## Web gateway

The embedded gateway currently serves only fixed assets and a read-only status endpoint.

Security properties:

- loopback peer check,
- Host header restriction,
- GET/HEAD only,
- fixed route table,
- no arbitrary filesystem path mapping,
- no CORS opt-in,
- Content Security Policy,
- no external assets,
- no state-changing HTTP API.

This deliberately avoids turning localhost into a privileged unauthenticated command API.

## Portability

The Rust core is intended to compile separately for Windows, macOS and Linux while sharing the same workspace format.

Executable files are platform-specific. Workspace data should remain platform-neutral wherever practical.

The first target is Windows because the USB workflow is being developed there. Cross-platform behavior remains an architectural requirement rather than an assumption that one binary can run on every OS.

## UI boundary

The native desktop UI uses eframe/egui behind the Cargo desktop feature. The choice is recorded in UI_LAYER_DECISION.md.

The UI uses a compact, square-edged, pixel-oriented visual system rather than a browser-like SaaS dashboard.

The embedded web UI remains a separate client surface for local status and future authenticated mobile access. It is not used to grant the desktop application filesystem authority.

## Dependency policy

The core CLI, workspace model, filesystem browser, text editor storage layer and loopback server use the Rust standard library only.

The native desktop shell introduces eframe/egui as an optional feature-gated dependency. Building without the desktop feature does not require the desktop stack.

Third-party crates may be introduced when they provide clear value, but offline rebuildability and vendoring must be considered before they become foundational dependencies.
