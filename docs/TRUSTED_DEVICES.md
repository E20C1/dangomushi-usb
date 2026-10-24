# Persistent Trusted-Device Public Keys

## Scope

This M3 task promotes an explicitly approved run-local pairing candidate into durable public-key trust.

The canonical store is `workspace/identity/trusted_devices.v1.json`.

Each record contains only:

- the user-visible device name,
- the 32-byte Ed25519 public key encoded as hexadecimal,
- the domain-separated SHA-256 public-key fingerprint encoded as hexadecimal.

Pairing codes, pairing challenges, HMAC proofs, Ed25519 signatures, private keys, LAN-control state and generic session state are not persisted in this file.

## Explicit trust transition

Successful cryptographic pairing and run-local approval still do not silently create durable trust.

The native MOBILE view exposes a separate `TRUST DEVICE PERSISTENTLY` action after run-local pairing. Only that action writes the approved public key into the trust store.

Repeated registration of the same public key or fingerprint is rejected rather than creating duplicate trust records.

## Storage boundary

The store is versioned and bounded to 256 devices and 256 KiB.

Every read validates the format version, store kind, device-name rules, exact public-key/fingerprint lengths, the fingerprint recomputed from each public key, and duplicate-key/fingerprint absence.

`identity/`, the committed file, pending file and previous rollback file must be real filesystem entries of the expected type; symlink or special-file layouts are rejected.

Replacement writes a create-new pending file, syncs it, fully parses and verifies it, retains the previous committed file while replacement occurs, verifies the new committed file, and only then removes the previous file.

Interrupted-write recovery is fail-closed: a pending-only first write is discarded, an existing committed store wins over an uncommitted pending file, and a verified previous store is restored when the replacement target is missing or invalid.

## Backup and restore

`workspace/identity/` is already included in Generic Workspace Backup, so trusted-device public keys are included in generic snapshots and may be restored with them.

An older backup can therefore restore an older trust set. There is still no LAN listener at this milestone. The following Device revocation task must define fail-closed revoked-key behavior across backup restore before network authorization is allowed to depend on this store.

## Threat boundary

Public keys are identity/authorization material but are not secret values.

The store does not claim protection from an attacker who already has arbitrary write access to the Workspace filesystem. It establishes bounded durable identity with corruption and layout checks, not a hardware-backed local trust root.

No remove, revoke, permission-scope, LAN-listener or network-authorization operation is added by this task.
