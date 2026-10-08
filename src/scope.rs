//! Scope names, scope selectors, and repository names.
//!
//! A scope is dot-separated segments (`keyword.control.rust`). A rule's
//! `name` may hold several scopes separated by single spaces, and may
//! substitute capture text with `$n` or `${n:/downcase}` / `${n:/upcase}`,
//! the forms VS Code's `vscode-textmate` expands. A scope selector
//! (`injectionSelector`, the keys of `injections`) combines scopes with
//! `,`, `|`, `-`, and parentheses, optionally prefixed by `L:` or `R:`.
//!
//! Every checker here is a single forward pass; the selector checker's
//! nesting is capped at [`MAX_SELECTOR_DEPTH`].

/// How deep parentheses may nest in a scope selector.
pub(crate) const MAX_SELECTOR_DEPTH: usize = 32;

/// Why a scope or selector was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Problem {
    /// Malformed at this byte offset, for this reason.
    At(usize, &'static str),
    /// A capture substitution names this group, which does not exist.
    Group(u32),
}

/// Checks a single scope with no substitutions: a grammar's `scopeName`, or
/// the scope of an external include.
pub(crate) fn single(text: &str) -> Result<(), Problem> {
    if let Some(at) = text.find(' ') {
        return Err(Problem::At(at, "a single scope cannot contain spaces"));
    }
    list(text, None)
}

/// Checks a space-separated list of scopes. `groups` is the number of
/// capture groups that `$n` substitutions may name, or `None` if
/// substitutions are not allowed here.
pub(crate) fn list(text: &str, groups: Option<u32>) -> Result<(), Problem> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return Err(Problem::At(0, "scope name is empty"));
    }
    let mut at = 0;
    // Whether the next character starts a segment.
    let mut segment_start = true;
    while let Some(&b) = bytes.get(at) {
        match b {
            b'.' | b' ' => {
                if segment_start {
                    return Err(Problem::At(
                        at,
                        "empty segment: `.` or ` ` where a name part belongs",
                    ));
                }
                segment_start = true;
                at += 1;
            }
            b'$' => {
                let Some(groups) = groups else {
                    return Err(Problem::At(
                        at,
                        "capture substitutions (`$n`) are not allowed here",
                    ));
                };
                at = substitution(bytes, at, groups)?;
                segment_start = false;
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' => {
                segment_start = false;
                at += 1;
            }
            b'+' | b'-' if !segment_start => at += 1,
            b'+' | b'-' => {
                return Err(Problem::At(
                    at,
                    "a segment must start with a letter, digit, or `_`",
                ));
            }
            _ => {
                return Err(Problem::At(
                    at,
                    "scope names may contain only ASCII letters, digits, `_`, `+`, `-`, `.`, and single spaces",
                ));
            }
        }
    }
    if segment_start {
        return Err(Problem::At(
            text.len() - 1,
            "scope name ends with `.` or ` `",
        ));
    }
    Ok(())
}

/// `$n` or `${n:/downcase}` / `${n:/upcase}` at `at`; returns the offset
/// after it.
fn substitution(bytes: &[u8], at: usize, groups: u32) -> Result<usize, Problem> {
    const MALFORMED: &str = "`$` must start `$n`, `${n:/downcase}`, or `${n:/upcase}`";
    let braced = bytes.get(at + 1) == Some(&b'{');
    let digits_at = at + 1 + usize::from(braced);
    let mut end = digits_at;
    let mut group: u32 = 0;
    while let Some(&d @ b'0'..=b'9') = bytes.get(end) {
        group = group.saturating_mul(10).saturating_add(u32::from(d - b'0'));
        end += 1;
    }
    if end == digits_at {
        return Err(Problem::At(at, MALFORMED));
    }
    if braced {
        let rest = bytes.get(end..).unwrap_or(&[]);
        let suffix = [&b":/downcase}"[..], b":/upcase}"]
            .into_iter()
            .find(|s| rest.starts_with(s))
            .ok_or(Problem::At(at, MALFORMED))?;
        end += suffix.len();
    }
    if group > groups {
        return Err(Problem::Group(group));
    }
    Ok(end)
}

/// Checks a repository entry name: ASCII letters, digits, `_`, `-`, and `.`.
pub(crate) fn repository_name(text: &str) -> Result<(), &'static str> {
    if text.is_empty() {
        return Err("repository names cannot be empty");
    }
    if text
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        Ok(())
    } else {
        Err("repository names may contain only ASCII letters, digits, `_`, `-`, and `.`")
    }
}

/// Checks a scope selector in the syntax `vscode-textmate` reads.
///
/// ```text
/// selector    = entry ("," entry)*
/// entry       = ("L:" | "R:")? conjunction
/// conjunction = operand+
/// operand     = "-" operand | "(" inner ")" | scope+
/// inner       = conjunction (("," | "|") conjunction)*
/// ```
///
/// Whitespace may separate any two tokens, and must separate two scopes.
pub(crate) fn selector(text: &str) -> Result<(), Problem> {
    let mut s = Selector {
        bytes: text.as_bytes(),
        at: 0,
    };
    s.skip_space();
    if s.peek().is_none() {
        return Err(Problem::At(0, "selector is empty"));
    }
    loop {
        s.skip_space();
        if matches!(s.peek(), Some(b'L' | b'R')) && s.bytes.get(s.at + 1) == Some(&b':') {
            s.at += 2;
        }
        s.conjunction(0)?;
        s.skip_space();
        match s.peek() {
            None => return Ok(()),
            Some(b',') => s.at += 1,
            Some(_) => return Err(s.unexpected()),
        }
    }
}

struct Selector<'s> {
    bytes: &'s [u8],
    at: usize,
}

impl Selector<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.at += 1;
        }
    }

    fn unexpected(&self) -> Problem {
        match self.peek() {
            None => Problem::At(
                self.bytes.len(),
                "selector ends where a scope or `(` belongs",
            ),
            Some(b')') => Problem::At(self.at, "unmatched `)`"),
            Some(_) => Problem::At(
                self.at,
                "unexpected character; selectors use scopes, `,`, `|`, `-`, `(`, `)`, and `L:`/`R:` prefixes",
            ),
        }
    }

    /// One or more operands.
    fn conjunction(&mut self, depth: usize) -> Result<(), Problem> {
        self.operand(depth)?;
        loop {
            self.skip_space();
            match self.peek() {
                Some(b'-' | b'(') => self.operand(depth)?,
                Some(b) if starts_scope(b) => self.operand(depth)?,
                _ => return Ok(()),
            }
        }
    }

    fn operand(&mut self, depth: usize) -> Result<(), Problem> {
        self.skip_space();
        // Negations chain without nesting.
        while self.peek() == Some(b'-') {
            self.at += 1;
            self.skip_space();
        }
        match self.peek() {
            Some(b'(') => {
                let open = self.at;
                if depth >= MAX_SELECTOR_DEPTH {
                    return Err(Problem::At(open, "parentheses nest too deeply"));
                }
                self.at += 1;
                self.inner(depth + 1)?;
                self.skip_space();
                if self.peek() != Some(b')') {
                    return Err(Problem::At(open, "unclosed `(`"));
                }
                self.at += 1;
                Ok(())
            }
            Some(b) if starts_scope(b) => {
                self.scope()?;
                // A path: further scopes separated by whitespace.
                loop {
                    let before = self.at;
                    self.skip_space();
                    if self.at > before && self.peek().is_some_and(starts_scope) {
                        self.scope()?;
                    } else {
                        self.at = before;
                        return Ok(());
                    }
                }
            }
            _ => Err(self.unexpected()),
        }
    }

    fn inner(&mut self, depth: usize) -> Result<(), Problem> {
        loop {
            self.conjunction(depth)?;
            self.skip_space();
            match self.peek() {
                Some(b',' | b'|') => self.at += 1,
                _ => return Ok(()),
            }
        }
    }

    /// One scope: dot-separated segments of `[A-Za-z0-9_][A-Za-z0-9_-]*`.
    fn scope(&mut self) -> Result<(), Problem> {
        let mut segment_start = true;
        while let Some(b) = self.peek() {
            match b {
                b'.' if segment_start => {
                    return Err(Problem::At(self.at, "empty scope segment"));
                }
                b'.' => segment_start = true,
                b'-' if segment_start => {
                    return Err(Problem::At(
                        self.at,
                        "a segment must start with a letter, digit, or `_`",
                    ));
                }
                b if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' => segment_start = false,
                _ => break,
            }
            self.at += 1;
        }
        if segment_start {
            return Err(Problem::At(
                self.at.saturating_sub(1),
                "scope ends with `.`",
            ));
        }
        Ok(())
    }
}

fn starts_scope(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_lists() {
        for ok in [
            "source.rust",
            "keyword.control.c++",
            "meta.tag string.quoted.double",
            "text.html.markdown",
            "entity.name.function.objective-c",
            "a",
        ] {
            assert_eq!(list(ok, None), Ok(()), "{ok}");
        }
        let bad = [
            ("", 0),
            (".a", 0),
            ("a.", 1),
            ("a..b", 2),
            ("a  b", 2),
            (" a", 0),
            ("a ", 1),
            ("a.-b", 2),
            ("a#b", 1),
            ("a\nb", 1),
            ("ключ", 0),
        ];
        for (text, at) in bad {
            assert!(
                matches!(list(text, Some(9)), Err(Problem::At(o, _)) if o == at),
                "{text:?}"
            );
        }
    }

    #[test]
    fn test_substitutions() {
        assert_eq!(list("keyword.$1.x", Some(1)), Ok(()));
        assert_eq!(list("entity.${2:/downcase}", Some(2)), Ok(()));
        assert_eq!(list("x.${0:/upcase}.y", Some(0)), Ok(()));
        assert_eq!(list("x.$3", Some(2)), Err(Problem::Group(3)));
        assert!(matches!(list("x.$", Some(2)), Err(Problem::At(2, _))));
        assert!(matches!(list("x.${1}", Some(2)), Err(Problem::At(2, _))));
        assert!(matches!(
            list("x.${1:/title}", Some(2)),
            Err(Problem::At(2, _))
        ));
        assert!(matches!(list("x.$1", None), Err(Problem::At(2, _))));
        assert_eq!(
            list("x.$99999999999", Some(3)),
            Err(Problem::Group(u32::MAX))
        );
    }

    #[test]
    fn test_single_scope() {
        assert_eq!(single("source.js"), Ok(()));
        assert!(matches!(single("source js"), Err(Problem::At(6, _))));
        assert!(single("source.$1").is_err());
    }

    #[test]
    fn test_repository_names() {
        assert!(repository_name("string-content.v2_x").is_ok());
        assert!(repository_name("").is_err());
        assert!(repository_name("a#b").is_err());
        assert!(repository_name("$self").is_err());
        assert!(repository_name("a b").is_err());
    }

    #[test]
    fn test_selectors() {
        for ok in [
            "source.js",
            "L:source.js -comment -string",
            "R:text.html.markdown, L:source.ts",
            "source.js meta.embedded (string | comment)",
            "-comment",
            "(a, b) - (c | d)",
            "  source.x  ",
            "- - a",
        ] {
            assert_eq!(selector(ok), Ok(()), "{ok}");
        }
        let bad = [
            ("", 0),
            ("   ", 0),
            ("a,", 2),
            (",a", 0),
            ("a,,b", 2),
            ("(a", 0),
            ("a)", 1),
            ("()", 1),
            ("a.", 1),
            ("a.-b", 2),
            ("B:source.x", 1),
            ("L:", 2),
            ("source.c++", 8),
            ("a || b", 2),
            ("-", 1),
        ];
        for (text, at) in bad {
            assert!(
                matches!(selector(text), Err(Problem::At(o, _)) if o == at),
                "{text:?} -> {:?}",
                selector(text)
            );
        }
    }

    #[test]
    fn test_selector_depth_is_capped() {
        let ok = "(".repeat(MAX_SELECTOR_DEPTH) + "a" + &")".repeat(MAX_SELECTOR_DEPTH);
        assert_eq!(selector(&ok), Ok(()));
        let deep = "(".repeat(MAX_SELECTOR_DEPTH + 1) + "a" + &")".repeat(MAX_SELECTOR_DEPTH + 1);
        assert!(matches!(
            selector(&deep),
            Err(Problem::At(MAX_SELECTOR_DEPTH, _))
        ));
        // Long negation chains do not nest.
        let negations = "-".repeat(100_000) + "a";
        assert_eq!(selector(&negations), Ok(()));
    }
}
