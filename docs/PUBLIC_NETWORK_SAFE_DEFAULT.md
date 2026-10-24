# Public-network safe default

## Scope

This M3 task defines the network-exposure safety gate that every future non-loopback listener must satisfy. It does not create that listener.

The existing explicit LAN control remains operator intent only. LAN intent by itself is never sufficient authority to expose DANGOMUSHI on a network.

## Fail-closed rule

A future LAN exposure is eligible only when both conditions are true:

1. the process-local LAN control is explicitly enabled for this run, and
2. the current connected-network assessment is trusted.

`Unknown` and `Public` always deny exposure. Probe failure, unsupported platform, zero connected networks, an unexpected platform category, or ambiguous mixed evidence all collapse to a deny result.

The network assessment is ephemeral. It is not persisted to `session.v1`, Workspace config, Generic Workspace Backup, Secret Backup / Restore, or any trust/permission store.

## Windows classification

On Windows, `src/network_safety.rs` queries the operating system's Network List Manager on a fresh COM thread and enumerates connected networks only.

Each connected network is mapped from the operating-system category:

- Public -> public/untrusted
- Private -> trusted
- DomainAuthenticated -> trusted
- anything else or any query failure -> unknown

For a multihomed machine, the aggregate rule is deliberately conservative:

- if any connected network is Public, the aggregate is Public and exposure is denied;
- otherwise, if any connected network is Unknown, the aggregate is Unknown and exposure is denied;
- only when at least one network exists and every connected network is Private or DomainAuthenticated is the aggregate Trusted.

DANGOMUSHI reads the category only. It never changes the Windows network category and it never assumes Windows Firewall configuration from that category.

## macOS and Linux

This task does not invent an equivalent trust classification where the project has not established one. On macOS and Linux the current platform probe returns UnsupportedPlatform / Unknown, which is a deny result.

That means future LAN exposure remains unavailable on those platforms until a separately reviewed platform adapter can provide trustworthy equivalent evidence. Loopback behavior remains available.

## Freshness requirement

A network-safety snapshot is display evidence, not a durable capability.

A future non-loopback transport must call the live safety assessment immediately before binding and again before accepting or authorizing each remote connection. A cached earlier Trusted result must never remain sufficient after the network changes.

If a live assessment becomes Public, Unknown, unsupported, or fails, the future transport must stop admitting new remote work and must not authorize remote operations from an earlier snapshot.

## Current transport boundary

`src/server.rs` remains loopback-only at `127.0.0.1` and is not modified by this task. No wildcard bind, LAN socket, firewall rule, UPnP/NAT-PMP mapping, remote API, TLS policy, cookie/session transport, or mobile handoff endpoint is added.

The next Exactly One Task is Mobile editor/session handoff.
