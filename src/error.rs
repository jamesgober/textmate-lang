//! Why a grammar could not be emitted.

use alloc::{string::String, vec::Vec};
use core::fmt;

use crate::RegexError;

/// Why [`Grammar`](crate::Grammar) refused to emit a grammar.
///
/// Emitting validates the whole grammar first and reports the first problem.
/// Every variant that has a `path` locates the problem with a
/// [JSON Pointer](https://www.rfc-editor.org/rfc/rfc6901) into the JSON that
/// would have been emitted: `/patterns/2/begin`, `/repository/string/captures/1/name`.
/// Repository and injection entries are addressed by name, so a path is the
/// same whatever order the entries were added in.
///
/// The checks run in this order: the grammar's `scopeName`, `fileTypes`,
/// `firstLineMatch`, and `injectionSelector`; repository names and injection
/// selectors (and their uniqueness); then every rule — the top-level
/// patterns, the repository in name order, the injections in selector order
/// — each checked before the rules nested in it; finally include cycles.
/// Within a rule, its regular expressions are checked first, then its scope
/// names, then its captures.
///
/// # Examples
///
/// ```
/// use textmate_lang::{Error, Grammar, Rule};
///
/// let grammar = Grammar::new("source.demo").pattern(Rule::include("strings"));
/// match grammar.to_json() {
///     Err(Error::UndefinedInclude { path, name }) => {
///         assert_eq!(path, "/patterns/0/include");
///         assert_eq!(name, "strings");
///     }
///     other => panic!("unexpected {other:?}"),
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A regular expression is outside the supported Oniguruma subset; see
    /// [`validate_regex`](crate::validate_regex) and
    /// [`RegexErrorKind`](crate::RegexErrorKind) for the fix.
    InvalidRegex {
        /// Where the pattern is.
        path: String,
        /// The byte offset in the pattern and what is wrong there.
        error: RegexError,
    },

    /// A `match`, `begin`, `end`, `while`, or `firstLineMatch` pattern is
    /// empty. An empty pattern matches everywhere without consuming text,
    /// which tokenizers either loop on or detect and skip.
    ///
    /// Give the pattern something to match; to end a region at once, use a
    /// look-ahead such as `(?=.)` or `$`.
    EmptyRegex {
        /// Where the pattern is.
        path: String,
    },

    /// A scope name is malformed: empty, with an empty segment, with a
    /// character outside ASCII letters, digits, `_`, `+`, and `-`, with a
    /// malformed `$n` substitution, or with a substitution where none is
    /// allowed (`scopeName`, external includes).
    ///
    /// Scopes are dot-separated segments such as `keyword.control.demo`; a
    /// rule's name may list several separated by single spaces.
    InvalidScope {
        /// Where the scope is.
        path: String,
        /// The scope as given.
        scope: String,
        /// Byte offset of the problem in `scope`.
        offset: usize,
        /// What is wrong.
        reason: &'static str,
    },

    /// A scope selector (`injectionSelector` or an `injections` key) is
    /// malformed, in the syntax VS Code's `vscode-textmate` reads: scopes
    /// combined with `,`, `|` (inside parentheses), `-`, and `(...)`, each
    /// top-level entry optionally prefixed with `L:` or `R:`.
    InvalidSelector {
        /// Where the selector is.
        path: String,
        /// The selector as given.
        selector: String,
        /// Byte offset of the problem in `selector`.
        offset: usize,
        /// What is wrong.
        reason: &'static str,
    },

    /// A repository name (defined or included) or a file type is
    /// malformed. Repository names are non-empty ASCII letters, digits, `_`,
    /// `-`, and `.`; file types are non-empty.
    InvalidName {
        /// Where the name is.
        path: String,
        /// The name as given.
        name: String,
        /// What is wrong.
        reason: &'static str,
    },

    /// Two repository entries share a name. The JSON object could hold only
    /// one, so the second would silently replace the first.
    DuplicateRepository {
        /// The name defined more than once.
        name: String,
    },

    /// Two injections share a selector.
    DuplicateInjection {
        /// The selector given more than once.
        selector: String,
    },

    /// A captures map sets the same group twice.
    DuplicateCapture {
        /// The captures map.
        path: String,
        /// The repeated group number.
        group: u32,
    },

    /// A capture or a `$n` substitution names a group the pattern does not
    /// have. For `captures` on a region, the group must exist in every
    /// pattern the captures apply to (`begin`, and `end` or `while`, unless
    /// those have their own captures); for a region's `name` and
    /// `contentName`, in `begin`.
    CaptureOutOfRange {
        /// Where the capture or the substitution is.
        path: String,
        /// The group named.
        group: u32,
        /// How many groups the pattern has.
        groups: u32,
    },

    /// A region's `captures` can never apply: the region has both
    /// `beginCaptures` and `endCaptures` (or `whileCaptures`), which take
    /// precedence. Move the captures into those, or remove them.
    ShadowedCaptures {
        /// Where the region is.
        path: String,
    },

    /// An include names a repository entry this grammar does not define.
    UndefinedInclude {
        /// Where the include is.
        path: String,
        /// The entry it names.
        name: String,
    },

    /// Includes form a cycle in which no rule consumes text: an entry that
    /// is only a list of patterns (or one include) includes itself through
    /// other such entries, or through `$self` / `$base` from the top-level
    /// patterns. Tokenizers expand these lists when building their scanners
    /// and recurse forever: `vscode-textmate` collects them with a recursion
    /// that keeps no record of lists already visited.
    ///
    /// Recursion through a region is fine — a `begin`/`end` rule whose
    /// patterns include the entry that contains it is how nested
    /// constructs are written — and so is recursion through captures.
    ///
    /// `cycle` lists the entries on the cycle, `$self` for the top-level
    /// patterns and `#name` for repository entries, starting and ending with
    /// the same one.
    IncludeCycle {
        /// The entries on the cycle.
        cycle: Vec<String>,
    },

    /// Rules nest more than [`MAX_RULE_DEPTH`](crate::MAX_RULE_DEPTH) deep
    /// (regions and captures inside regions and captures). Editors build
    /// their rule tables recursively, so a deeper grammar can crash them;
    /// use repository entries and includes instead.
    TooDeep {
        /// Where the limit was crossed.
        path: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRegex { path, error } => {
                write!(f, "invalid regular expression at `{path}`, {error}")
            }
            Self::EmptyRegex { path } => write!(f, "empty regular expression at `{path}`"),
            Self::InvalidScope {
                path,
                scope,
                offset,
                reason,
            } => write!(
                f,
                "invalid scope name {scope:?} at `{path}`, byte {offset}: {reason}"
            ),
            Self::InvalidSelector {
                path,
                selector,
                offset,
                reason,
            } => write!(
                f,
                "invalid scope selector {selector:?} at `{path}`, byte {offset}: {reason}"
            ),
            Self::InvalidName { path, name, reason } => {
                write!(f, "invalid name {name:?} at `{path}`: {reason}")
            }
            Self::DuplicateRepository { name } => {
                write!(f, "repository entry {name:?} is defined more than once")
            }
            Self::DuplicateInjection { selector } => {
                write!(f, "injection selector {selector:?} is given more than once")
            }
            Self::DuplicateCapture { path, group } => {
                write!(f, "capture group {group} is set more than once at `{path}`")
            }
            Self::CaptureOutOfRange {
                path,
                group,
                groups,
            } => write!(
                f,
                "`{path}` refers to capture group {group}, but the highest group in the pattern is {groups}"
            ),
            Self::ShadowedCaptures { path } => write!(
                f,
                "`{path}/captures` never applies: both sides of the region have their own captures"
            ),
            Self::UndefinedInclude { path, name } => {
                write!(
                    f,
                    "`{path}` includes repository entry {name:?}, which is not defined"
                )
            }
            Self::IncludeCycle { cycle } => {
                f.write_str("includes form a cycle that consumes no text: ")?;
                for (i, entry) in cycle.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" -> ")?;
                    }
                    f.write_str(entry)?;
                }
                Ok(())
            }
            Self::TooDeep { path } => write!(
                f,
                "rules nest more than {} deep at `{path}`",
                crate::MAX_RULE_DEPTH
            ),
        }
    }
}

impl core::error::Error for Error {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::InvalidRegex { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::{string::ToString, vec};

    use super::*;
    use crate::RegexErrorKind;

    #[test]
    fn test_every_variant_displays_its_details() {
        let p = || String::from("/p");
        let all = [
            Error::InvalidRegex {
                path: p(),
                error: RegexError {
                    offset: 3,
                    kind: RegexErrorKind::UnclosedGroup,
                },
            },
            Error::EmptyRegex { path: p() },
            Error::InvalidScope {
                path: p(),
                scope: "a..b".into(),
                offset: 2,
                reason: "r",
            },
            Error::InvalidSelector {
                path: p(),
                selector: "(".into(),
                offset: 0,
                reason: "r",
            },
            Error::InvalidName {
                path: p(),
                name: "a b".into(),
                reason: "r",
            },
            Error::DuplicateCapture {
                path: p(),
                group: 1,
            },
            Error::CaptureOutOfRange {
                path: p(),
                group: 3,
                groups: 2,
            },
            Error::ShadowedCaptures { path: p() },
            Error::UndefinedInclude {
                path: p(),
                name: "x".into(),
            },
            Error::TooDeep { path: p() },
        ];
        for e in all {
            assert!(e.to_string().contains("/p"), "{e}");
        }
        assert!(
            Error::DuplicateRepository { name: "s".into() }
                .to_string()
                .contains("\"s\"")
        );
        assert!(
            Error::DuplicateInjection {
                selector: "L:x".into()
            }
            .to_string()
            .contains("L:x")
        );
        let cycle = Error::IncludeCycle {
            cycle: vec!["#a".into(), "#b".into(), "#a".into()],
        };
        assert!(cycle.to_string().ends_with("#a -> #b -> #a"));
    }

    #[test]
    fn test_regex_error_is_the_source() {
        use core::error::Error as _;
        let e = Error::InvalidRegex {
            path: "/x".into(),
            error: RegexError {
                offset: 0,
                kind: RegexErrorKind::TrailingBackslash,
            },
        };
        assert!(e.source().is_some());
        assert!(Error::EmptyRegex { path: "/x".into() }.source().is_none());
        assert_eq!(
            e.to_string(),
            "invalid regular expression at `/x`, at byte 0: the pattern ends with a lone `\\`"
        );
    }
}
