# Internet P2P Threat Review

## Decision

**Decision: DEFER / DISABLE Internet P2P in the current DANGOMUSHI roadmap.**

M4 is considered complete with trusted peer identity, encrypted text/file transfer, received-file integrity verification, and the explicit direct-LAN path.

The threat review does **not** authorize a public listener, global peer address, rendezvous service, relay, NAT traversal, automatic port mapping, public discovery, or Internet peer transport.

The repository-level policy is represented by `src/internet_p2p_policy.rs`, whose reviewed decision remains fail-closed:

`DISABLED / THREAT REVIEW COMPLETE`

There is no runtime toggle, environment variable, CLI option, Cargo feature, saved setting, or UI control that can enable Internet P2P.

## Why Direct LAN does not automatically extend to the Internet

The current direct-LAN transport intentionally operates inside a narrower trust and exposure model:

- explicit process-local LAN intent,
- fresh current-network safety checks,
- IPv4 loopback/private/link-local targets only,
- no global addresses,
- no peer discovery,
- no firewall mutation,
- no UPnP/NAT-PMP,
- no relay or rendezvous dependency,
- one-shot bounded receive windows,
- existing trusted-peer identity,
- end-to-end encrypted transfer,
- file verification before acknowledgement.

Internet P2P would not be a small address-range expansion. It would introduce new network authority, new metadata, new denial-of-service surfaces, new availability dependencies, and potentially third-party infrastructure.

## Assets to protect

A future Internet path would need to protect at least:

- long-lived Ed25519 peer identity keys,
- trusted-peer policy,
- peer IP addresses and network topology,
- transfer metadata such as timing and approximate size,
- encrypted transfer payloads,
- received-file verification state,
- local network classification and interface information,
- user intent about whether the machine should be reachable,
- availability of the desktop application and host network,
- the local-first property that core operation does not depend on public infrastructure.

The existing end-to-end transfer encryption protects content confidentiality and authenticity against many network attackers, but it does not by itself solve endpoint exposure, discovery privacy, traffic analysis, denial of service, abuse, or relay operations.

## Threat actors

The review assumes the following adversaries may exist:

1. **Unauthenticated Internet scanners** that can discover or repeatedly connect to a public endpoint.
2. **On-path observers** that can see endpoint IPs, timing, packet sizes, connection attempts, and service availability.
3. **Malicious or compromised trusted peers** that possess a legitimately trusted peer identity.
4. **Compromised rendezvous or relay infrastructure** able to observe metadata, suppress traffic, correlate peers, replay control messages, or return malicious routing information.
5. **Attackers controlling NAT/router state** or exploiting automatic port-mapping behavior.
6. **Resource-exhaustion attackers** attempting connection floods, oversized framing, slow reads/writes, CPU exhaustion, or memory pressure.
7. **Endpoint compromise** where a local peer private key or trusted-peer store is stolen.
8. **Downgrade/version-confusion attackers** attempting to force older framing, weaker transport behavior, or unsafe compatibility paths.

## Threat review

### Public listener and unsolicited reachability — HIGH

A globally reachable listener becomes discoverable by arbitrary Internet hosts and creates a persistent attack surface even when application payloads remain cryptographically authenticated.

The current one-shot LAN listener is bounded and protected by network classification. A future Internet listener would need an explicit and separate reachability model, anti-scanning/anti-DoS controls, rate limiting, connection quotas, and a clear lifecycle.

**Current decision:** do not bind a public/global listener.

### Global address support — HIGH

Allowing global IPv4/IPv6 addresses would bypass the current address-class restriction that prevents accidental public targeting.

It would also require resolving multi-homing, IPv6 scope, interface selection, DNS provenance, stale addresses, endpoint substitution, and user-visible target verification.

**Current decision:** direct peer APIs continue rejecting global, CGNAT, and IPv6 targets.

### NAT traversal and hole punching — HIGH

STUN/ICE-style traversal can reveal public endpoint metadata and creates complex behavior across symmetric NAT, carrier-grade NAT, multiple interfaces, VPNs, and firewall policies.

A traversal implementation also creates downgrade and confusion risks if peers disagree about candidate provenance or route selection.

**Current decision:** no NAT traversal or hole punching.

### Automatic router/firewall mutation — CRITICAL

UPnP, NAT-PMP, PCP, OS firewall rules, or equivalent automatic exposure can make the host reachable beyond the user's expected boundary and can outlive a single application operation.

**Current decision:** DANGOMUSHI must not automatically create router mappings or firewall exceptions.

### Rendezvous/discovery service — HIGH

A public rendezvous system can learn who is online, approximate location through IP address, connection timing, and peer relationship metadata. It also becomes an availability and abuse target.

A malicious rendezvous service could attempt peer substitution, traffic correlation, censorship, or downgrade. Existing peer fingerprints reduce substitution risk, but do not eliminate metadata/privacy or availability risks.

**Current decision:** no public peer directory, discovery service, or rendezvous dependency.

### Relay service — HIGH

A TURN-like or custom relay can observe peer IPs, timing, transfer size and relationship metadata even when payloads remain end-to-end encrypted.

Operating a relay also introduces bandwidth cost, abuse handling, rate limiting, retention/logging policy, legal/operational obligations, and service availability dependencies that conflict with the project's current local-first scope.

**Current decision:** no project-operated or third-party relay integration.

### Denial of service and resource exhaustion — HIGH

Internet attackers could repeatedly trigger connection establishment, signature verification, ephemeral key exchange, frame allocation, slow I/O, or pending receive state.

The existing payload bounds and timeouts help but are not a complete Internet-facing anti-DoS strategy.

**Current decision:** do not expose the peer protocol to arbitrary Internet initiators.

### Malicious trusted peer — HIGH

Cryptographic authentication proves which trusted key sent data; it does not make that peer benign.

A trusted peer can intentionally send the maximum allowed payload repeatedly, send harmful content with a matching SHA-256 digest, or attempt social-engineering through filenames and transfer timing.

**Current decision:** future Internet work would still require explicit per-operation user intent, rate/resource controls, and durable trust/revocation semantics. File integrity is not malware approval.

### Identity-key compromise — HIGH

Stealing `workspace/state/peer_identity.v1` permits peer impersonation because the current key is software-stored and not hardware-backed.

Internet reachability increases the usefulness of a stolen identity and may make impersonation attempts possible from anywhere.

**Current decision:** do not broaden reachability while peer identity remains software-held without an Internet-specific compromise/recovery design.

### Metadata and traffic analysis — MEDIUM/HIGH

End-to-end encryption does not hide source/destination IP addresses from network observers, nor does it fully hide timing or approximate ciphertext size.

A relay or rendezvous system would add another observer capable of correlation.

**Current decision:** no claim of anonymity or metadata privacy; do not add Internet infrastructure without a documented metadata policy.

### Replay, downgrade and protocol evolution — MEDIUM/HIGH

The current one-shot transfer IDs and ephemeral keys resist envelope replay, but Internet deployment would need explicit protocol-version negotiation rules, downgrade resistance, stale-client policy, and compatibility behavior across independently updated peers.

**Current decision:** no Internet compatibility layer is introduced in M4.

### Availability and local-first dependency — MEDIUM/HIGH

A hosted rendezvous/relay can become a central dependency and outage point. It can also create pressure to require an account or cloud service.

That conflicts with the current product goal that essential operation remains usable without a public Internet service.

**Current decision:** core peer communication remains direct LAN and cloud-independent.

## Preconditions before Internet P2P can be reconsidered

A future milestone may reopen this decision only after a new, explicit security design covers all of the following:

- a user-visible opt-in that defaults OFF and is not silently persisted into exposure,
- a precise public-listener lifecycle with no background always-on reachability by default,
- an authenticated rendezvous design or an explicit decision to avoid rendezvous,
- relay trust, privacy, retention, bandwidth, abuse and availability policy if relays are considered,
- explicit NAT traversal behavior and candidate provenance,
- a continued prohibition or separately reviewed policy for automatic router/firewall mutation,
- IPv4/IPv6/global-address handling and interface-selection rules,
- rate limits, connection/concurrency quotas, memory/CPU bounds, slow-client handling and backoff,
- protocol versioning and downgrade resistance,
- peer compromise/revocation/re-key/recovery behavior suitable for Internet reachability,
- metadata/privacy disclosure and log-redaction requirements,
- adversarial tests for scanning, malformed framing, replay, connection flood, slow I/O and stale trust,
- independent review of any new cryptographic handshake or transport change,
- confirmation that local-only/offline operation remains fully functional without rendezvous/relay infrastructure.

Completing those prerequisites would permit a new implementation task to be proposed. This threat review alone does not authorize implementation.

## Current code-level guardrails

The current repository retains the following fail-closed properties:

- `peer_lan::send_transfer` rejects global IPv4, CGNAT and all IPv6 targets,
- the direct-LAN receive path accepts only loopback/private/link-local IPv4 peers,
- `src/server.rs` remains loopback-only,
- no public discovery, rendezvous, relay, NAT traversal or router/firewall mutation exists,
- `internet_p2p_policy::internet_p2p_enabled()` is always false,
- no runtime configuration can override that reviewed policy.

## Roadmap consequence

The threat-review task is complete, but **Internet P2P is not implemented**.

M4 therefore closes with Direct LAN as its supported peer transport. The roadmap advances to M5 Offline continuity.

If Internet P2P is reconsidered later, it must return as a new scoped milestone after the prerequisites above are satisfied and reviewed.
