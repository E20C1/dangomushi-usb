# Mobile editor/session handoff

## Scope

This M3 task defines the transport-independent handoff boundary between the authoritative desktop Rust core and a future authenticated mobile client. It does not add a LAN listener, HTTP/WebSocket route, mobile web UI, browser key store, QR transport, or another network protocol.

The existing M3 security boundaries remain authoritative. A future transport must still require explicit process-local LAN intent, a fresh trusted-network assessment, an effectively trusted non-revoked device, fresh reconnect proof, and current permission scopes before invoking this handoff core.

## Session handoff

`mobile_handoff::capture_session` accepts the current in-process `SessionState` and returns only the existing navigation fields:

- active view,
- current FILES directory,
- selected FILES entry,
- current Editor path.

The operation requires the current authenticated device to hold `session.handoff`. Relative paths are validated again before the snapshot is returned.

The handoff snapshot does not contain the Editor buffer, autosave drafts, secrets, credentials, Vault plaintext, pairing material, reconnect credentials, network classification, or permission state. It is ephemeral and is not written to `session.v1` or another durable store.

## Mobile Editor read

Opening a document through the handoff core requires both `session.handoff` and `files.read`.

The operation delegates file access to the existing Text Editor read boundary. The same relative-path, no-symlink, in-Workspace, regular-file, UTF-8 and 4 MiB rules therefore remain authoritative.

A successful read returns the relative path, UTF-8 text, and a 32-byte SHA-256 content revision. The revision is a concurrency token only. It is not an authentication token, bearer credential, secret, or durable capability.

## Mobile Editor save

Saving requires both `session.handoff` and `files.write`.

The caller must return the revision obtained from the version it edited. DANGOMUSHI re-reads the current disk text and compares its revision before saving. If the file changed after handoff, the newer disk file is not overwritten. The submitted mobile text is first stored through the existing Editor recovery-draft boundary and the operation returns a revision conflict.

If the revision still matches, the save delegates to the existing Text Editor save boundary. That boundary writes a recovery draft before touching the source file, flushes the explicit source-file write, and clears stale recovery only after the save succeeds.

This follows the existing desktop Editor conflict model. It does not claim an operating-system-atomic compare-and-swap against another process changing the file in the narrow interval between the revision check and the final write.

## Authorization freshness

The handoff core does not snapshot permissions into a mobile session. Every capture, read, and save calls the current `AuthenticatedDeviceSession::authorize` path.

Consequently, disabling LAN intent, revoking the device, removing trust, removing `session.handoff`, or removing the corresponding `files.read` / `files.write` scope affects subsequent operations without waiting for a fixed session timeout.

## Persistence and transport boundary

No handoff payload, Editor text, content revision, authenticated principal, or mobile-specific credential is added to `session.v1`, Workspace config, Generic Workspace Backup, Secret Backup / Restore, trusted-device identity, revocation state, or permission policy.

`src/server.rs` remains loopback-only at `127.0.0.1`. This task does not create a non-loopback socket, remote endpoint, public bind, firewall mutation, UPnP/NAT-PMP mapping, relay, TLS policy, or mobile transport.

M3's transport-independent mobile security and handoff contracts are complete after this task. The next Exactly One Task is M4 Trusted peer identity.
