# Device Reconnection

## Scope

This M3 task allows a persistently trusted device to authenticate a new connection without repeating the short-lived human pairing ceremony and without creating a short fixed authenticated-session expiry.

It does not add a LAN listener or remote HTTP/API route.

## Fresh proof on every connection

A device reconnect starts only while the explicit process-local LAN control is enabled and only for an effectively trusted, non-revoked device fingerprint.

DANGOMUSHI generates a fresh 256-bit challenge from the operating-system CSPRNG. The challenge is public, one-time by Rust ownership, and valid for two minutes only to bound an abandoned handshake.

The device signs the domain-separated reconnect transcript with its already trusted Ed25519 private key. The transcript binds the reconnect protocol domain, the fresh 256-bit challenge, and the trusted-device fingerprint.

`complete` consumes the challenge object, re-reads effective trust after the challenge was issued, and uses Ed25519 strict verification. A revocation that occurs between challenge issuance and completion therefore makes the proof fail as not trusted.

The short challenge lifetime is a handshake anti-staleness boundary. It is not an authenticated-session lifetime.

## No durable bearer token

Successful proof returns an in-process `AuthenticatedDeviceSession` containing only the device fingerprint and display name.

The authenticated session has no fixed expiry field, access token, refresh token, cookie secret, shared secret, or persisted session identifier. Nothing from reconnection is written to `session.v1`, Workspace config, Generic Workspace Backup, Secret Backup / Restore, or another durable store.

A future transport owns the lifetime of this process-local principal. Closing the connection discards it. Reconnecting later requires another fresh challenge and Ed25519 proof, but does not require a new pairing code while the same public key remains trusted.

This avoids both five-minute-style authenticated sessions and indefinitely reusable bearer credentials.

## Live revocation and permission checks

`AuthenticatedDeviceSession::is_currently_valid` re-reads effective trusted-device state and also requires LAN intent to remain enabled.

`AuthenticatedDeviceSession::authorize` performs that validation and then calls the current default-deny permission policy. Scopes are not snapshotted into the authenticated session.

Therefore disabling LAN intent invalidates use of the principal, revoking the device invalidates use of an already authenticated principal, removing a permission immediately removes that authorization, and adding a permission can take effect without issuing a new long-lived token.

Failing to verify trusted-device or permission state never produces an allow result.

## Threat and transport boundary

Replay of a signature against another reconnect attempt fails because every attempt uses a fresh random challenge and the challenge is signed.

This milestone does not define network framing, cookies, TLS, public-network classification, origin policy, connection concurrency, or mobile editor/session-handoff endpoints.

The embedded web gateway remains loopback-only. Public-network safe default is the next M3 task.
