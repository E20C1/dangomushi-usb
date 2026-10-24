# Encrypted peer transfer

## Scope

M4 encrypted text/file transfer is implemented as a transport-independent, one-shot encrypted-envelope core in `src/peer_transfer.rs`.

It does not add peer discovery, a LAN listener, HTTP/WebSocket routes, firewall changes, filesystem receive/write behavior, relay, NAT traversal, or Internet P2P.

The core assumes M4 trusted peer identity already exists on both sides.

## Cryptographic construction

A receiver begins one receive attempt for one already-trusted sender.

The receiver creates:

- a fresh 256-bit transfer ID from the operating-system CSPRNG,
- a fresh ephemeral X25519 secret/public key pair,
- a signed public offer containing the transfer ID, receiver peer fingerprint, expected sender peer fingerprint, and receiver ephemeral X25519 public key.

The offer is authenticated with the receiver's existing Ed25519 peer identity. It is bound to one expected sender fingerprint and does not create trust by itself.

The sender:

1. verifies that the offer names its own peer fingerprint,
2. verifies the receiver is currently present in the sender's trusted-peer store,
3. verifies the receiver's Ed25519 signature over the offer,
4. creates a fresh ephemeral X25519 key pair,
5. performs X25519 Diffie-Hellman and rejects non-contributory shared secrets,
6. derives a 256-bit transfer key with HKDF-SHA256.

HKDF uses the transfer ID as salt. Its context binds both peer fingerprints and both ephemeral X25519 public keys under a versioned DANGOMUSHI domain string.

The payload is encrypted with XChaCha20Poly1305 using a fresh 192-bit nonce from the operating-system CSPRNG. The complete fixed envelope header is AEAD associated data.

The sender also signs a SHA-256 digest over a separate domain string plus the authenticated envelope header and ciphertext. The receiver verifies that signature against the current trusted sender public key before attempting decryption.

The receiver then performs the matching X25519 exchange, HKDF derivation and XChaCha20Poly1305 decryption.

Ed25519 peer identity remains authentication-only. It is not converted into an X25519 static encryption key.

## Replay and lifetime boundary

A `PendingPeerReceive` owns the receiver's ephemeral X25519 secret and an `Instant` expiry.

The offer lifetime is five minutes.

Opening an encrypted transfer consumes the `PendingPeerReceive` by value. A completed or failed open attempt therefore cannot reuse that receiver ephemeral secret through the safe API.

A replayed envelope cannot be opened against a new pending receive because the transfer ID and receiver ephemeral public key are authenticated into the offer, KDF context, AEAD header and sender signature.

The public offer itself contains no bearer authorization and no reusable session secret. The private pending state is never serialized or persisted.

## Framing and bounds

Both public offers and encrypted envelopes use fixed versioned binary framing with exact-length parsing.

Unknown versions, truncated frames, trailing bytes, impossible lengths and over-limit payloads fail closed.

Current limits:

- text payload: 4 MiB UTF-8 maximum,
- file payload: 64 MiB maximum,
- suggested file name: 255 UTF-8 bytes maximum,
- one encrypted envelope contains exactly one text or file payload.

The decrypted inner payload has its own magic/version/type/length framing.

## File destination boundary

A transferred file carries only a suggested single-component UTF-8 name and file bytes.

Suggested names reject empty names, `.`, `..`, control characters, forward slashes and backslashes.

This task never maps that name to a filesystem destination and never writes a received file. The receiver returns an in-memory `ReceivedTransfer::File` only.

Destination selection, collision handling, durable receive semantics and integrity verification of a received file remain outside this task.

## Integrity semantics

XChaCha20Poly1305 authenticates the encrypted payload and fixed envelope header for the derived transfer key. The sender Ed25519 signature authenticates the sender peer identity over the envelope digest.

This provides cryptographic transport authenticity/integrity for this one encrypted envelope.

The next roadmap task, **Integrity verification for received files**, remains separate. It must define the file-level verification record and receiver-side verification semantics before a received file is durably accepted or written.

## Persistence and logs

Transfer IDs, ephemeral X25519 secrets, derived keys, nonces, plaintext transfer contents, ciphertext envelopes and pending receive state are not written to:

- `session.v1`,
- Generic Workspace Backup,
- Secret Backup / Restore,
- trusted-peer policy,
- M3 trusted-device state,
- autosave,
- diagnostic logs.

The core returns values directly to its caller. Derived transfer keys are stored in zeroizing buffers, and x25519-dalek ephemeral/shared secrets are dropped after use.

## Transport boundary

`src/server.rs` remains loopback-only.

A future Direct LAN task may transport the already-defined offer/envelope bytes, but it must not weaken current trusted-peer checks or M3 public-network safety rules. This task does not make any non-loopback network path available.
