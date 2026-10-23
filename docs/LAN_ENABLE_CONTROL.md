# Explicit LAN Enable Control

## Scope

The first M3 network task establishes explicit operator intent without opening a LAN listener.

`LanEnableControl` is process-local state owned by the native application. It starts disabled on every application launch. The native MOBILE view is the only current user-facing surface that enables or disables it.

## Persistence boundary

The control is intentionally ephemeral.

It is not stored in:

- `workspace/state/session.v1`,
- Workspace `config/`,
- generic Workspace backups,
- Secret Backup / Restore,
- the embedded web gateway.

This prevents a restart or restoration of an older Workspace snapshot from silently restoring a previous LAN-enabled intent.

## Network authority

Enabled does not mean listening.

The control records that the operator has explicitly requested LAN capability for the current process. It does not bind a socket, change the existing loopback gateway, add a state-changing HTTP API, open a firewall port, or perform port mapping.

The current gateway therefore remains:

~~~text
127.0.0.1:<port>
~~~

with its existing loopback peer check and Host restriction.

A future non-loopback listener must require this explicit operator intent and must also remain blocked until later M3 security boundaries are complete, including device pairing, persistent trusted-device identity, revocation, permission scopes, and public-network-safe behavior.

## Failure and restart semantics

There is no durable write associated with enable or disable.

If the process exits, crashes, is updated, or the machine restarts, the next DANGOMUSHI process starts with LAN control disabled.

If a future subsystem cannot establish its required authenticated LAN boundary, this control alone must never be treated as permission to fall back to an unauthenticated listener.
