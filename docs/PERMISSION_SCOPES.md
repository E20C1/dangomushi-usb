# Device Permission Scopes

## Scope

This M3 task introduces the durable authorization policy that a future authenticated LAN transport must consult after device identity and revocation checks.

The canonical store is `workspace/state/device_permissions.v1.json`.

Permission policy is deliberately separate from `identity/`. A public key can be trusted while having no permissions at all.

## Canonical scopes

The first scope vocabulary is intentionally small and fixed:

- `status.read` — necessary authorization for future remote status reads.
- `files.read` — necessary authorization for future non-secret file read surfaces.
- `files.write` — necessary authorization for future explicit non-secret file mutation surfaces.
- `session.handoff` — necessary authorization for the later Mobile editor/session handoff milestone.

These identifiers do not create endpoints and do not broaden existing filesystem, editor, secret, repository, backup, pairing, trust, revocation, or network boundaries. A future endpoint must satisfy its own boundary checks in addition to the required permission scope.

In particular, `files.read` and `files.write` are not blanket authority over `identity/`, `config/`, `state/`, backups, Vault plaintext, Recovery Codes, credentials, arbitrary commands, Git mutation, or network configuration.

## Default deny

A persistent trusted-device public key receives no permission scopes automatically.

`device_permissions::authorize` first requires that the fingerprint is still present in the effective trusted-device set. Revoked or otherwise untrusted keys return false even if a stale permission record remains on disk.

If trusted-device or permission state cannot be verified, authorization does not silently grant access.

## Explicit owner action

The native MOBILE view exposes a separate permission editor for each effective trusted device.

Checking or unchecking a scope changes only ephemeral UI draft state. Durable policy changes occur only when the operator presses `SAVE PERMISSIONS`.

Saving an empty scope set removes the policy record and returns the device to default deny.

## Re-trust safety

Permission records are keyed by the domain-separated trusted-device fingerprint.

Before a newly paired public key is persisted as a new trusted-device record, DANGOMUSHI clears any stale permission record for that fingerprint. Therefore a key that disappeared from trust state because of a historical Workspace restore cannot silently recover old authorization merely by being trusted again.

Revoked fingerprints cannot be trusted again at all without device key rotation, as defined by the Device revocation boundary.

## Generic Workspace Backup / Restore

The permission store lives directly under `workspace/state/`.

Generic Workspace Backup intentionally manages only `state/autosave/`, not the whole `state/` root. Therefore `device_permissions.v1.json` is excluded from DGMWBK01 snapshots and is not replaced by Generic Workspace restore.

This is intentional security behavior:

- restoring an older backup cannot re-grant a previously removed scope,
- restoring a backup into another initialized Workspace does not import permission grants,
- a trusted device restored from backup starts default-deny on a Workspace that has no local permission record.

The permission file still remains durable on the portable Workspace itself across normal application restarts and movement between computers.

## Storage boundary

The store is versioned, sorted, duplicate-free, bounded to 256 device records and 128 KiB, and accepts only the fixed canonical scope identifiers.

The `state/` root and fixed permission files reject symlink or special-file layouts.

Replacement uses create-new pending output, full parse verification, a previous-file rollback slot, file sync and best-effort directory synchronization. Interrupted-write recovery keeps the last verified committed store and never promotes a pending-only first write.

## Threat boundary

This task does not add a non-loopback listener, remote request parser, session token, reconnect policy, public-network policy, mobile editor endpoint, or network authorization transport.

The current embedded gateway remains loopback-only. Permission scopes are an authorization authority for later M3 work, not evidence that LAN access is already available.
