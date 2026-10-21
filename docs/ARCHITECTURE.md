# DANGOMUSHI Architecture

## Goal

DANGOMUSHI is a portable workspace whose useful state remains local and whose core functions do not require a vendor account, cloud service, or public internet connection.

The application is not intended to become a general-purpose web browser.

## Authority and data model

The Rust core is the authority for privileged operations.

Browser-delivered UI is a client surface. It must not become the source of truth for files, identity, repository state, backups or permissions.

The intended durable layout is:

```text
DANGOMUSHI/
├─ app/          replaceable program output
├─ workspace/    durable user data
├─ build/        temporary staging and rollback material
└─ source tree   development source
```

Build and update operations may replace `app/`. They must not erase `workspace/`.

## Network boundary

v0.1 intentionally supports only:

```text
127.0.0.1:<port>
```

The listener is not bound to a LAN interface. There is no public listener and no automatic port opening.

The project must not silently add:

- `0.0.0.0` listening,
- UPnP or NAT-PMP port mapping,
- reverse tunnels,
- public relay dependencies,
- externally hosted JavaScript, CSS or fonts.

LAN/mobile mode is a future capability and requires an explicit security design before implementation. At minimum that design must include device pairing, persistent trusted-device identity, revocation, authorization boundaries, and safe behavior on public networks.

## Web gateway

The embedded gateway currently serves only fixed assets and a read-only status endpoint.

Security properties in v0.1:

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

## UI direction

The native desktop UI is expected to use a compact, square-edged, pixel-oriented visual system rather than a browser-like SaaS dashboard.

The embedded web UI follows the same visual language for status and future mobile access.

## Dependency policy

The bootstrap core currently uses only the Rust standard library. This is intentional: a freshly built DANGOMUSHI binary should not need a separate web runtime or package manager at run time.

Third-party crates may be introduced when they provide clear value, but offline rebuildability and vendoring must be considered before they become foundational dependencies.
