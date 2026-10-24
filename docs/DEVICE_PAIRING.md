# Device Pairing

## Scope

The second M3 network task implements the process-local pairing protocol core without opening a LAN listener or writing a trusted-device record.

A pairing session can begin only while the explicit LAN enable control is ON. Turning that control OFF clears the native run-local pairing state.

## Pairing invitation

Each session generates independent values from the operating-system CSPRNG:

- a 16-character code from `ABCDEFGHJKLMNPQRSTUVWXYZ23456789` displayed as four groups of four characters, carrying 80 bits of entropy,
- a 32-byte public challenge.

The session expires after five minutes. At most five cryptographic proof failures are accepted before the session is discarded.

The pairing code is secret and process-local. It is not written to session state, Workspace config, backups, identity storage, logs, or the embedded web gateway.

## Client proof

The future mobile client removes ASCII spaces and hyphens from the displayed code, uppercases ASCII letters, and uses the resulting 16 ASCII bytes directly as the HMAC-SHA256 key.

The proof transcript is the byte concatenation:

~~~text
ASCII "DANGOMUSHI-PAIRING-V1\0"
32-byte pairing challenge
32-byte Ed25519 public key
2-byte unsigned big-endian UTF-8 device-name length
exact UTF-8 device-name bytes
~~~

The device name must be 1 to 64 UTF-8 bytes, contain no control characters, and have no leading or trailing whitespace.

The client computes HMAC-SHA256 over that transcript. The raw pairing code therefore does not need to be a future LAN request field.

The Ed25519 signature message is:

~~~text
ASCII "DANGOMUSHI-PAIRING-SIGNATURE-V1\0"
pairing transcript
32-byte HMAC proof
~~~

The client signs that message with the private key corresponding to the submitted 32-byte Ed25519 public key.

## Server verification

The core verifies the HMAC proof and then verifies the Ed25519 signature with `ed25519-dalek` strict verification. Cryptographic failures share one `ProofRejected` result so callers do not receive separate HMAC-versus-signature failure detail.

A valid proof establishes that the submitting device knows the short-lived code and possesses the private key for the claimed public key. The proof is bound to the random session challenge, device name and public key.

## Explicit desktop approval

Successful cryptographic proof moves the process-local state to `AwaitingApproval`. It does not create durable trust.

The native owner must explicitly approve the candidate. Approval returns a `PairedDeviceCandidate` containing the exact device name, the 32-byte Ed25519 public key and a domain-separated SHA-256 fingerprint.

The native shell may retain that candidate only for the current run. Persistent storage of trusted-device public keys is the next M3 task.

## Network boundary

`src/server.rs` remains unchanged and loopback-only. This task adds no pairing HTTP route, LAN bind, UPnP/NAT-PMP behavior, reverse tunnel, firewall change or public relay.

The future transport adapter must use this pairing core rather than weakening or bypassing its expiry, proof, attempt and approval boundaries.
