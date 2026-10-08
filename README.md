<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <b>textmate-lang</b>
    <br>
    <sub><sup>TEXTMATE GRAMMAR GENERATION</sup></sub>
</h1>

<div align="center">
    <a href="https://crates.io/crates/textmate-lang"><img alt="Crates.io" src="https://img.shields.io/crates/v/textmate-lang"></a>
    <a href="https://crates.io/crates/textmate-lang"><img alt="Downloads" src="https://img.shields.io/crates/d/textmate-lang?color=%230099ff"></a>
    <a href="https://docs.rs/textmate-lang"><img alt="docs.rs" src="https://img.shields.io/docsrs/textmate-lang"></a>
    <a href="https://github.com/jamesgober/textmate-lang/actions"><img alt="CI" src="https://github.com/jamesgober/textmate-lang/actions/workflows/ci.yml/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.85%2B-blue"></a>
</div>

<br>

<div align="left">
    <p>
        <strong>textmate-lang</strong> builds TextMate grammars in Rust and emits them as <code>tmLanguage.json</code>, the highlighting format that VS Code, Sublime Text, TextMate, Shiki, and many other editors and highlighters load without tree-sitter. Describe a language's tokens with matches, begin/end regions, captures, and includes; call <code>to_json</code>; get a file an editor can load.
    </p>
    <p>
        A grammar file cannot check itself: a typo in a regular expression, a capture of a group that does not exist, an include of an entry that is not there, or two pattern lists that include each other all load silently and then highlight wrongly, or hang the tokenizer. Emitting here validates the whole grammar first and refuses each of these with an error that points into the output. Regular expressions are held to a documented subset of Oniguruma syntax that the engines loading TextMate grammars read the same way. The JSON writer is the crate's own and escapes every string it writes, so no name, pattern, or scope can change the structure of the output. It is the sibling of <code>treesitter-lang</code> in the <code>-lang</code> family.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.85+</strong> (Rust 2024 edition). <code>no_std</code>-compatible (needs only <code>alloc</code>), <code>#![forbid(unsafe_code)]</code>, no dependencies.
    </p>
    <blockquote>
        <strong>Status: pre-1.0 (0.2.0, the foundation).</strong> The grammar model, validation, and JSON emission are complete; semantic-token legends and embedded-language helpers come in 0.5. The API may change before <code>1.0.0</code>. See <a href="./dev/ROADMAP.md"><code>ROADMAP</code></a> and <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a>.
    </blockquote>
</div>

<hr>
<br>

## The model

- A **[`Grammar`](./docs/API.md#grammar)** is a scope name (`source.demo`), top-level patterns, a repository of named rules, and injections. [`to_json`](./docs/API.md#grammarto_json) validates it and returns the JSON.
- A **[`Rule`](./docs/API.md#rule)** is one entry of a pattern list: a **[`Match`](./docs/API.md#match)**, a **[`BeginEnd`](./docs/API.md#beginend)** or **[`BeginWhile`](./docs/API.md#beginwhile)** region, an include of a repository entry, `$self`, `$base`, or another grammar, or a list of rules. A **[`Capture`](./docs/API.md#capture)** says what to do with a capture group.
- An **[`Error`](./docs/API.md#error)** says what is wrong and where, as a JSON Pointer into the output (`/repository/strings/end`); a regular-expression error also gives the byte offset in the pattern.
- **[`validate_regex`](./docs/API.md#validate_regex)** and **[`escape_regex`](./docs/API.md#escape_regex)** check one pattern, and turn literal text (keywords, operators) into one.

<br>

What it guarantees, and how each guarantee is checked:

| Guarantee | How it is held |
|---|---|
| No input can change the structure of the output. | Property tests build random grammars whose names, file types, and pattern text are arbitrary strings (quotes, backslashes, control characters, U+2028/U+2029, any Unicode), emit them pretty and compact, read them back with a strict RFC 8259 parser written in the tests, and compare with the expected value built directly. |
| Output is deterministic. | Repository entries and injections are sorted by name and captures by group; property tests add them in random orders and get identical bytes. |
| Every regular expression is in the documented Oniguruma subset. | Random syntax trees of the subset print to patterns the checker accepts with the right group count; removing any closing parenthesis is refused; arbitrary input never panics and errors point at character boundaries. |
| `escape_regex` output matches its input literally, in every context. | Property-tested: any text, and any concatenation of escaped texts, is accepted in `match` and `end` patterns and in extended mode. |
| Include cycles that would hang a tokenizer are refused; recursion through regions is not. | Property tests compare the cycle check with a reachability computation on random include graphs, and check that each reported cycle is real. |
| Scope names follow TextMate conventions, and substitutions name real groups. | Property tests compare the checker with a direct reading of the rules. |
| No grammar can overflow the stack. | Validation, emission, and dropping are iterative; 500,000-deep rules are refused without recursion, and 100,000-entry grammars validate and emit. |

<hr>
<br>

## Installation

```toml
[dependencies]
textmate-lang = "0.2"
```

Without the standard library:

```toml
[dependencies]
textmate-lang = { version = "0.2", default-features = false }
```

<hr>
<br>

## Quick start

```rust
use textmate_lang::{BeginEnd, Grammar, Match, Rule};

let json = Grammar::new("source.demo")
    .name("Demo")
    .file_type("demo")
    .pattern(Rule::include("comments"))
    .pattern(Rule::include("strings"))
    .pattern(Match::new(r"\b(if|else|while|return)\b").scope("keyword.control.demo"))
    .pattern(
        Match::new(r"\b(fn)\s+([a-z_]\w*)")
            .capture(1, "storage.type.function.demo")
            .capture(2, "entity.name.function.demo"),
    )
    .repository("comments", Match::new("#.*$").scope("comment.line.number-sign.demo"))
    .repository(
        "strings",
        BeginEnd::new("\"", "\"")
            .scope("string.quoted.double.demo")
            .pattern(Match::new(r"\\.").scope("constant.character.escape.demo")),
    )
    .to_json()?;

assert!(json.contains("\"scopeName\": \"source.demo\""));
// std::fs::write("syntaxes/demo.tmLanguage.json", json)?;
# Ok::<(), textmate_lang::Error>(())
```

### Mistakes caught before an editor sees them

```rust
use textmate_lang::{BeginEnd, Error, Grammar, Match, RegexError, RegexErrorKind, Rule};

// A pattern outside the subset: `x{2}?` is lazy in some engines and
// optional in Oniguruma.
let err = Grammar::new("source.demo").pattern(Match::new("x{2}?")).validate().unwrap_err();
assert_eq!(
    err,
    Error::InvalidRegex {
        path: "/patterns/0/match".into(),
        error: RegexError { offset: 1, kind: RegexErrorKind::AmbiguousQuantifier },
    },
);

// `end` back-references name `begin` groups, and there is only one.
let err = Grammar::new("source.demo").pattern(BeginEnd::new("(['\"])", r"\2")).validate().unwrap_err();
assert!(matches!(err, Error::InvalidRegex { ref path, .. } if path == "/patterns/0/end"));

// Two pattern lists that include each other expand forever.
let err = Grammar::new("source.demo")
    .repository("a", Rule::patterns([Rule::include("b")]))
    .repository("b", Rule::patterns([Rule::include("a")]))
    .validate()
    .unwrap_err();
assert_eq!(err.to_string(), "includes form a cycle that consumes no text: #a -> #b -> #a");
```

### Generating from data

Keyword and operator lists usually come from somewhere else; `escape_regex` makes literal text safe to match:

```rust
use textmate_lang::{Grammar, Match, escape_regex};

let keywords = ["let", "fn", "match"];
let operators = ["=>", "->", "::", "+="].map(escape_regex);
let json = Grammar::new("source.demo")
    .pattern(Match::new(format!(r"\b(?:{})\b", keywords.join("|"))).scope("keyword.demo"))
    .pattern(Match::new(operators.join("|")).scope("keyword.operator.demo"))
    .to_json_compact()?;
assert!(json.contains(r#""match":"\\=\\>|\\-\\>|\\:\\:|\\+\\=""#));
# Ok::<(), textmate_lang::Error>(())
```

<hr>
<br>

## Examples

Runnable programs in [`examples/`](./examples):

| Example | What it shows |
|---|---|
| [`calc`](./examples/calc.rs) | The Tier-1 path: a small language's grammar, built and emitted. `cargo run --example calc > calc.tmLanguage.json` |
| [`errors`](./examples/errors.rs) | What validation reports for common mistakes. `cargo run --example errors` |

<hr>
<br>

## Performance

Validation is one pass over the grammar plus one sort of the repository; includes resolve by binary search over a contiguous name index, and the include graph is searched once, iteratively. The regex checker is a single left-to-right pass with an explicit group stack, reused across a grammar's patterns. Emission writes straight into one `String` from a work stack, copying unescaped runs of each string in bulk.

Measured with the benchmarks in [`benches/`](./benches), Windows x86_64, Rust stable, release profile. Throughput is relative to the size of the pretty JSON (for the regex rows, of the pattern or text):

| Benchmark | What it measures | Time | Throughput |
|---|---|---:|---:|
| `large_100k/validate` | Validate 100,000 repository entries (matches with captures, regions with nested patterns), 100,000 includes; 45 MB of JSON. | ~136 ms | ~332 MiB/s |
| `large_100k/to_json_pretty` | Validate and emit the same grammar, pretty. | ~255 ms | ~177 MiB/s |
| `large_100k/to_json_compact` | Validate and emit it, compact. | ~249 ms | ~182 MiB/s |
| `typical_300/to_json_pretty` | Validate and emit a 300-entry grammar, the size of a hand-written language grammar. | ~0.38 ms | ~356 MiB/s |
| `regex/validate_keywords` | Check one alternation of 100,000 keywords (1.3 MiB). | ~3.9 ms | ~344 MiB/s |
| `regex/validate_mixed` | Check 400 KiB of classes, groups, look-around, and quantifiers. | ~2.0 ms | ~203 MiB/s |
| `regex/escape` | Escape 1.1 MiB of text. | ~2.3 ms | ~484 MiB/s |

A grammar the size real languages have validates and emits in well under a millisecond. On the 100,000-entry grammar, include resolution dominates validation: each include is a binary search over 100,000 names that no longer fit in cache. A hash index would be faster there, but its worst case on hostile names is quadratic; the binary search's is not.

```bash
cargo bench --bench bench
```

Criterion writes per-benchmark reports to `target/criterion/`. Numbers vary by CPU and between runs on this machine by about ±10%; use the trend, not a single absolute.

<hr>
<br>

## Design notes

- **A subset, on purpose.** Oniguruma accepts many spellings that other engines refuse or read differently: `x{3}?` is an optional `x{3}`, not a lazy one; plain groups stop being numbered when named groups appear unless the loader asks otherwise (VS Code does, others do not). Refusing these keeps a grammar's meaning the same in every editor. The subset is listed in [`docs/API.md`](./docs/API.md#regex-subset).
- **VS Code's quirks are checked, not assumed away.** VS Code substitutes `begin` captures into `end` and `while` patterns by searching the text for a backslash and digits, so `\\1` there is misread; it is refused. A top-level `|` in a selector silently drops the rest; it is refused. `+` in a selector's scopes is dropped by its tokenizer; it is refused.
- **Which cycles are errors.** Recursion through a region (parentheses within parentheses) consumes text at every level and is how nested constructs are written. A cycle of pattern lists that include each other consumes nothing: the tokenizer expands the lists while building its scanner and never stops. Only the second is refused.
- **Escaping is not conditional on validation.** Every string goes through the JSON escaper whatever the validator decided, so a gap in validation could produce a grammar an editor rejects, never JSON with a different structure.
- **No recursion where input reaches.** Validation and emission run from explicit work stacks; dropping a rule moves nested rules onto a stack; nesting is capped at levels editors handle.

<hr>
<br>

## Testing

The suite runs on Windows, Linux, and macOS through the CI matrix, on stable and the 1.85 MSRV:

```bash
cargo test                       # unit + integration + property + doctests
cargo test --no-default-features # no_std + alloc
cargo clippy --all-targets --all-features -- -D warnings
cargo bench --bench bench
```

The property tests in [`tests/proptests.rs`](./tests/proptests.rs) hold the emitter to a strict RFC 8259 reader and a direct construction of the expected JSON, the regex checker to random syntax trees of the subset, the cycle check to a reachability computation, and the scope checker to a direct reading of the naming rules. `PROPTEST_CASES=20000` runs a longer soak. Every `rust` example in this README and in [`docs/API.md`](./docs/API.md) is compiled and run as a doctest.

What the tests cannot show: that every editor's loader accepts every grammar emitted. The subset is chosen from Oniguruma's documented syntax and VS Code's loader source; running emitted grammars through `vscode-textmate` itself is planned for 0.9 ([`ROADMAP`](./dev/ROADMAP.md)).

<hr>
<br>

## Cross-platform support

- Linux (x86_64, aarch64)
- macOS (x86_64, Apple Silicon)
- Windows (x86_64)

The crate uses no operating-system facilities and no platform-specific code; a grammar emits the same bytes on every platform.

<hr>
<br>

## Contributing

See [`REPS.md`](./REPS.md) and [`dev/DIRECTIVES.md`](./dev/DIRECTIVES.md) for the engineering standards every change is held to, and [`dev/ROADMAP.md`](./dev/ROADMAP.md) for what comes next. Before a PR: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` must be clean.

<br>

<div id="license">
    <h2>License</h2>
    <p>Licensed under either of</p>
    <ul>
        <li><b>Apache License, Version 2.0</b> &mdash; <a href="./LICENSE-APACHE">LICENSE-APACHE</a></li>
        <li><b>MIT License</b> &mdash; <a href="./LICENSE-MIT">LICENSE-MIT</a></li>
    </ul>
    <p>at your option.</p>
</div>

<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>James Gober.</strong></sup>
</div>
