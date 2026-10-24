# Received file integrity

## Scope

M4 received-file integrity adds a file-level verification gate on top of the encrypted peer-transfer envelope.

The encrypted-transfer boundary already authenticates and encrypts the complete transfer envelope. This task adds a separate sender-authenticated commitment to the exact file bytes so a receiver can verify the file payload itself before any future durable acceptance or filesystem write.

This task does not add a LAN listener, peer discovery, filesystem receiver, automatic save path, overwrite behavior, quarantine directory, malware scanning, relay, NAT traversal, or Internet P2P.

## Sender-authenticated file commitment

For every file transfer, the sender computes SHA-256 over the raw file bytes before encryption.

The inner file payload carries:

- payload format version,
- file payload type,
- validated suggested filename,
- declared file byte length,
- integrity algorithm identifier fixed to SHA-256,
- 32-byte SHA-256 digest,
- raw file bytes.

The integrity record is inside the existing XChaCha20Poly1305 ciphertext. The entire encrypted envelope is also authenticated by the sender's Ed25519 peer identity.

Consequently, the declared length and expected SHA-256 are not unauthenticated side metadata: after the existing envelope checks succeed, they are an authenticated commitment made by the trusted sender identity for that transfer.

A malicious or compromised trusted sender can still send harmful bytes together with a matching digest. SHA-256 verification proves equality with the authenticated sender commitment; it is not malware detection, code-signing validation, or a statement that the content is safe.

## Pending versus verified file states

A decrypted file becomes `PendingReceivedFile`, not directly usable file bytes.

`PendingReceivedFile` exposes only non-content verification metadata:

- suggested filename,
- declared byte length,
- expected SHA-256,
- sender peer fingerprint.

Its raw file byte buffer is private.

Calling `verify(self)` consumes the pending object. Verification first checks that the actual byte length equals the authenticated declared length, then recomputes SHA-256 over the in-memory bytes and compares it with the authenticated expected digest.

Length or digest mismatch fails closed.

Only successful verification produces `VerifiedReceivedFile`. That verified type exposes the file bytes, verified length, verified SHA-256, suggested filename and sender fingerprint.

The type boundary is deliberate: callers using the safe public API cannot retrieve file bytes from a `PendingReceivedFile` without passing the file-level verification step.

## Relationship to encrypted transfer integrity

There are now two distinct integrity layers:

1. XChaCha20Poly1305 plus the sender Ed25519 envelope signature authenticates the encrypted transport envelope, identities, framing and ciphertext.
2. The file SHA-256 record verifies the exact decrypted file bytes against the authenticated sender commitment.

The second layer is useful for future durable receive logic because the digest can remain the acceptance reference after the encrypted envelope has been processed.

This task still keeps all file bytes in memory. A future filesystem receiver must re-check the verified digest against the bytes it actually makes durable if it introduces any write path where bytes could change between verification and durable acceptance.

## Bounds and framing

The existing transfer limits remain authoritative:

- file bytes: maximum 64 MiB,
- suggested filename: maximum 255 UTF-8 bytes,
- one file per encrypted transfer envelope.

The inner payload format is advanced to version 2 for the authenticated SHA-256 record. Unknown algorithms, old/unknown payload versions, truncation, trailing bytes, impossible lengths and oversized data fail closed.

Text transfer remains unchanged semantically and does not receive a file-integrity record.

## Persistence boundary

Neither `PendingReceivedFile` nor `VerifiedReceivedFile` writes to disk.

Received file bytes, expected digests, verified digests and verification state are not persisted to:

- `session.v1`,
- autosave,
- Generic Workspace Backup,
- Secret Backup / Restore,
- trusted-peer policy,
- M3 trusted-device state,
- diagnostic logs.

The transfer core still returns values directly to its caller.

## Transport boundary

`src/server.rs` remains loopback-only.

No peer socket is opened by this task. The next M4 task is **Direct LAN path first**, which may transport the already-defined trusted-peer offer/envelope protocol only if it preserves the existing public-network safety, peer-trust, encryption and received-file verification boundaries.
