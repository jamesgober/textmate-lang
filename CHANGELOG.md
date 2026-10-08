<h1 align="center">
    <img width="90px" height="auto" src="https://raw.githubusercontent.com/jamesgober/jamesgober/main/media/icons/hexagon-3.svg" alt="Triple Hexagon">
    <br><b>CHANGELOG</b>
</h1>
<p>
  All notable changes to <code>textmate-lang</code> will be documented in this file. The format is based on <a href="https://keepachangelog.com/en/1.1.0/">Keep a Changelog</a>,
  and this project adheres to <a href="https://semver.org/spec/v2.0.0.html/">Semantic Versioning</a>.
</p>

---

## [Unreleased]

---

## [0.2.0] - 2026-10-08

The foundation: a TextMate grammar model, validation of everything a grammar
file cannot check for itself, and `tmLanguage.json` emission through the
crate's own JSON writer.

### Added

- `Grammar`: a by-value builder (`new`, `name`, `file_type`,
  `first_line_match`, `injection_selector`, `pattern`, `repository`,
  `injection`), `scope_name`, `validate`, and the emitters `to_json`
  (pretty, the Tier-1 call), `to_json_compact`, and `write_json` with
  `Style::{Pretty, Compact}`.
- `Rule`: includes of repository entries (`include`), `$self`
  (`include_self`), `$base` (`include_base`), other grammars
  (`include_scope`, `include_scope_entry`), and pattern lists (`patterns`);
  `From` conversions from `Match`, `BeginEnd`, and `BeginWhile`. Dropping a
  rule is iterative.
- `Match`, `BeginEnd` (with `contentName`, begin/end/shared captures,
  nested patterns, `applyEndPatternLast`), `BeginWhile` (with
  `whileCaptures`), and `Capture` (a scope, nested patterns, or both).
- Validation of a documented Oniguruma subset for every regular expression:
  balanced groups and classes, known escapes, hex and Unicode escapes,
  Unicode properties from a fixed list, POSIX brackets, nested classes and
  intersections, quantifiers with one meaning in every engine, look-ahead,
  fixed-length look-behind, atomic groups, inline options with extended-mode
  comments, back-references to existing groups (in `end` and `while`, to
  `begin` groups), and no mixing of named and numbered groups. Errors carry
  the byte offset and a `RegexErrorKind`.
- `validate_regex` (the check on its own, returning the group count) and
  `escape_regex` (literal text as a pattern, safe in every context and
  under concatenation).
- Validation of scope names (with `$n`, `${n:/downcase}`, `${n:/upcase}`
  substitutions naming existing groups), scope selectors in
  `vscode-textmate`'s syntax, repository names, file types, captures
  (existing, unique, applicable), includes (no dangling repository
  references), include cycles of pattern lists (refused) versus recursion
  through regions and captures (accepted), and rule nesting
  (`MAX_RULE_DEPTH` = 128; `MAX_REGEX_DEPTH` = 256 for patterns).
- `Error` (`#[non_exhaustive]`, thirteen variants), each locating the
  problem with a JSON Pointer into the output.
- The JSON writer: deterministic key order, repository entries and
  injections sorted by name, captures by group; every string escaped
  (quotes, backslashes, all control characters, U+2028, U+2029).
- Unit, integration, and property tests (a strict RFC 8259 reader and a
  direct construction of the expected JSON; random regex syntax trees;
  a reference cycle check; a reference scope checker), criterion benchmarks
  at 100,000 entries, and the `calc` and `errors` examples.

### Changed

- Version 0.2.0. `README.md` and `docs/API.md` document the surface.

---

## [0.1.0] - 2026-10-08

Initial scaffold and repository bootstrap. No domain logic yet &mdash; this release establishes the structure, tooling, and quality gates the implementation will be built on.

### Added

- `Cargo.toml` with crate metadata, Rust 2024 edition, MSRV 1.85.
- Dual `Apache-2.0 OR MIT` license files.
- `README.md`, `CHANGELOG.md`, and a documentation skeleton.
- `REPS.md` compliance baseline.
- `.github/workflows/ci.yml` CI matrix; `deny.toml`, `clippy.toml`, `rustfmt.toml`.
- `dev/DIRECTIVES.md` and `dev/ROADMAP.md` (committed engineering standards + plan).

[Unreleased]: https://github.com/jamesgober/textmate-lang/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/jamesgober/textmate-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/textmate-lang/releases/tag/v0.1.0
