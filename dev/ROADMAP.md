# textmate-lang - Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
> Master plan: ../_lexersketch/ROADMAP.md and ../_lexersketch/NEW-LIBS.md
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

## v0.1.0 - Scaffold (DONE)
Compiles, CI green, structure correct, no domain logic.
- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt, DIRECTIVES, ROADMAP.

## v0.2.0 - Foundation
- [ ] Grammar model (scopes, patterns, begin/end, includes, repositories), validation, JSON emission with escaping.

## v0.5.0 - Implementation
- [ ] Semantic-token legend generation; embedded-language injection; scope naming conventions.

## v0.9.0 - Hardening
- [ ] Fuzzing emission for injection; validation against real editor loaders (dev-only).

## v1.0.0 - Stable
- [ ] Frozen after the LexerSketch VS Code extension ships grammars from it (D18).
