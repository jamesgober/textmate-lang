# textmate-lang - Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
> Master plan: ../_lexersketch/ROADMAP.md and ../_lexersketch/NEW-LIBS.md
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

## v0.1.0 - Scaffold (DONE)
Compiles, CI green, structure correct, no domain logic.
- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt, DIRECTIVES, ROADMAP.

## v0.2.0 - Foundation (DONE)
- [x] Grammar model (scopes, patterns, begin/end, includes, repositories), validation, JSON emission with escaping.

Delivered:
- `Grammar` (scope name, display name, file types, `firstLineMatch`,
  `injectionSelector`, patterns, repository, injections; `validate`,
  `to_json`, `to_json_compact`, `write_json` + `Style`), `Rule` (includes of
  `#name`, `$self`, `$base`, `scope`, `scope#name`; pattern lists), `Match`,
  `BeginEnd`, `BeginWhile`, `Capture`, `Error`, `RegexError` +
  `RegexErrorKind`, `validate_regex`, `escape_regex`, `MAX_RULE_DEPTH`,
  `MAX_REGEX_DEPTH`.
- Oniguruma subset validator: one iterative pass, byte offsets, documented in
  `docs/API.md#regex-subset`; models isolated `(?imx)` the way Oniguruma
  parses it, extended-mode comments, fixed-length look-behind, and VS Code's
  `end`/`while` back-reference substitution.
- Scope-name, selector, repository-name, capture, include, and include-cycle
  validation; errors located by JSON Pointer.
- Own JSON writer (pretty and compact), deterministic, escaping every string
  including U+2028/U+2029.
- Property tests against a strict RFC 8259 reader and reference
  implementations (expected-JSON construction, regex syntax trees,
  reachability for cycles, scope rules); benchmarks at 100,000 entries.

Dependency wiring (decided here, recorded per the anti-deferral rule):
- **No first-party crate is wired.** treesitter-lang is the sibling emitter,
  but its grammar model (tree-sitter rules) and its JSON helpers are private
  and shaped for `grammar.json`; nothing in its public API is reusable here.
  grammar-lang's regex parser reads Rust `regex` syntax, not Oniguruma, and
  is private. syntax-lang, span-lang, and diag-lang would add nothing: this
  crate reports positions inside its own output (JSON Pointers and pattern
  byte offsets), not inside source files. The JSON writer stays inside the
  crate per the task; it moves to `json-io` if and when that crate exists
  (NEW-LIBS infrastructure).
- **Semantic-token legends were never part of 0.2.0**; they stay in 0.5.0 as
  planned below.
- **Fuzz target (DIRECTIVES §3.6):** the regex checker and the selector and
  scope checkers are parsers. 0.2.0 covers them with property tests on
  arbitrary input (no panic, offsets on character boundaries); a `cargo fuzz`
  target for them and for emission belongs to the 0.9.0 fuzzing item below,
  which already lists it. Not moved, recorded so it is not forgotten.

## v0.5.0 - Implementation
- [ ] Semantic-token legend generation; embedded-language injection; scope naming conventions.
- [ ] Iterative `Clone`/`Debug`/`PartialEq` for `Rule` and `Grammar` (family policy F11): the derived impls recurse, so a programmatically built tree far deeper than `MAX_RULE_DEPTH` can overflow on clone/debug/eq. Untrusted grammars cannot reach this (validation caps depth); moved here from 0.2 by the coordinator's review.

## v0.9.0 - Hardening
- [ ] Fuzzing emission for injection; validation against real editor loaders (dev-only).

## v1.0.0 - Stable
- [ ] Frozen after the LexerSketch VS Code extension ships grammars from it (D18).
