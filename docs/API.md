# textmate-lang &mdash; API Reference

> Complete reference for every public item in `textmate-lang`, with examples.
> **Status: pre-1.0 (0.2.0).** The surface below is designed across the 0.x
> series and frozen at `1.0.0`, after a real consumer has used it end to end.
> See [`../dev/ROADMAP.md`](../dev/ROADMAP.md).

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>

## Table of contents

- [Overview](#overview)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Concepts](#concepts)
  - [What a grammar is](#what-a-grammar-is)
  - [The output](#the-output)
  - [What validation checks](#what-validation-checks)
  - [Captures, substitutions, and back-references](#captures-substitutions-and-back-references)
  - [Includes and cycles](#includes-and-cycles)
- [Regex subset](#regex-subset)
- [Scope names and selectors](#scope-names-and-selectors)
- [`Grammar`](#grammar)
  - [`Grammar::new`](#grammarnew)
  - [`Grammar::scope_name`](#grammarscope_name)
  - [`Grammar::name`](#grammarname)
  - [`Grammar::file_type`](#grammarfile_type)
  - [`Grammar::first_line_match`](#grammarfirst_line_match)
  - [`Grammar::injection_selector`](#grammarinjection_selector)
  - [`Grammar::pattern`](#grammarpattern)
  - [`Grammar::repository`](#grammarrepository)
  - [`Grammar::injection`](#grammarinjection)
  - [`Grammar::validate`](#grammarvalidate)
  - [`Grammar::to_json`](#grammarto_json)
  - [`Grammar::to_json_compact`](#grammarto_json_compact)
  - [`Grammar::write_json`](#grammarwrite_json)
- [`Style`](#style)
- [`Rule`](#rule)
  - [`Rule::include`](#ruleinclude)
  - [`Rule::include_self`](#ruleinclude_self)
  - [`Rule::include_base`](#ruleinclude_base)
  - [`Rule::include_scope`](#ruleinclude_scope)
  - [`Rule::include_scope_entry`](#ruleinclude_scope_entry)
  - [`Rule::patterns`](#rulepatterns)
- [`Match`](#match)
- [`BeginEnd`](#beginend)
- [`BeginWhile`](#beginwhile)
- [`Capture`](#capture)
- [`validate_regex`](#validate_regex)
- [`escape_regex`](#escape_regex)
- [`Error`](#error)
- [`RegexError` and `RegexErrorKind`](#regexerror-and-regexerrorkind)
- [Constants](#constants)
- [Feature flags](#feature-flags)
- [Limits](#limits)

## Overview

`textmate-lang` builds TextMate grammars in Rust and emits them as
`tmLanguage.json`, the highlighting format VS Code, Sublime Text, TextMate,
Shiki, and many other editors and highlighters load. Emitting validates the
whole grammar first: regular expressions against a documented
[Oniguruma subset](#regex-subset), scope names, captures, includes, and
selectors. The JSON writer is the crate's own and escapes every string it
writes.

| Item | Kind | Purpose |
|---|---|---|
| [`Grammar`](#grammar) | struct | The grammar: scope name, patterns, repository, injections. Validates and emits. |
| [`Style`](#style) | enum | Pretty or compact output. |
| [`Rule`](#rule) | struct | One entry of a pattern list: a match, a region, an include, or a list. |
| [`Match`](#match) | struct | A rule matching one regular expression. |
| [`BeginEnd`](#beginend) | struct | A region from a `begin` match to an `end` match. |
| [`BeginWhile`](#beginwhile) | struct | A region that continues while each line starts with a `while` match. |
| [`Capture`](#capture) | struct | What to do with a capture group's text. |
| [`validate_regex`](#validate_regex) | fn | Checks one pattern against the subset; returns its group count. |
| [`escape_regex`](#escape_regex) | fn | Escapes literal text into a pattern. |
| [`Error`](#error) | enum | Why a grammar was refused, and where. |
| [`RegexError`, `RegexErrorKind`](#regexerror-and-regexerrorkind) | struct, enum | Why a pattern was refused, and at which byte. |
| [`MAX_RULE_DEPTH`, `MAX_REGEX_DEPTH`](#constants) | consts | Nesting limits. |

## Installation

```toml
[dependencies]
textmate-lang = "0.2"
```

For `no_std` targets (the crate needs only `alloc`):

```toml
[dependencies]
textmate-lang = { version = "0.2", default-features = false }
```

## Quick start

```rust
use textmate_lang::{BeginEnd, Grammar, Match, Rule};

let json = Grammar::new("source.demo")
    .name("Demo")
    .file_type("demo")
    .pattern(Rule::include("strings"))
    .pattern(Match::new(r"\b(if|else|while)\b").scope("keyword.control.demo"))
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

## Concepts

### What a grammar is

A TextMate tokenizer reads a file line by line. At each position it tries the
current list of patterns and takes the earliest match (the first pattern in
the list wins a tie). A [`Match`](#match) rule scopes what it matched and
continues. A [`BeginEnd`](#beginend) rule opens a region: from then on its own
patterns apply, plus its `end` pattern, until `end` matches. A
[`BeginWhile`](#beginwhile) region is the line-oriented version: it lasts while
each following line starts with a `while` match. An include splices in
another pattern list: a repository entry, the grammar's own top-level
patterns (`$self`), the base grammar's (`$base`), or another grammar's.

Scopes (`keyword.control.demo`) are what themes color. Every match and region
can carry one, and capture groups can carry their own.

### The output

`to_json` writes `tmLanguage.json` with a fixed key order:

- grammar: `name`, `scopeName`, `fileTypes`, `firstLineMatch`,
  `injectionSelector`, `patterns`, `repository`, `injections`;
- rule: `include`, `name`, `contentName`, `match`, `begin`, `beginCaptures`,
  `end`, `endCaptures`, `while`, `whileCaptures`, `captures`,
  `applyEndPatternLast`, `patterns`;
- capture: `name`, `patterns`.

Absent and empty fields are left out, except the grammar's `patterns`, which
is always present. Repository entries and injections are sorted by name, and
captures by group number, so the output depends only on the grammar, never on
the order things were added. `applyEndPatternLast` is written as `1`, the
TextMate convention.

Every string is escaped: `"` and `\`, all control characters (as `\n`, `\r`,
`\t`, `\b`, `\f`, or `\u00XX`), and U+2028 and U+2029, which JSON allows raw
but JavaScript does not. All other characters are written as UTF-8. No string
can change the structure of the output; the property tests read the output
back with a strict RFC 8259 parser and compare it with a direct construction
of the expected value.

### What validation checks

`to_json`, `to_json_compact`, `write_json`, and `validate` check the whole
grammar and report the first problem, in this order: the grammar's
`scopeName`, `fileTypes`, `firstLineMatch`, and `injectionSelector`; the
repository names and injection selectors and their uniqueness; every rule
(the top-level patterns, then the repository in name order, then the
injections in selector order), each before the rules nested in it; and
finally include cycles. Within a rule, its patterns are checked first, then
its scope names, then its captures.

Every error that concerns a place in the grammar names it with a
[JSON Pointer](https://www.rfc-editor.org/rfc/rfc6901) into the output:
`/patterns/0/match`, `/repository/strings/endCaptures/1/name`. Repository and
injection entries are addressed by name.

```rust
use textmate_lang::{Error, Grammar, Match, RegexError, RegexErrorKind};

let err = Grammar::new("source.demo")
    .repository("numbers", Match::new(r"\d+(\.\d+"))
    .validate()
    .unwrap_err();
assert_eq!(
    err,
    Error::InvalidRegex {
        path: "/repository/numbers/match".into(),
        error: RegexError { offset: 3, kind: RegexErrorKind::UnclosedGroup },
    },
);
```

### Captures, substitutions, and back-references

Capture group 0 is the whole match; groups are numbered by their `(` from the
left. Every capture must name a group the pattern has. Rule and capture
scopes may substitute captured text: `$1`, `${1:/downcase}`,
`${1:/upcase}` (VS Code's forms), and the group must exist.

Which pattern's groups count:

| Where | Groups of |
|---|---|
| `Match`: `name`, `captures` | `match` |
| region: `name`, `contentName`, `beginCaptures` | `begin` |
| region: `endCaptures` / `whileCaptures` | `end` / `while` |
| region: `captures` | each of `begin` and `end`/`while` it applies to (a side with its own captures ignores them) |
| `end`/`while`: numbered back-references `\1` | `begin` (VS Code substitutes the text `begin` captured) |
| a capture's `name` | the pattern the capture belongs to |

```rust
use textmate_lang::{BeginEnd, Error, Grammar};

// A heredoc: `end` refers back to the delimiter `begin` captured.
let ok = Grammar::new("source.demo")
    .pattern(BeginEnd::new(r"<<(\w+)", r"^\1$").content_scope("string.unquoted.heredoc.demo"));
assert_eq!(ok.validate(), Ok(()));

// `$2` in the region's name refers to `begin`, which has one group.
let err = Grammar::new("source.demo")
    .pattern(BeginEnd::new(r"<<(\w+)", r"^\1$").scope("string.heredoc.$2"))
    .validate()
    .unwrap_err();
assert_eq!(err, Error::CaptureOutOfRange { path: "/patterns/0/name".into(), group: 2, groups: 1 });
```

### Includes and cycles

An include of a repository entry must name an entry that exists. Includes of
other grammars (`source.js`, `source.js#expression`) cannot be checked here,
except that naming this grammar's own scope is resolved like a local include.

Recursion is how nested constructs are written, and is accepted wherever text
is consumed in between: a region whose patterns include the list that
contains it (parentheses inside parentheses), or a capture whose patterns
include its own rule. What is refused is a cycle of **pattern lists** that
include each other directly: the top-level patterns, repository entries that
are a [`Rule::patterns`](#rulepatterns) list, and repository entries that are
a single include. A tokenizer expands such lists when it builds its scanner,
before matching anything, and recurses forever: `vscode-textmate` collects
them with a recursion that keeps no record of lists already visited (read
from its source, not observed in an editor).
`$base` is treated as `$self`, which is what it means when the grammar is not
embedded in another.

```rust
use textmate_lang::{BeginEnd, Error, Grammar, Match, Rule};

// Fine: the region consumes `(` before recursing.
let ok = Grammar::new("source.demo")
    .pattern(Rule::include("expression"))
    .repository("expression", Rule::patterns([
        Match::new(r"\d+").scope("constant.numeric.demo").into(),
        BeginEnd::new(r"\(", r"\)").pattern(Rule::include("expression")).into(),
    ]));
assert_eq!(ok.validate(), Ok(()));

// Refused: two lists that include each other.
let err = Grammar::new("source.demo")
    .repository("a", Rule::patterns([Rule::include("b")]))
    .repository("b", Rule::patterns([Rule::include("a")]))
    .validate()
    .unwrap_err();
assert_eq!(err, Error::IncludeCycle { cycle: vec!["#a".into(), "#b".into(), "#a".into()] });
```

## Regex subset

TextMate grammars are matched by [Oniguruma](https://github.com/kkos/oniguruma):
VS Code runs it as `vscode-oniguruma` with its default syntax and the
`CAPTURE_GROUP` option. Oniguruma accepts far more than other engines that
load TextMate grammars, and reads some spellings differently from them, so
every pattern is held to this subset. Anything else is refused with the byte
offset of the first construct outside it.

| Syntax | Meaning |
|---|---|
| any character except `\ ^ $ . \| ? * + ( ) [ ] {` | itself; `}` alone is literal too |
| `.` | any character except newline |
| `\t \n \r \f \v \a \e` | tab, newline, carriage return, form feed, vertical tab, bell, escape |
| `\xHH` | the character `HH`, 00 to 7F (above 7F Oniguruma reads a raw byte) |
| `\x{H...}` | the code point with 1 to 8 hex digits, not a surrogate, at most 10FFFF |
| `\uHHHH` | the code point with exactly four hex digits, not a surrogate |
| `\` + ASCII punctuation or space | that character |
| `\w \W \d \D \s \S \h \H` | word, digit, space, and hex-digit characters, and their complements |
| `\p{Name} \p{^Name} \P{Name}` | Unicode property; see the list below |
| `[...] [^...]` | class: characters, escapes, ranges `a-z`, shorthands, properties, POSIX brackets `[:alpha:]` / `[:^alpha:]`, nested classes, intersections `&&` |
| `^ $ \A \z \Z \G \b \B` | anchors |
| `x\|y`, `xy` | alternation, sequence |
| `(...)` `(?:...)` `(?<name>...)` `(?>...)` | capturing, non-capturing, named, atomic groups |
| `(?=...)` `(?!...)` `(?<=...)` `(?<!...)` | look-ahead and look-behind |
| `(?imx-imx)` `(?imx-imx:...)` | options: `i` ignore case, `m` dot matches newline, `x` extended |
| `(?#...)` | comment |
| `* + ?` `{n}` `{n,}` `{n,m}` | quantifiers, counts up to 100000; `*? +? ?? {n,}? {n,m}?` lazy; `*+ ++ ?+` possessive |
| `\1` ... `\k<name>` | back-references |

Extended mode (`x`) is honored: outside classes, whitespace is skipped and
`#` starts a comment that runs to the end of the line. An isolated `(?imx)`
applies to the rest of its group, which Oniguruma parses as a group of its
own (so `a(?i)b|c` is `a(?i:b|c)`).

Refused, with the reason in [`RegexErrorKind`](#regexerror-and-regexerrorkind):

- unbalanced `(`, `)`, `[`, `]`; empty classes (`[]`, `[^]`; `]` is never
  literal at the start of a class); a `{` that does not start a valid
  quantifier (write `\{`); `{,n}` (write `{0,n}`);
- unknown escapes, octal escapes (`\0`, `\012`), `\b` inside a class,
  `\cX`, `\K`, `\R`, `\X`, `\Q...\E`, `\g<...>`, one-letter `\pL`;
- `{n}?`, `{n}+`, `{n,m}+` (Oniguruma: optional or repeated repetition;
  other engines: lazy or possessive) and a quantifier after a quantifier
  (`a**`);
- quantified anchors and look-arounds; a quantifier with nothing to repeat;
- look-behind alternatives of variable length (each top-level alternative
  must have a fixed length, and alternatives of groups inside must all have
  the same length), and capture groups or back-references inside a
  look-behind;
- named and unnamed capturing groups in one pattern, and numbered
  back-references in a pattern with named groups (without `CAPTURE_GROUP`,
  Oniguruma does not number plain groups, so captures would differ between
  editors); duplicate group names; `\k<name>` naming no group; numbered
  back-references beyond the group count;
- conditionals, absent operators, callouts, `(?P<...>)`, `(?'...')`, other
  options;
- a raw NUL character (write `\x00`);
- in `end` and `while` patterns, an escaped backslash before a digit
  (`\\1`), which VS Code's back-reference substitution misreads;
- groups and classes nested more than [`MAX_REGEX_DEPTH`](#constants) deep.

Supported property names (case, spaces, `_`, and `-` are ignored, as
Oniguruma ignores them): the general categories `L Lu Ll Lt Lm Lo M Mn Mc Me
N Nd Nl No P Pc Pd Ps Pe Pi Pf Po S Sm Sc Sk So Z Zs Zl Zp C Cc Cf Cs Co Cn`;
`Alnum Alpha ASCII Blank Cntrl Digit Graph Lower Print Punct Space Upper
XDigit Word Any Assigned`; `Alphabetic Lowercase Uppercase White_Space
ID_Start ID_Continue XID_Start XID_Continue`; and the scripts `Arabic
Armenian Bengali Bopomofo Cherokee Common Cyrillic Devanagari Ethiopic
Georgian Greek Gujarati Gurmukhi Han Hangul Hebrew Hiragana Inherited Kannada
Katakana Khmer Lao Latin Malayalam Mongolian Myanmar Oriya Sinhala Syriac
Tamil Telugu Thaana Thai Tibetan`.

```rust
use textmate_lang::{RegexErrorKind, validate_regex};

assert_eq!(validate_regex(r"(?<=\.)[\p{L}_][\p{L}\p{N}_]*"), Ok(0));
assert_eq!(validate_regex("(?x) (\\d+) # digits\n  (?: , (\\d+) )*"), Ok(2));
assert_eq!(validate_regex("x{3}?").unwrap_err().kind, RegexErrorKind::AmbiguousQuantifier);
assert_eq!(validate_regex("(?<=a+)b").unwrap_err().kind, RegexErrorKind::LookbehindNotFixed);
assert_eq!(validate_regex("(?<n>a)(b)").unwrap_err().kind, RegexErrorKind::MixedGroups);
```

## Scope names and selectors

A **scope** is dot-separated segments, each made of ASCII letters, digits,
`_`, `+`, and `-`, starting with a letter, digit, or `_`: `source.c++`,
`entity.name.function.objective-c`. A rule's or capture's **name** may list
several scopes separated by single spaces and may substitute captures
(`$1`, `${1:/downcase}`, `${1:/upcase}`). The grammar's `scopeName` and the
scope of an external include are a single scope without substitutions.

A **scope selector** (`injectionSelector`, the keys of `injections`) is
checked in the syntax `vscode-textmate` reads:

```text
selector    = entry ("," entry)*
entry       = ("L:" | "R:")? conjunction
conjunction = operand+
operand     = "-" operand | "(" inner ")" | scope+
inner       = conjunction (("," | "|") conjunction)*
```

Scopes in selectors may not contain `+` (`vscode-textmate` drops it). `|` is
accepted only inside parentheses, because at the top level `vscode-textmate`
ignores everything after it. Parentheses nest at most 32 deep.

```rust
use textmate_lang::{Error, Grammar};

assert_eq!(Grammar::new("todo.injection").injection_selector("L:source.js -comment -string").validate(), Ok(()));
let err = Grammar::new("todo.injection").injection_selector("B:source.js").validate().unwrap_err();
assert!(matches!(err, Error::InvalidSelector { offset: 1, .. }));
```

## `Grammar`

```rust,ignore
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grammar { /* private */ }
```

A TextMate grammar. Every method but `scope_name`, `validate`, and the
emitters takes and returns the grammar by value, so a grammar reads as one
chain. Nothing is checked until it is validated or emitted. Strings are taken
as `impl Into<Cow<'static, str>>`: a `&'static str` is not copied, a `String`
is moved.

### `Grammar::new`

```rust,ignore
pub fn new(scope_name: impl Into<Cow<'static, str>>) -> Grammar
```

An empty grammar whose root scope is `scope_name` (`"scopeName"`). By
convention `source.<language>` or `text.<format>`.

| Parameter | Meaning |
|---|---|
| `scope_name` | A single scope; see [Scope names](#scope-names-and-selectors). |

```rust
use textmate_lang::Grammar;

assert_eq!(Grammar::new("source.toml").to_json_compact()?, r#"{"scopeName":"source.toml","patterns":[]}"#);
# Ok::<(), textmate_lang::Error>(())
```

### `Grammar::scope_name`

```rust,ignore
pub fn scope_name(&self) -> &str
```

The root scope given to `new`.

```rust
use textmate_lang::Grammar;

assert_eq!(Grammar::new("source.demo").scope_name(), "source.demo");
```

### `Grammar::name`

```rust,ignore
pub fn name(self, name: impl Into<Cow<'static, str>>) -> Grammar
```

The display name (`"name"`). Any text.

```rust
use textmate_lang::Grammar;

let json = Grammar::new("source.demo").name("Demo").to_json_compact()?;
assert!(json.starts_with(r#"{"name":"Demo","#));
# Ok::<(), textmate_lang::Error>(())
```

### `Grammar::file_type`

```rust,ignore
pub fn file_type(self, extension: impl Into<Cow<'static, str>>) -> Grammar
```

Adds a file extension (`"fileTypes"`). Must not be empty
([`InvalidName`](#error)).

```rust
use textmate_lang::Grammar;

let json = Grammar::new("source.demo").file_type("dm").to_json_compact()?;
assert!(json.contains(r#""fileTypes":["dm"]"#));
# Ok::<(), textmate_lang::Error>(())
```

### `Grammar::first_line_match`

```rust,ignore
pub fn first_line_match(self, regex: impl Into<Cow<'static, str>>) -> Grammar
```

A pattern matched against a file's first line (`"firstLineMatch"`), such as a
shebang. Checked like every pattern.

```rust
use textmate_lang::Grammar;

let json = Grammar::new("source.demo").first_line_match(r"^#!.*\bdemo\b").to_json_compact()?;
assert!(json.contains(r#""firstLineMatch":"^#!.*\\bdemo\\b""#));
# Ok::<(), textmate_lang::Error>(())
```

### `Grammar::injection_selector`

```rust,ignore
pub fn injection_selector(self, selector: impl Into<Cow<'static, str>>) -> Grammar
```

Makes this an injection grammar (`"injectionSelector"`): its top-level
patterns are injected where the selector matches in other grammars.

```rust
use textmate_lang::{Grammar, Match};

let json = Grammar::new("todo.injection")
    .injection_selector("L:comment")
    .pattern(Match::new(r"\bTODO\b").scope("keyword.todo"))
    .to_json_compact()?;
assert!(json.contains(r#""injectionSelector":"L:comment""#));
# Ok::<(), textmate_lang::Error>(())
```

### `Grammar::pattern`

```rust,ignore
pub fn pattern(self, rule: impl Into<Rule>) -> Grammar
```

Adds a top-level pattern (`"patterns"`), tried in the order added.

```rust
use textmate_lang::{Grammar, Match};

let json = Grammar::new("source.demo")
    .pattern(Match::new("==").scope("keyword.operator.comparison.demo"))
    .pattern(Match::new("=").scope("keyword.operator.assignment.demo"))
    .to_json()?;
assert!(json.find("comparison").unwrap() < json.find("assignment").unwrap());
# Ok::<(), textmate_lang::Error>(())
```

### `Grammar::repository`

```rust,ignore
pub fn repository(self, name: impl Into<Cow<'static, str>>, rule: impl Into<Rule>) -> Grammar
```

Adds a repository entry (`"repository"`). Names are non-empty ASCII letters,
digits, `_`, `-`, and `.` ([`InvalidName`](#error)), and unique
([`DuplicateRepository`](#error)). Entries are emitted sorted by name.

| Parameter | Meaning |
|---|---|
| `name` | The entry's name; rules include it with [`Rule::include`](#ruleinclude). |
| `rule` | The entry: usually a [`Rule::patterns`](#rulepatterns) list, a match, or a region. |

```rust
use textmate_lang::{Error, Grammar, Match};

let err = Grammar::new("source.demo")
    .repository("x", Match::new("a"))
    .repository("x", Match::new("b"))
    .validate()
    .unwrap_err();
assert_eq!(err, Error::DuplicateRepository { name: "x".into() });
```

### `Grammar::injection`

```rust,ignore
pub fn injection(self, selector: impl Into<Cow<'static, str>>, rule: impl Into<Rule>) -> Grammar
```

Adds an injection (`"injections"`): `rule` is tried, inside this grammar,
wherever the scope stack matches `selector`. Selectors are unique
([`DuplicateInjection`](#error)) and emitted sorted.

```rust
use textmate_lang::{Grammar, Match, Rule};

let json = Grammar::new("source.demo")
    .injection("L:string.quoted.demo", Rule::patterns([Match::new(r"\{\w+\}").scope("variable.demo").into()]))
    .to_json_compact()?;
assert!(json.contains(r#""injections":{"L:string.quoted.demo":{"patterns":["#));
# Ok::<(), textmate_lang::Error>(())
```

### `Grammar::validate`

```rust,ignore
pub fn validate(&self) -> Result<(), Error>
```

Checks the whole grammar without emitting it. The same checks every emitter
runs.

**Errors:** the first problem, as an [`Error`](#error).

```rust
use textmate_lang::{Error, Grammar, Rule};

let err = Grammar::new("source.demo").pattern(Rule::include("nope")).validate().unwrap_err();
assert_eq!(err, Error::UndefinedInclude { path: "/patterns/0/include".into(), name: "nope".into() });
```

### `Grammar::to_json`

```rust,ignore
pub fn to_json(&self) -> Result<String, Error>
```

Validates and returns pretty JSON: two-space indentation, `"key": value`, a
final newline. The Tier-1 call.

**Errors:** the first problem, as an [`Error`](#error).

```rust
use textmate_lang::{Grammar, Match};

let json = Grammar::new("source.demo").pattern(Match::new("a").scope("x.demo")).to_json()?;
assert_eq!(json, "{\n  \"scopeName\": \"source.demo\",\n  \"patterns\": [\n    {\n      \"name\": \"x.demo\",\n      \"match\": \"a\"\n    }\n  ]\n}\n");
# Ok::<(), textmate_lang::Error>(())
```

### `Grammar::to_json_compact`

```rust,ignore
pub fn to_json_compact(&self) -> Result<String, Error>
```

Validates and returns JSON without whitespace.

**Errors:** the first problem, as an [`Error`](#error).

```rust
use textmate_lang::{Grammar, Match};

let json = Grammar::new("source.demo").pattern(Match::new("a").scope("x.demo")).to_json_compact()?;
assert_eq!(json, r#"{"scopeName":"source.demo","patterns":[{"name":"x.demo","match":"a"}]}"#);
# Ok::<(), textmate_lang::Error>(())
```

### `Grammar::write_json`

```rust,ignore
pub fn write_json(&self, out: &mut String, style: Style) -> Result<(), Error>
```

Validates and appends to `out`, reusing its capacity. On an error nothing is
appended.

| Parameter | Meaning |
|---|---|
| `out` | The buffer to append to. |
| `style` | [`Style::Pretty`](#style) or [`Style::Compact`](#style). |

**Errors:** the first problem, as an [`Error`](#error).

```rust
use textmate_lang::{Grammar, Match, Style};

let mut out = String::from("// generated\n");
Grammar::new("source.demo").write_json(&mut out, Style::Compact)?;
assert_eq!(out, "// generated\n{\"scopeName\":\"source.demo\",\"patterns\":[]}");
assert!(Grammar::new("source.demo").pattern(Match::new("(")).write_json(&mut out, Style::Compact).is_err());
assert!(out.ends_with("[]}"));
# Ok::<(), textmate_lang::Error>(())
```

## `Style`

```rust,ignore
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Style { Pretty, Compact }
```

The layout [`write_json`](#grammarwrite_json) uses. `Pretty` (the default) is
two-space indentation with a final newline; `Compact` has no whitespace.

```rust
use textmate_lang::{Grammar, Style};

let mut out = String::new();
Grammar::new("source.demo").write_json(&mut out, Style::default())?;
assert!(out.ends_with("}\n"));
# Ok::<(), textmate_lang::Error>(())
```

## `Rule`

```rust,ignore
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule { /* private */ }
impl From<Match> for Rule {}
impl From<BeginEnd> for Rule {}
impl From<BeginWhile> for Rule {}
```

One entry of a pattern list. [`Match`](#match), [`BeginEnd`](#beginend), and
[`BeginWhile`](#beginwhile) convert into it; includes and lists are built with
the constructors below. Every method that takes a rule takes
`impl Into<Rule>`, so the region and match types can be passed directly.
Dropping a rule is iterative, so a rule nested arbitrarily deep drops without
recursion.

### `Rule::include`

```rust,ignore
pub fn include(name: impl Into<Cow<'static, str>>) -> Rule
```

Includes repository entry `name` (`"include": "#name"`). It must exist
([`UndefinedInclude`](#error)).

```rust
use textmate_lang::{Grammar, Match, Rule};

let json = Grammar::new("source.demo")
    .pattern(Rule::include("numbers"))
    .repository("numbers", Match::new(r"\d+").scope("constant.numeric.demo"))
    .to_json_compact()?;
assert!(json.contains(r##"{"include":"#numbers"}"##));
# Ok::<(), textmate_lang::Error>(())
```

### `Rule::include_self`

```rust,ignore
pub fn include_self() -> Rule
```

Includes this grammar's top-level patterns (`"include": "$self"`).

```rust
use textmate_lang::{BeginEnd, Grammar, Rule};

let json = Grammar::new("source.demo")
    .pattern(BeginEnd::new(r"\{", r"\}").pattern(Rule::include_self()))
    .to_json_compact()?;
assert!(json.contains(r#"{"include":"$self"}"#));
# Ok::<(), textmate_lang::Error>(())
```

### `Rule::include_base`

```rust,ignore
pub fn include_base() -> Rule
```

Includes the base grammar's top-level patterns (`"include": "$base"`): this
grammar's, unless it is embedded in another.

```rust
use textmate_lang::{BeginEnd, Grammar, Rule};

let json = Grammar::new("source.demo")
    .pattern(BeginEnd::new(r"\(", r"\)").pattern(Rule::include_base()))
    .to_json_compact()?;
assert!(json.contains(r#"{"include":"$base"}"#));
# Ok::<(), textmate_lang::Error>(())
```

### `Rule::include_scope`

```rust,ignore
pub fn include_scope(scope: impl Into<Cow<'static, str>>) -> Rule
```

Includes another grammar's top-level patterns by scope
(`"include": "source.js"`). Only the scope's syntax is checked.

```rust
use textmate_lang::{BeginEnd, Grammar, Rule};

let json = Grammar::new("text.html.demo")
    .pattern(BeginEnd::new("<script>", "</script>").pattern(Rule::include_scope("source.js")))
    .to_json_compact()?;
assert!(json.contains(r#"{"include":"source.js"}"#));
# Ok::<(), textmate_lang::Error>(())
```

### `Rule::include_scope_entry`

```rust,ignore
pub fn include_scope_entry(scope: impl Into<Cow<'static, str>>, name: impl Into<Cow<'static, str>>) -> Rule
```

Includes one repository entry of another grammar
(`"include": "source.js#expression"`). When `scope` is this grammar's own,
the entry must exist.

```rust
use textmate_lang::{Grammar, Rule};

let json = Grammar::new("source.demo").pattern(Rule::include_scope_entry("source.js", "expression")).to_json_compact()?;
assert!(json.contains(r##"{"include":"source.js#expression"}"##));
# Ok::<(), textmate_lang::Error>(())
```

### `Rule::patterns`

```rust,ignore
pub fn patterns(rules: impl IntoIterator<Item = Rule>) -> Rule
```

A list of rules tried together (`{"patterns": [...]}`): the usual body of a
repository entry.

```rust
use textmate_lang::{Grammar, Match, Rule};

let json = Grammar::new("source.demo")
    .pattern(Rule::include("literals"))
    .repository("literals", Rule::patterns([
        Match::new(r"\b(true|false)\b").scope("constant.language.demo").into(),
        Match::new(r"\b\d+\b").scope("constant.numeric.demo").into(),
    ]))
    .to_json_compact()?;
assert!(json.contains(r#""literals":{"patterns":[{"name":"constant.language.demo""#));
# Ok::<(), textmate_lang::Error>(())
```

## `Match`

```rust,ignore
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match { /* private */ }
pub fn new(regex: impl Into<Cow<'static, str>>) -> Match
pub fn scope(self, scope: impl Into<Cow<'static, str>>) -> Match
pub fn capture(self, group: u32, capture: impl Into<Capture>) -> Match
```

A rule matching one regular expression (`"match"`). `scope` names the whole
match (`"name"`); `capture` sets what to do with group `group`
(`"captures"`), where `capture` is a scope name or a [`Capture`](#capture).
Groups must exist ([`CaptureOutOfRange`](#error)) and be set once
([`DuplicateCapture`](#error)).

```rust
use textmate_lang::{Grammar, Match};

let json = Grammar::new("source.demo")
    .pattern(
        Match::new(r"\b(fn)\s+([a-z_]\w*)")
            .capture(1, "storage.type.function.demo")
            .capture(2, "entity.name.function.demo"),
    )
    .to_json_compact()?;
assert!(json.contains(r#""captures":{"1":{"name":"storage.type.function.demo"},"2":{"name":"entity.name.function.demo"}}"#));
# Ok::<(), textmate_lang::Error>(())
```

## `BeginEnd`

```rust,ignore
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeginEnd { /* private */ }
pub fn new(begin: impl Into<Cow<'static, str>>, end: impl Into<Cow<'static, str>>) -> BeginEnd
pub fn scope(self, scope) -> BeginEnd                     // "name"
pub fn content_scope(self, scope) -> BeginEnd             // "contentName"
pub fn capture(self, group: u32, capture) -> BeginEnd     // "captures", both sides
pub fn begin_capture(self, group: u32, capture) -> BeginEnd   // "beginCaptures"
pub fn end_capture(self, group: u32, capture) -> BeginEnd     // "endCaptures"
pub fn pattern(self, rule: impl Into<Rule>) -> BeginEnd   // "patterns"
pub fn apply_end_pattern_last(self, last: bool) -> BeginEnd   // "applyEndPatternLast": 1
```

A region from a `begin` match to the next `end` match. Inside, the region's
patterns apply. `scope` covers the whole region, `content_scope` only the
text between the delimiters; both may substitute `begin` captures. Numbered
back-references in `end` name `begin` groups. `capture` applies to each side
that has no captures of its own, and its groups must exist on each side it
applies to; if both sides have their own, it is refused as
[`ShadowedCaptures`](#error). `apply_end_pattern_last` tries `end` after the
region's patterns instead of before.

```rust
use textmate_lang::{BeginEnd, Grammar, Match};

let raw = BeginEnd::new(r#"r(#*)""#, r#""\1"#)
    .scope("string.quoted.raw.demo")
    .begin_capture(1, "punctuation.definition.string.demo");
let block = BeginEnd::new(r"/\*", r"\*/")
    .scope("comment.block.demo")
    .capture(0, "punctuation.definition.comment.demo")
    .pattern(Match::new(r"\bTODO\b").scope("keyword.todo.demo"));
let json = Grammar::new("source.demo").pattern(raw).pattern(block).to_json_compact()?;
assert!(json.contains(r#""end":"\"\\1""#));
# Ok::<(), textmate_lang::Error>(())
```

## `BeginWhile`

```rust,ignore
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeginWhile { /* private */ }
pub fn new(begin: impl Into<Cow<'static, str>>, while_: impl Into<Cow<'static, str>>) -> BeginWhile
pub fn scope(self, scope) -> BeginWhile                   // "name"
pub fn content_scope(self, scope) -> BeginWhile           // "contentName"
pub fn capture(self, group: u32, capture) -> BeginWhile   // "captures", both sides
pub fn begin_capture(self, group: u32, capture) -> BeginWhile  // "beginCaptures"
pub fn while_capture(self, group: u32, capture) -> BeginWhile  // "whileCaptures"
pub fn pattern(self, rule: impl Into<Rule>) -> BeginWhile // "patterns"
```

A region opened by `begin` that continues while each following line starts
with a `while` match, as Markdown block quotes do. Captures and
back-references work as for [`BeginEnd`](#beginend), with `while` in place of
`end`.

```rust
use textmate_lang::{BeginWhile, Grammar, Rule};

let quote = BeginWhile::new(r"^\s*(>) ?", r"^\s*(>) ?")
    .scope("markup.quote.demo")
    .capture(1, "punctuation.definition.quote.demo")
    .pattern(Rule::include_self());
let json = Grammar::new("text.demo").pattern(quote).to_json_compact()?;
assert!(json.contains(r#""while":"^\\s*(>) ?""#));
# Ok::<(), textmate_lang::Error>(())
```

## `Capture`

```rust,ignore
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capture { /* private */ }
pub fn new() -> Capture
pub fn scope(self, scope: impl Into<Cow<'static, str>>) -> Capture   // "name"
pub fn pattern(self, rule: impl Into<Rule>) -> Capture              // "patterns"
impl From<&'static str> for Capture {}
impl From<String> for Capture {}
impl From<Cow<'static, str>> for Capture {}
```

What to do with a capture group's text: scope it, run patterns over it, or
both. A scope name converts into a `Capture` directly.

```rust
use textmate_lang::{Capture, Grammar, Match, Rule};

let json = Grammar::new("source.demo")
    .pattern(Match::new(r#"(import)\s+("[^"]*")"#)
        .capture(1, "keyword.control.import.demo")
        .capture(2, Capture::new().scope("string.quoted.demo").pattern(Rule::include("escapes"))))
    .repository("escapes", Match::new(r"\\.").scope("constant.character.escape.demo"))
    .to_json_compact()?;
assert!(json.contains(r##""2":{"name":"string.quoted.demo","patterns":[{"include":"#escapes"}]}"##));
# Ok::<(), textmate_lang::Error>(())
```

## `validate_regex`

```rust,ignore
pub fn validate_regex(pattern: &str) -> Result<u32, RegexError>
```

Checks one pattern against the [subset](#regex-subset) and returns its number
of capture groups. Numbered back-references must name groups of the pattern
itself. Linear in the pattern's length, iterative.

**Errors:** a [`RegexError`](#regexerror-and-regexerrorkind) with the byte
offset and kind of the first problem.

```rust
use textmate_lang::{RegexErrorKind, validate_regex};

assert_eq!(validate_regex(r"\b(fn)\s+([a-z_]\w*)"), Ok(2));
let err = validate_regex("[a-").unwrap_err();
assert_eq!((err.offset, err.kind), (2, RegexErrorKind::InvalidRange));
```

## `escape_regex`

```rust,ignore
pub fn escape_regex(text: &str) -> String
```

Escapes literal text into a pattern that matches exactly that text, in every
context (plain and extended mode, `end` and `while` patterns), and that stays
safe when concatenated with other escaped text. ASCII letters, digits, `_`,
and non-ASCII characters are copied; other printable ASCII is preceded by
`\`, except `\` itself, which becomes `\x5c`; control characters become
`\xHH`.

```rust
use textmate_lang::{escape_regex, validate_regex};

let operators = ["<=", ">=", "&&", "||", "?."].map(escape_regex);
assert_eq!(operators[0], r"\<\=");
assert_eq!(validate_regex(&operators.join("|")), Ok(0));
assert_eq!(escape_regex(r"C:\1"), r"C\:\x5c1");
```

## `Error`

```rust,ignore
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error { /* variants below */ }
impl Display for Error {}
impl core::error::Error for Error {}   // source(): the RegexError, for InvalidRegex
```

Why a grammar was refused. `path` fields are JSON Pointers into the output.
The enum is `#[non_exhaustive]`: match it with a wildcard arm.

| Variant | Meaning |
|---|---|
| `InvalidRegex { path, error }` | A pattern is outside the [subset](#regex-subset); `error` has the byte offset and kind. |
| `EmptyRegex { path }` | A `match`, `begin`, `end`, `while`, or `firstLineMatch` pattern is empty. |
| `InvalidScope { path, scope, offset, reason }` | A scope name is malformed at byte `offset`. |
| `InvalidSelector { path, selector, offset, reason }` | A selector is malformed at byte `offset`. |
| `InvalidName { path, name, reason }` | A repository name (defined or included) or a file type is malformed. |
| `DuplicateRepository { name }` | Two repository entries share a name. |
| `DuplicateInjection { selector }` | Two injections share a selector. |
| `DuplicateCapture { path, group }` | A captures map sets a group twice. |
| `CaptureOutOfRange { path, group, groups }` | A capture or a `$n` substitution names a missing group. |
| `ShadowedCaptures { path }` | A region's `captures` never applies: both sides have their own. |
| `UndefinedInclude { path, name }` | An include names an entry this grammar does not define. |
| `IncludeCycle { cycle }` | Pattern lists include each other in a cycle; `cycle` lists them, `$self` or `#name`, first repeated at the end. |
| `TooDeep { path }` | Rules nest more than [`MAX_RULE_DEPTH`](#constants) deep. |

```rust
use textmate_lang::{Error, Grammar, Match};

fn explain(err: &Error) -> &'static str {
    match err {
        Error::InvalidRegex { .. } | Error::EmptyRegex { .. } => "fix the pattern",
        Error::CaptureOutOfRange { .. } | Error::DuplicateCapture { .. } => "fix the captures",
        _ => "other",
    }
}

let err = Grammar::new("source.demo").pattern(Match::new("(a)").capture(2, "x")).validate().unwrap_err();
assert_eq!(explain(&err), "fix the captures");
assert_eq!(err.to_string(), "`/patterns/0/captures/2` refers to capture group 2, but the highest group in the pattern is 1");
```

## `RegexError` and `RegexErrorKind`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegexError { pub offset: usize, pub kind: RegexErrorKind }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegexErrorKind { /* variants below */ }
```

Why a pattern was refused: `offset` is the byte offset of the problem in the
pattern, always on a character boundary and at most the pattern's length.

| Kind | Meaning |
|---|---|
| `TrailingBackslash` | The pattern ends with a lone `\`. |
| `UnknownEscape` | An escape outside the subset (unknown letter, octal, `\b` in a class, backslash before a control or non-ASCII character). |
| `InvalidHexEscape` | `\x` without two hex digits or `{1-8 hex digits}`; `\u` without four. |
| `InvalidCodePoint` | A surrogate, a value above 10FFFF, or `\xHH` above 7F. |
| `MalformedProperty` | `\p`/`\P` not followed by `{name}`. |
| `UnknownProperty` | A property name outside the supported list. |
| `UnclosedGroup` | A `(` without `)`. |
| `UnmatchedParen` | A `)` without `(`. |
| `UnknownGroup` | A group form outside the subset. |
| `UnknownOption` | An option other than `i`, `m`, `x`, or none at all. |
| `UnclosedComment` | A `(?#` without `)`. |
| `InvalidGroupName` | A group or back-reference name that is not an ASCII identifier. |
| `DuplicateGroupName` | Two groups share a name. |
| `MixedGroups` | Named and unnamed capturing groups in one pattern. |
| `NumberedBackrefWithNamedGroups` | `\1` in a pattern with named groups. |
| `UndefinedGroupName` | `\k<name>` names no group. |
| `BackreferenceOutOfRange { group, groups }` | `\n` beyond the groups available. |
| `UnclosedClass` | A `[` without `]`. |
| `EmptyClass` | `[]`, `[^]`, or an empty side of `&&`. |
| `UnescapedBracket` | A `]` outside a class. |
| `InvalidRange` | A backwards range, or a misplaced `-`. |
| `UnknownPosixClass` | `[:` not starting a supported POSIX bracket. |
| `NothingToRepeat` | A quantifier with nothing before it. |
| `RepeatedQuantifier` | A quantifier directly after another. |
| `AmbiguousQuantifier` | `{n}?`, `{n}+`, or `{n,m}+`. |
| `InvalidInterval` | A malformed `{...}`, a count above 100000, or `n > m`. |
| `QuantifiedAssertion` | A quantifier on an anchor or look-around. |
| `LookbehindNotFixed` | A look-behind alternative of variable length. |
| `CaptureInLookbehind` | A capture group or back-reference in a look-behind. |
| `TooDeep` | Nesting beyond [`MAX_REGEX_DEPTH`](#constants). |
| `NulCharacter` | A raw NUL character. |
| `BackslashBeforeDigit` | `\\` followed by a digit in an `end` or `while` pattern. |
| `TooManyGroups` | More capture groups than fit in a `u32`. |

```rust
use textmate_lang::{RegexError, RegexErrorKind, validate_regex};

let err = validate_regex("ab)").unwrap_err();
assert_eq!(err, RegexError { offset: 2, kind: RegexErrorKind::UnmatchedParen });
assert_eq!(err.to_string(), "at byte 2: unmatched `)`");
```

## Constants

```rust,ignore
pub const MAX_RULE_DEPTH: usize = 128;
pub const MAX_REGEX_DEPTH: usize = 256;
```

`MAX_RULE_DEPTH` bounds how deep rules nest (regions and captures with
patterns, and lists inside them; includes do not count): editors build their
rule tables recursively, and real grammars nest a handful of levels.
`MAX_REGEX_DEPTH` bounds group and class nesting in one pattern, below
Oniguruma's own limit of 4096.

```rust
use textmate_lang::{MAX_REGEX_DEPTH, MAX_RULE_DEPTH, validate_regex};

assert_eq!(MAX_RULE_DEPTH, 128);
let deep = "(?:".repeat(MAX_REGEX_DEPTH) + "a" + &")".repeat(MAX_REGEX_DEPTH);
assert_eq!(validate_regex(&deep), Ok(0));
```

## Feature flags

| Feature | Default | Effect |
|---|---|---|
| `std` | yes | The standard library. Without it the crate is `no_std` and needs only `alloc`. |

The crate has no dependencies.

## Limits

| Limit | Value | When exceeded |
|---|---|---|
| Rule nesting | [`MAX_RULE_DEPTH`](#constants) = 128 | [`Error::TooDeep`](#error) |
| Group and class nesting in a pattern | [`MAX_REGEX_DEPTH`](#constants) = 256 | `RegexErrorKind::TooDeep` |
| Quantifier counts | 100000 (Oniguruma's maximum) | `RegexErrorKind::InvalidInterval` |
| Selector parenthesis nesting | 32 | [`Error::InvalidSelector`](#error) |

Validation sorts the repository and injections once and resolves includes by
binary search; everything else is a single pass. Validation and emission are
`O(n log n)` in the size of the grammar and never recurse, so no grammar can
overflow the stack; the tests validate and emit grammars of 100,000 entries
and patterns of 100,000 alternatives.
