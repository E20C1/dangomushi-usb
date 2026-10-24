# Device Revocation

## Scope

This M3 task adds durable public-key revocation on top of the existing pairing and trusted-device stores.

The canonical tombstone ledger is `workspace/identity/device_revocations.v1.json`.

A revocation records only the domain-separated SHA-256 fingerprint of an Ed25519 public key. The fingerprint is a durable tombstone: there is no un-revoke operation in this milestone.

To trust the same physical device again after revocation, the device must rotate to a new Ed25519 keypair and complete pairing and explicit persistent trust again.

## Effective trust

`trusted_devices::load` is now an effective-trust read. It validates the trusted-device store and the revocation ledger, then removes every trusted-device record whose fingerprint is tombstoned.

If the revocation ledger is malformed, unsafe or unavailable, trusted-device loading fails closed instead of ignoring revocation state.

`register_candidate` also consults the ledger. A previously revoked public key cannot be added again even if an old trusted-device record is restored.

The historical trusted-device JSON does not have to be synchronously deleted for revocation to take effect. The tombstone is the authorization authority. A later successful trust registration compacts tombstoned trusted records before enforcing the 256-device limit.

## Explicit owner action

The native MOBILE view exposes `REVOKE DEVICE` for a currently effective trusted device.

Selecting a device is not enough. The operator must type the exact confirmation word `REVOKE` before the durable tombstone is written.

Revocation is independent of the process-local LAN enable control. Turning LAN intent off or on does not remove or bypass durable revocation.

## Generic Workspace Backup / Restore

The revocation ledger lives under `identity/`, so Generic Workspace Backup carries it without changing the DGMWBK01 archive format.

Restore has an additional security reconciliation step after archive verification/extraction and before any managed root is replaced. DANGOMUSHI parses the currently live revocation ledger and the staged backup ledger, forms their set union, and writes that union into the staged identity root before replacement.

This makes revocation monotonic across restore on the same Workspace: restoring an older snapshot can reintroduce an old trusted-device record, but it cannot remove a fingerprint that has already been revoked locally.

It also means a backup created after revocation carries the tombstone to a different initialized Workspace. A backup created before a future revocation obviously cannot contain that future fact; if both the live Workspace and every post-revocation backup are lost, DANGOMUSHI has no external authority from which to reconstruct it.

If either the live or staged revocation ledger is malformed or unsafe, restore aborts before managed-root replacement. Restore never silently discards a ledger it cannot verify.

## Storage boundary

The ledger is versioned, sorted, duplicate-free, bounded to 4,096 fingerprints and 512 KiB, and rejects symlink or special-file layouts.

Replacement uses create-new pending output, full parse verification, a previous-file rollback slot, file sync and best-effort directory synchronization. Interrupted-write recovery uses the same fail-closed pattern as the trusted-device store.

## Threat boundary

This task does not provide an operating-system or hardware tamper-proof root. A local attacker with arbitrary write access to the Workspace can modify both trusted-device and revocation files.

No permission scopes, reconnect/session policy, non-loopback listener or network authorization is added by this task.
