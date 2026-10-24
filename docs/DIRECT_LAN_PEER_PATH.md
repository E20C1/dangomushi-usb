# Direct LAN peer path

## Scope

M4 Direct LAN path first adds the first non-loopback DANGOMUSHI peer transport.

The transport is intentionally narrow:

- direct TCP only,
- IPv4 only,
- loopback, RFC1918 private IPv4, and IPv4 link-local targets only,
- one inbound transfer per receive window,
- no discovery,
- no broadcast or multicast,
- no firewall mutation,
- no UPnP or NAT-PMP,
- no relay,
- no Internet P2P,
- no durable received-file write.

The existing loopback HTTP status gateway in `src/server.rs` is unchanged and remains bound to `127.0.0.1`.

## Explicit LAN intent and live network safety

`src/peer_lan.rs` composes the process-local `LanEnableControl` with the existing live `network_safety` policy.

Direct LAN is denied unless:

1. LAN intent has been explicitly enabled for this process run, and
2. a fresh current-network assessment is eligible.

The production path calls `network_safety::current_exposure_decision`. It does not accept a caller-supplied cached assessment.

On Windows, this means all currently connected Network List Manager entries must still classify as Private and/or DomainAuthenticated. Any Public, Unknown, probe failure, or empty/unsupported evidence blocks the operation.

macOS and Linux still fail closed because their current network-safety adapter remains UnsupportedPlatform/Unknown.

The receiver re-probes:

- immediately before binding,
- repeatedly while waiting for a connection,
- immediately after accepting a connection,
- again before issuing the signed receive offer,
- immediately before authenticating/opening the encrypted payload,
- once more before sending the final success acknowledgement.

The sender re-probes:

- before connecting,
- immediately after connecting,
- again before sealing/sending the encrypted envelope.

A previously trusted snapshot is never treated as a durable capability.

## One-shot receive window

`peer_lan::receive_one` opens a temporary IPv4 wildcard TCP listener for a bounded receive window only.

The maximum receive window is five minutes. The listener is nonblocking and the code re-checks the live network-safety gate every 50 ms while waiting.

After the first accepted connection, the listener is dropped before the connection is processed. The API does not create a persistent background listener.

If the receive window expires without a connection, the listener is dropped and `None` is returned.

The default peer LAN port is 45822.

Binding to the IPv4 wildcard address is permitted only after the fresh LAN-safety gate succeeds. DANGOMUSHI does not create or modify operating-system firewall rules, so platform firewall policy remains independently authoritative.

## Direct-address restriction

Outbound target addresses and accepted peer addresses must be IPv4 and one of:

- loopback,
- 10.0.0.0/8,
- 172.16.0.0/12,
- 192.168.0.0/16,
- 169.254.0.0/16 link-local.

Global IPv4, CGNAT, multicast, unspecified, and IPv6 targets are not part of this first direct-LAN transport and are rejected by the transport API.

Loopback is retained for local testing and same-host peer use. It does not weaken the production non-loopback network-safety gate because LAN intent and the fresh assessment are still required by the public API.

## Wire protocol

The direct LAN protocol is a small bounded binary framing layer around the existing M4 encrypted transfer.

The sender first transmits a fixed hello containing:

- protocol magic/version,
- its trusted-peer fingerprint.

The receiver treats that fingerprint only as a selector for the expected sender. It re-reads the trusted-peer store and rejects unknown fingerprints before generating an offer.

The receiver then creates the existing signed one-shot `PeerTransferOffer` bound to that expected sender and sends it in a length-bounded frame.

The sender calls `peer_transfer::seal_transfer_to`, which verifies:

- the signed receiver offer,
- the receiver remains trusted,
- the offer's recipient fingerprint matches the caller's explicitly expected recipient fingerprint.

This prevents an address mix-up from silently redirecting a send to a different trusted peer.

The sender returns the existing encrypted transfer envelope. The receiver re-checks current LAN safety and then passes the envelope into `peer_transfer::open_transfer`.

No plaintext text or file payload is transmitted outside the existing X25519 + HKDF-SHA256 + XChaCha20Poly1305 + Ed25519 envelope.

## Received file verification before acknowledgement

Text transfers are returned only after the existing sender-authenticated encrypted transfer succeeds.

File transfers must additionally pass `PendingReceivedFile::verify`. The receiver does not send a success acknowledgement until the file has become `VerifiedReceivedFile`.

Therefore the direct LAN transport never acknowledges a file whose authenticated declared length or SHA-256 does not match the received bytes.

Even after verification, this task does not write the file to disk.

## Timeouts and framing bounds

TCP connect timeout is five seconds.

Accepted TCP streams use ten-second read and write timeouts.

Protocol frames have fixed magic values and bounded length fields. The offer frame is required to be exactly the existing fixed offer size. The encrypted-envelope frame is rejected before allocation when its declared length exceeds `MAX_ENCRYPTED_TRANSFER_BYTES`.

Unknown magic, malformed hello/version, zero-length frames, oversized frames, truncated frames, failed trust lookup, failed cryptographic verification, failed file integrity verification, or missing acknowledgement all fail closed.

No detailed cryptographic failure reason is sent to an unauthenticated remote peer.

## Persistence boundary

Direct LAN transport state is process-local.

The transport does not persist:

- connection addresses,
- hello frames,
- encrypted envelopes,
- plaintext messages,
- file contents,
- acknowledgements,
- transfer keys,
- network-safety assessments.

No new fields are added to `session.v1`, Generic Workspace Backup, Secret Backup / Restore, trusted-peer policy, or M3 device state.

## Internet boundary

This task does not make global-IP or Internet peer communication possible.

The separate Internet P2P threat review is now complete. Its decision is **DEFER / DISABLE Internet P2P** for the current roadmap. No relay, NAT traversal, global listener, rendezvous, public discovery, automatic port mapping, or Internet transport implementation is authorized. M4 closes with Direct LAN as its supported peer transport; future Internet P2P work requires a new scoped milestone satisfying the review prerequisites.
