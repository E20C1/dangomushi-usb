# Password Generator Core

Status: implemented as an M2-S core module; no UI or persistence is attached.

## Boundary

The generator is intentionally independent of:

- native UI,
- Tool Registry exposure,
- Credential persistence,
- Vault storage,
- encryption,
- Recovery Codes,
- Clipboard handling,
- generic Text Editor/autosave/session persistence.

Generated values are returned in the in-memory `GeneratedPassword` object. A crate-private consuming ownership-transfer method now lets the dedicated generated-Credential integration boundary move the owned String into `SecretString` without requiring a clone.

`GeneratedPassword` does not implement `Display`, and its `Debug` implementation is redacted. This reduces accidental diagnostic disclosure but is not a claim of complete memory erasure. The broader limitations in `SECRET_DATA_BOUNDARY.md` continue to apply.

## Production randomness

Production generation uses the `getrandom` crate as a thin cross-platform interface to the operating system's preferred random source.

The dependency was selected for this task because its official documentation describes system-provided cryptographically secure randomness and support for the project's Windows, macOS and Linux targets. It remains a random-source dependency only; DANGOMUSHI is not implementing a cryptographic primitive here.

## Configuration

`GeneratorConfig` supports:

- length,
- uppercase,
- lowercase,
- digits,
- symbols,
- configurable symbol set,
- optional minimum one character from every selected group,
- ambiguous-character exclusion.

The current ambiguous set is:

~~~text
0 O 1 I l |
~~~

Custom symbols are restricted to printable ASCII punctuation. Duplicate custom symbols are removed.

## Bias avoidance

Character selection uses rejection sampling rather than directly applying modulo to arbitrary random bytes.

When every selected group is required, the core first selects one character from each group, fills the remaining positions from the combined pool, then applies a Fisher-Yates shuffle whose indices use the same rejection-sampled source. This prevents required groups from being pinned to predictable positions.

## Deterministic tests

Core generation accepts an injected `RandomSource` implementation.

Production calls use `OsRandom`. Tests use deterministic sources so configuration, required-group behavior, ambiguous-character filtering, source failure and modulo-bias rejection can be checked without introducing a seeded pseudo-random generator into production.

Tests do not log generated values or use generated values as credential fixtures.

## Limits

The core currently accepts lengths from 1 through 4096 bytes/characters. Character groups are ASCII, so generated String length and character count are identical.

No generated value is persisted by this module.

## Integration status

The generator core remains UI/storage independent. Generated-password convergence on the source-agnostic Credential model is implemented separately in `GENERATED_CREDENTIAL_INTEGRATION.md`.
