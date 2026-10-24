# Trusted peer identity

## Scope

M4 begins by establishing a transport-independent identity and trust boundary for communication between DANGOMUSHI peers.

This task does not add a LAN listener, peer discovery, multicast, HTTP/WebSocket endpoint, encrypted transfer protocol, file receiver, relay, NAT traversal, or Internet P2P path.

M3 trusted-device identity remains a separate authority. A trusted mobile/client device is not automatically a trusted DANGOMUSHI peer, and a trusted peer is not automatically authorized for M3 mobile/session operations.

## Local peer identity

A DANGOMUSHI Workspace can explicitly initialize one Ed25519 peer identity.

The private 32-byte seed is generated from the operating-system CSPRNG and stored in:

`workspace/state/peer_identity.v1`

The fixed binary record also stores the derived Ed25519 public key and a domain-separated SHA-256 peer fingerprint. Every load recomputes the public key and fingerprint from the private seed and fails closed if they do not match.

Initialization is create-once. Existing identity is never silently replaced or rotated. A verified pending record can be promoted after an interrupted first initialization, but ambiguous target-plus-pending layouts are rejected instead of guessed at.

On Unix, the file is created with mode 0600. This is not encryption at rest. An attacker who can read the Workspace state files can recover the peer private key and impersonate that peer. The current task establishes network identity continuity, not hardware-backed key protection.

The peer private key is intentionally kept outside `workspace/identity/`. Generic Workspace Backup includes `identity/`, but only `state/autosave/` from `state/`. Consequently, Generic Backup / Restore does not copy the peer private key into a snapshot and does not replace it during restore.

## Peer proof of possession

Peer verification uses a fresh 256-bit challenge generated from the operating-system CSPRNG.

The challenge is valid for at most five minutes and is consumed by the verification API. The peer signs a domain-separated transcript containing:

- the fresh challenge,
- the claimed Ed25519 public key,
- the exact validated UTF-8 peer display name.

Verification uses Ed25519 strict verification. A successful result becomes a `VerifiedPeerCandidate`; raw network address, hostname, display name, or public key alone never creates this type.

The challenge signature proves possession of the submitted private key for that verification exchange. It is not a session-encryption key and does not itself trust the peer.

## Explicit trusted-peer policy

Persistent peer trust requires a separate explicit call using a `VerifiedPeerCandidate`.

Trusted peer policy is stored in:

`workspace/state/trusted_peers.v1.json`

Each record contains only:

- peer display name,
- Ed25519 public key,
- domain-separated SHA-256 peer fingerprint.

The store is bounded, versioned, duplicate-free, rejects malformed or mismatched fingerprints, rejects symlink/special-file layouts, and uses verified pending/previous replacement with file sync.

Trust is not TOFU. Discovery, an IP address, a hostname, or receiving a public key does not automatically persist trust.

Removing a trusted peer deletes that key from the current trusted-peer policy. Re-trusting the same key requires a new proof-of-possession flow and a new explicit trust action.

Trusted-peer policy is also outside Generic Workspace Backup. Therefore restoring an older generic backup cannot silently resurrect a peer that was removed after the backup was created.

This task does not create a separate monotonic peer-revocation tombstone ledger. The non-backed-up trusted-peer policy is the current authority, and filesystem write compromise remains outside this boundary.

## Separation from M3 device identity

M3 device fingerprints and M4 peer fingerprints use different domain-separation strings even though both currently use Ed25519 public keys.

The stores are also separate:

- M3 trusted devices: `workspace/identity/trusted_devices.v1.json`
- M4 local peer identity: `workspace/state/peer_identity.v1`
- M4 trusted peers: `workspace/state/trusted_peers.v1.json`

No M3 permission scope is granted to a peer by being present in the trusted-peer store.

## Transport boundary

The new core only establishes local identity, proof-of-possession, and explicit peer trust.

`src/server.rs` remains loopback-only at `127.0.0.1`. No peer socket is opened by this task.

The next M4 task is Encrypted text/file transfer. That task must authenticate the remote party against this peer-identity boundary and define separate confidentiality, integrity, replay, framing, size, destination, and failure semantics before a non-loopback transfer path is enabled.
