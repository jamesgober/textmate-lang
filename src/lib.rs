//! # textmate_lang
//!
//! Generate [TextMate grammars](https://macromates.com/manual/en/language_grammars)
//! (`tmLanguage.json`) from Rust: describe a language's tokens with
//! [`Grammar`] and [`Rule`]s, and emit the JSON that VS Code, Sublime Text,
//! TextMate, Shiki, and other editors and highlighters load to highlight it,
//! no tree-sitter required.
//!
//! Emitting validates everything a grammar file cannot check for itself.
//! Every regular expression is held to a documented subset of Oniguruma
//! syntax, and refused with a byte offset and a reason when it strays
//! outside. Scope names, scope selectors, captures, and includes are checked;
//! includes that would send a tokenizer into endless recursion are found.
//! The JSON writer is the crate's own and escapes every string it writes, so
//! no name, pattern, or scope can change the structure of the output.
//!
//! ## The pieces
//!
//! - [`Grammar`] — the scope name, top-level patterns, the repository of
//!   named rules, and injections. [`Grammar::to_json`] validates it and
//!   returns the JSON.
//! - [`Rule`] — an entry of a pattern list: a [`Match`], a [`BeginEnd`] or
//!   [`BeginWhile`] region, an include ([`Rule::include`] and friends), or a
//!   list ([`Rule::patterns`]). [`Capture`] says what to do with a capture
//!   group.
//! - [`Error`] — what validation found, and where, as a JSON Pointer into
//!   the output.
//! - [`validate_regex`] and [`escape_regex`] — the regular-expression check
//!   on its own, and literal text made safe to match.
//!
//! ## Example
//!
//! ```
//! use textmate_lang::{BeginEnd, Grammar, Match, Rule, escape_regex};
//!
//! let operators = ["+", "-", "*", "/", "=="].map(escape_regex).join("|");
//! let json = Grammar::new("source.calc")
//!     .name("Calc")
//!     .file_type("calc")
//!     .pattern(Rule::include("comment"))
//!     .pattern(Match::new(r"\b\d+(\.\d+)?\b").scope("constant.numeric.calc"))
//!     .pattern(Match::new(operators).scope("keyword.operator.calc"))
//!     .pattern(
//!         Match::new(r"\b(let)\s+([a-z_]\w*)")
//!             .capture(1, "storage.type.calc")
//!             .capture(2, "variable.other.calc"),
//!     )
//!     .repository(
//!         "comment",
//!         BeginEnd::new("#", "$")
//!             .scope("comment.line.number-sign.calc")
//!             .begin_capture(0, "punctuation.definition.comment.calc"),
//!     )
//!     .to_json()?;
//!
//! assert!(json.contains(r#""match": "\\+|\\-|\\*|\\/|\\=\\=""#));
//! // std::fs::write("syntaxes/calc.tmLanguage.json", json)?;
//! # Ok::<(), textmate_lang::Error>(())
//! ```
//!
//! ## What is checked
//!
//! - **Regular expressions** (`match`, `begin`, `end`, `while`,
//!   `firstLineMatch`) are non-empty and in the subset `docs/API.md`
//!   documents: balanced groups and classes, known escapes, quantifiers with
//!   one meaning in every engine, fixed-length look-behind, back-references
//!   to groups that exist (in `end` and `while`, to groups of `begin`), and
//!   no mixing of named and numbered groups.
//! - **Scope names** are dot-separated segments of ASCII letters, digits,
//!   `_`, `+`, and `-`; rule names may list several scopes and substitute
//!   capture text with `$n`, which must name an existing group.
//! - **Captures** name groups the pattern has, each at most once.
//! - **Includes** of repository entries resolve; includes that form a cycle
//!   of pattern lists, which tokenizers expand forever, are refused, while
//!   recursion through a region (how nested constructs are written) is
//!   accepted.
//! - **Selectors** (`injectionSelector`, injection keys) parse.
//! - Rules nest at most [`MAX_RULE_DEPTH`] deep and patterns at most
//!   [`MAX_REGEX_DEPTH`]; validation and emission are iterative.
//!
//! What cannot be checked here: rules in other grammars that external
//! includes name, and whether a pattern matches what its author intended.
//!
//! ## Features
//!
//! - `std` (default) — the standard library. Without it the crate is `no_std`
//!   and needs only `alloc`. There are no dependencies.

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![deny(
    warnings,
    missing_docs,
    unsafe_op_in_unsafe_fn,
    unused_must_use,
    unused_results,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable,
    clippy::dbg_macro,
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::undocumented_unsafe_blocks
)]

extern crate alloc;

mod check;
mod error;
mod grammar;
mod json;
mod regex;
mod rule;
mod scope;

pub use check::MAX_RULE_DEPTH;
pub use error::Error;
pub use grammar::Grammar;
pub use json::Style;
pub use regex::{MAX_REGEX_DEPTH, RegexError, RegexErrorKind, escape_regex, validate_regex};
pub use rule::{BeginEnd, BeginWhile, Capture, Match, Rule};

/// Compiles and runs the `rust` code blocks in `README.md` and `docs/API.md` as
/// part of `cargo test`, so the published examples cannot drift from the API.
///
/// Present only while collecting doctests (`#[cfg(doctest)]`); it is not part of
/// the public surface and does not appear in the built library or its docs.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
#[doc = include_str!("../docs/API.md")]
pub struct MarkdownDocTests;
