//! Validation of the Oniguruma regular-expression subset TextMate grammars
//! may use.
//!
//! TextMate grammars are matched by Oniguruma (`vscode-oniguruma` in VS Code,
//! the native library in TextMate and others). Oniguruma accepts far more than
//! is portable, and treats some spellings differently from other engines, so
//! this module accepts a documented subset and refuses everything else with
//! the byte offset of the first problem. The subset is listed in
//! `docs/API.md#regex-subset`.
//!
//! The checker is one left-to-right pass over the pattern with an explicit
//! group stack, so its cost is linear in the pattern's length and it never
//! recurses. Groups and classes nest at most [`MAX_REGEX_DEPTH`] deep.

use alloc::{string::String, vec::Vec};
use core::fmt;

/// How deep groups and character classes may nest in one pattern.
///
/// Oniguruma's own default limit is far higher (4096); this one is low enough
/// that every engine that loads TextMate grammars handles it, and far above
/// what a hand-written or generated pattern needs.
///
/// # Examples
///
/// ```
/// use textmate_lang::{MAX_REGEX_DEPTH, validate_regex};
///
/// let deep = "(".repeat(MAX_REGEX_DEPTH + 1) + &")".repeat(MAX_REGEX_DEPTH + 1);
/// assert!(validate_regex(&deep).is_err());
/// ```
pub const MAX_REGEX_DEPTH: usize = 256;

/// The largest repetition count Oniguruma accepts (`ONIG_MAX_REPEAT_NUM`).
const MAX_REPEAT: u32 = 100_000;

/// Why a pattern is outside the accepted subset, and where.
///
/// `offset` is the byte offset in the pattern where the problem starts; it is
/// always on a character boundary and never past the end of the pattern.
///
/// # Examples
///
/// ```
/// use textmate_lang::{RegexErrorKind, validate_regex};
///
/// let err = validate_regex("a(b|c").unwrap_err();
/// assert_eq!(err.offset, 1);
/// assert_eq!(err.kind, RegexErrorKind::UnclosedGroup);
/// assert_eq!(err.to_string(), "at byte 1: unclosed group: `(` has no matching `)`");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegexError {
    /// Byte offset of the problem in the pattern.
    pub offset: usize,
    /// What the problem is.
    pub kind: RegexErrorKind,
}

/// What is wrong with a pattern. See [`RegexError`].
///
/// Every variant names a construct that Oniguruma refuses, that some engine
/// loading TextMate grammars refuses, or that engines read differently. The
/// fix for each is in its documentation.
///
/// # Examples
///
/// ```
/// use textmate_lang::{RegexErrorKind, validate_regex};
///
/// assert_eq!(validate_regex("x{2}?").unwrap_err().kind, RegexErrorKind::AmbiguousQuantifier);
/// assert_eq!(validate_regex("[z-a]").unwrap_err().kind, RegexErrorKind::InvalidRange);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegexErrorKind {
    /// The pattern ends with a lone `\`. Write `\\` for a backslash.
    TrailingBackslash,
    /// An escape outside the subset: an unknown letter (`\q`, `\K`, `\R`), an
    /// octal escape (`\0`, `\012`), `\b` inside a class (a backspace there, a
    /// word boundary elsewhere), a back-reference inside a class, or a
    /// backslash before a control or non-ASCII character. Write the character
    /// itself, or `\xHH` / `\x{H...}` for a code point.
    UnknownEscape,
    /// `\x` is not followed by exactly two hex digits or `{` one to eight hex
    /// digits `}`, or `\u` is not followed by exactly four hex digits.
    InvalidHexEscape,
    /// A hex escape names a surrogate or a value above U+10FFFF, or `\xHH`
    /// names a byte above 7F (Oniguruma reads that as a raw UTF-8 byte). Use
    /// `\x{HH}` for code points above 7F.
    InvalidCodePoint,
    /// `\p` or `\P` is not followed by `{name}` (the one-letter form `\pL` is
    /// not portable).
    MalformedProperty,
    /// `\p{...}` names a property outside the supported list (Unicode
    /// general categories, a set of scripts and binary properties, and the
    /// POSIX classes). See `docs/API.md#regex-subset`.
    UnknownProperty,
    /// A `(` has no matching `)`.
    UnclosedGroup,
    /// A `)` has no matching `(`.
    UnmatchedParen,
    /// A group form outside the subset: conditionals `(?(...)`, absent
    /// operators `(?~...)`, callouts, `(?P<...>)`, `(?'name'...)`, and the
    /// like.
    UnknownGroup,
    /// An inline option other than `i`, `m`, and `x`, or an option group with
    /// no options (`(?)`, `(?-)`).
    UnknownOption,
    /// A `(?#` comment has no closing `)`.
    UnclosedComment,
    /// A group or back-reference name is not an ASCII identifier
    /// (`[A-Za-z_][A-Za-z0-9_]*`).
    InvalidGroupName,
    /// Two named groups share a name.
    DuplicateGroupName,
    /// The pattern has both named groups and plain capturing groups.
    /// Oniguruma numbers plain groups only when told to (VS Code does; other
    /// engines do not), so capture numbers would differ between editors. Use
    /// `(?:...)` for grouping, or name every group.
    MixedGroups,
    /// A numbered back-reference in a pattern with named groups, which
    /// Oniguruma refuses unless plain groups capture. Use `\k<name>`.
    NumberedBackrefWithNamedGroups,
    /// `\k<name>` names no group in the pattern.
    UndefinedGroupName,
    /// A numbered back-reference names a group the pattern does not have.
    /// In an `end` or `while` pattern, numbered back-references name groups of
    /// the rule's `begin` pattern.
    BackreferenceOutOfRange {
        /// The group the back-reference names.
        group: u32,
        /// How many capture groups there are to refer to.
        groups: u32,
    },
    /// A `[` has no matching `]`.
    UnclosedClass,
    /// A character class, or one side of a `&&` intersection, is empty. `]`
    /// is never taken literally at the start of a class: write `\]`.
    EmptyClass,
    /// A `]` outside a class. Write `\]` for the character.
    UnescapedBracket,
    /// A range in a class runs backwards (`z-a`), or a `-` sits where it
    /// is neither the first or last item nor between two single characters.
    /// Write `\-` for the character.
    InvalidRange,
    /// `[:` inside a class does not start a POSIX bracket from the supported
    /// list (`[:alpha:]`, `[:^digit:]`, ...). Write `\[:` for the characters.
    UnknownPosixClass,
    /// A quantifier with nothing before it to repeat.
    NothingToRepeat,
    /// A quantifier directly after another (`a**`, `a{2}{3}`). Oniguruma reads
    /// these as nested repetition, other engines refuse them; group the inner
    /// repetition explicitly.
    RepeatedQuantifier,
    /// `{n}?`, `{n}+`, or `{n,m}+`. Oniguruma reads `x{n}?` as an optional
    /// `x{n}` and `x{n,m}+` as a repeated `x{n,m}`, where other engines read
    /// lazy and possessive repetition. Group explicitly: `(?:x{n})?`.
    AmbiguousQuantifier,
    /// A `{` that does not start a valid `{n}`, `{n,}`, or `{n,m}`
    /// quantifier, a count above 100000, or `{n,m}` with `n > m`. Write `\{`
    /// for the character; write `{0,m}` for `{,m}`.
    InvalidInterval,
    /// A quantifier applied to an anchor or a look-around, which Oniguruma
    /// refuses or ignores.
    QuantifiedAssertion,
    /// A look-behind alternative whose length is not fixed. Each top-level
    /// alternative of a look-behind must match a fixed number of characters,
    /// and alternatives of groups inside it must all have the same length.
    LookbehindNotFixed,
    /// A capturing group or a back-reference inside a look-behind.
    CaptureInLookbehind,
    /// Groups and classes nest more than [`MAX_REGEX_DEPTH`] deep.
    TooDeep,
    /// A raw NUL character; engines that read patterns as C strings stop
    /// there. Write `\x00`.
    NulCharacter,
    /// In an `end` or `while` pattern, an escaped backslash followed by a
    /// digit (`\\1`). VS Code substitutes `begin` captures into these
    /// patterns by searching the text for a backslash and digits, and would
    /// treat the second backslash as a back-reference. Write `\x5c1`.
    BackslashBeforeDigit,
    /// More capture groups than fit in a `u32`.
    TooManyGroups,
}

impl fmt::Display for RegexErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::TrailingBackslash => "the pattern ends with a lone `\\`",
            Self::UnknownEscape => "escape outside the supported subset",
            Self::InvalidHexEscape => {
                "`\\x` needs two hex digits or `{1-8 hex digits}`; `\\u` needs four hex digits"
            }
            Self::InvalidCodePoint => {
                "code point is a surrogate, above U+10FFFF, or a `\\xHH` byte above 7F"
            }
            Self::MalformedProperty => "`\\p` and `\\P` must be followed by `{name}`",
            Self::UnknownProperty => "unsupported Unicode property name",
            Self::UnclosedGroup => "unclosed group: `(` has no matching `)`",
            Self::UnmatchedParen => "unmatched `)`",
            Self::UnknownGroup => "group syntax outside the supported subset",
            Self::UnknownOption => "inline options must be `i`, `m`, or `x`",
            Self::UnclosedComment => "unclosed `(?#` comment",
            Self::InvalidGroupName => "group names must be ASCII identifiers",
            Self::DuplicateGroupName => "two named groups share a name",
            Self::MixedGroups => {
                "named and unnamed capturing groups are mixed; use `(?:...)` or name every group"
            }
            Self::NumberedBackrefWithNamedGroups => {
                "numbered back-reference in a pattern with named groups; use `\\k<name>`"
            }
            Self::UndefinedGroupName => "back-reference to an undefined group name",
            Self::BackreferenceOutOfRange { group, groups } => {
                return write!(
                    f,
                    "back-reference to group {group}, but the highest group it can name is {groups}"
                );
            }
            Self::UnclosedClass => "unclosed class: `[` has no matching `]`",
            Self::EmptyClass => "empty character class (write `\\]` for a literal `]`)",
            Self::UnescapedBracket => "unbalanced `]`; write `\\]` for the character",
            Self::InvalidRange => "invalid range in class; write `\\-` for a literal `-`",
            Self::UnknownPosixClass => "unsupported POSIX bracket expression",
            Self::NothingToRepeat => "quantifier with nothing to repeat",
            Self::RepeatedQuantifier => "quantifier follows another quantifier",
            Self::AmbiguousQuantifier => {
                "`{n}?`, `{n}+`, and `{n,m}+` mean different things in different engines"
            }
            Self::InvalidInterval => "invalid `{n,m}` quantifier; write `\\{` for a literal `{`",
            Self::QuantifiedAssertion => "quantifier applied to an anchor or look-around",
            Self::LookbehindNotFixed => "look-behind alternative does not have a fixed length",
            Self::CaptureInLookbehind => "capture group or back-reference inside a look-behind",
            Self::TooDeep => "groups and classes nest too deeply",
            Self::NulCharacter => "raw NUL character; write `\\x00`",
            Self::BackslashBeforeDigit => {
                "`\\\\` followed by a digit in an `end`/`while` pattern; write `\\x5c`"
            }
            Self::TooManyGroups => "too many capture groups",
        };
        f.write_str(text)
    }
}

impl fmt::Display for RegexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at byte {}: {}", self.offset, self.kind)
    }
}

impl core::error::Error for RegexError {}

/// Checks that `pattern` is in the accepted Oniguruma subset and returns its
/// number of capture groups.
///
/// This is the check every regular expression in a grammar passes before it
/// is emitted, available on its own for patterns built elsewhere. Numbered
/// back-references must name a group of the pattern itself. (In an `end` or
/// `while` pattern of a rule they name groups of the rule's `begin` pattern
/// instead; [`Grammar`](crate::Grammar) validation handles that case.)
///
/// | Parameter | Meaning |
/// |---|---|
/// | `pattern` | An Oniguruma regular expression, as it will appear in the grammar. |
///
/// # Errors
///
/// A [`RegexError`] with the byte offset and kind of the first construct
/// outside the subset.
///
/// # Examples
///
/// ```
/// use textmate_lang::validate_regex;
///
/// assert_eq!(validate_regex(r"\b(fn)\s+([a-z_]\w*)"), Ok(2));
/// assert_eq!(validate_regex(r"(?<=\.)\h+(?!\w)"), Ok(0));
/// assert_eq!(validate_regex(r#"(?x) "  # a quote, with a comment"#), Ok(0));
///
/// let err = validate_regex(r"(a)\2").unwrap_err();
/// assert_eq!(err.offset, 3);
/// assert_eq!(err.to_string(), "at byte 3: back-reference to group 2, but the highest group it can name is 1");
/// ```
pub fn validate_regex(pattern: &str) -> Result<u32, RegexError> {
    check(pattern, Backrefs::Own).map(|info| info.groups)
}

/// Escapes `text` so that it is a regular expression matching exactly
/// `text`, in every context a grammar uses: plain and extended (`(?x)`) mode,
/// and `end`/`while` patterns.
///
/// ASCII letters, digits, `_`, and non-ASCII characters are copied; every
/// other printable ASCII character is preceded by `\`; control characters
/// become `\xHH`. A backslash becomes `\x5c` rather than `\\`, so that no
/// concatenation of escaped text, with itself or with digits, can put an
/// escaped backslash before a digit (see
/// [`RegexErrorKind::BackslashBeforeDigit`]). The result always passes
/// [`validate_regex`] with zero groups.
///
/// | Parameter | Meaning |
/// |---|---|
/// | `text` | Literal text, such as a keyword or an operator. |
///
/// # Examples
///
/// ```
/// use textmate_lang::{escape_regex, validate_regex};
///
/// assert_eq!(escape_regex("a+b"), r"a\+b");
/// assert_eq!(escape_regex("<=>"), r"\<\=\>");
/// assert_eq!(escape_regex("tab\there"), r"tab\x09here");
/// let ops = ["+", "-", "*", "**", "?."];
/// let alternation: Vec<String> = ops.iter().map(|op| escape_regex(op)).collect();
/// assert_eq!(validate_regex(&alternation.join("|")), Ok(0));
/// ```
#[must_use]
pub fn escape_regex(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 2);
    for c in text.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => out.push(c),
            '\\' => out.push_str("\\x5c"),
            ' '..='~' => {
                out.push('\\');
                out.push(c);
            }
            '\0'..='\u{1f}' | '\u{7f}' => {
                // Both arms are ASCII, so the cast is exact.
                let byte = c as u8;
                out.push_str("\\x");
                out.push(hex_digit(byte >> 4));
                out.push(hex_digit(byte & 0x0F));
            }
            _ => out.push(c),
        }
    }
    out
}

fn hex_digit(nibble: u8) -> char {
    char::from(b"0123456789abcdef"[usize::from(nibble & 0x0F)])
}

/// What numbered back-references refer to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Backrefs {
    /// Groups of the pattern itself.
    Own,
    /// Groups of the rule's `begin` pattern: this is an `end` or `while`
    /// pattern, into which VS Code substitutes the captured text.
    Begin,
}

/// What a successful check learned about a pattern.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Info {
    /// Number of capture groups.
    pub(crate) groups: u32,
    /// In [`Backrefs::Begin`] mode, the largest numbered back-reference and
    /// its offset; the caller checks it against the `begin` pattern.
    pub(crate) max_backref: Option<(u32, usize)>,
}

/// Checks `pattern` against the subset.
pub(crate) fn check(pattern: &str, backrefs: Backrefs) -> Result<Info, RegexError> {
    check_in(pattern, backrefs, &mut Buffers::default())
}

/// Working memory for [`check_in`], reused from one pattern to the next so
/// that checking a grammar's patterns does not allocate per pattern.
#[derive(Default)]
pub(crate) struct Buffers {
    frames: Vec<Frame>,
}

/// [`check`] with reused working memory.
pub(crate) fn check_in(
    pattern: &str,
    backrefs: Backrefs,
    buffers: &mut Buffers,
) -> Result<Info, RegexError> {
    let mut parser = Parser::new(pattern, backrefs);
    parser.frames = core::mem::take(&mut buffers.frames);
    parser.frames.clear();
    let result = parser.run();
    buffers.frames = parser.frames;
    result
}

/// A fixed length in characters, or `None` for a variable one.
type Len = Option<u32>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Root,
    Capture,
    Named,
    NonCapture,
    Atomic,
    Lookahead,
    Lookbehind,
    /// The rest of the enclosing group after an isolated `(?imx)`, which
    /// Oniguruma parses as if it were `(?imx:...)` up to the enclosing `)`.
    Implicit,
}

/// What precedes the current position in the current alternative, which is
/// what a quantifier would apply to.
#[derive(Clone, Copy, Debug)]
enum Last {
    /// Nothing: the start of a group or an alternative.
    Nothing,
    /// An atom of this length, not yet quantified.
    Atom(Len),
    /// A quantified atom of this length.
    Quantified(Len),
    /// An anchor or look-around.
    Assertion,
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    kind: Kind,
    /// Offset of the `(` (0 for the root).
    open: usize,
    /// Whether extended mode is on inside this group.
    extended: bool,
    /// Length of the current alternative, the pending atom excluded.
    alt: Len,
    /// Length of the first finished alternative; `None` until there is one.
    first_alt: Option<Len>,
    /// Whether every finished alternative had the first one's length.
    same: bool,
    last: Last,
}

impl Frame {
    fn new(kind: Kind, open: usize, extended: bool) -> Frame {
        Frame {
            kind,
            open,
            extended,
            alt: Some(0),
            first_alt: None,
            same: true,
            last: Last::Nothing,
        }
    }

    /// Adds the pending atom to the alternative's length.
    fn flush(&mut self) {
        let pending = match self.last {
            Last::Atom(len) | Last::Quantified(len) => len,
            Last::Nothing | Last::Assertion => Some(0),
        };
        self.alt = match (self.alt, pending) {
            (Some(a), Some(b)) => a.checked_add(b),
            _ => None,
        };
        self.last = Last::Nothing;
    }

    /// Ends the current alternative; returns its length.
    fn end_alternative(&mut self) -> Len {
        self.flush();
        let len = self.alt;
        match self.first_alt {
            None => self.first_alt = Some(len),
            Some(first) => self.same &= first == len,
        }
        self.alt = Some(0);
        len
    }

    /// The group's length once all alternatives are finished.
    fn length(&self) -> Len {
        match self.first_alt {
            Some(len) if self.same => len,
            _ => None,
        }
    }
}

struct Parser<'p> {
    text: &'p str,
    bytes: &'p [u8],
    pos: usize,
    backrefs: Backrefs,
    frames: Vec<Frame>,
    lookbehinds: usize,
    groups: u32,
    first_unnamed: Option<usize>,
    first_named: Option<usize>,
    /// Named groups and where they start.
    names: Vec<(&'p str, usize)>,
    /// `\k<name>` references and where they start.
    named_refs: Vec<(&'p str, usize)>,
    /// The largest numbered back-reference and its offset.
    max_backref: Option<(u32, usize)>,
    first_backref: Option<usize>,
}

impl<'p> Parser<'p> {
    fn new(text: &'p str, backrefs: Backrefs) -> Parser<'p> {
        Parser {
            text,
            bytes: text.as_bytes(),
            pos: 0,
            backrefs,
            frames: Vec::new(),
            lookbehinds: 0,
            groups: 0,
            first_unnamed: None,
            first_named: None,
            names: Vec::new(),
            named_refs: Vec::new(),
            max_backref: None,
            first_backref: None,
        }
    }

    fn err<T>(offset: usize, kind: RegexErrorKind) -> Result<T, RegexError> {
        Err(RegexError { offset, kind })
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos.saturating_add(ahead)).copied()
    }

    fn top(&mut self) -> &mut Frame {
        // The root frame is pushed first and never popped, so the stack is
        // not empty; the push only guards the index below.
        if self.frames.is_empty() {
            self.frames.push(Frame::new(Kind::Root, 0, false));
        }
        let last = self.frames.len() - 1;
        &mut self.frames[last]
    }

    fn extended(&self) -> bool {
        self.frames.last().is_some_and(|f| f.extended)
    }

    fn depth_ok(&self, offset: usize, classes: usize) -> Result<(), RegexError> {
        // The root frame does not count as nesting.
        if self.frames.len().saturating_sub(1).saturating_add(classes) > MAX_REGEX_DEPTH {
            return Self::err(offset, RegexErrorKind::TooDeep);
        }
        Ok(())
    }

    fn run(&mut self) -> Result<Info, RegexError> {
        self.frames.push(Frame::new(Kind::Root, 0, false));
        loop {
            if self.extended() {
                self.skip_extended();
            }
            let Some(b) = self.peek() else { break };
            let at = self.pos;
            match b {
                b'(' => self.open_group()?,
                b')' => self.close_paren()?,
                b'|' => {
                    self.pos += 1;
                    self.alternative()?;
                }
                b'[' => {
                    self.class()?;
                    self.atom(Some(1));
                }
                b']' => return Self::err(at, RegexErrorKind::UnescapedBracket),
                b'{' => self.interval()?,
                b'*' | b'+' | b'?' => {
                    self.pos += 1;
                    self.modifier_after_simple();
                    self.quantify(at, None)?;
                }
                b'^' | b'$' => {
                    self.pos += 1;
                    self.assertion();
                }
                b'.' => {
                    self.pos += 1;
                    self.atom(Some(1));
                }
                b'\\' => self.escape()?,
                0 => return Self::err(at, RegexErrorKind::NulCharacter),
                _ => {
                    self.pos += char_len(b);
                    self.atom(Some(1));
                }
            }
        }
        // Isolated option groups end with the pattern.
        while self.frames.last().is_some_and(|f| f.kind == Kind::Implicit) {
            self.close_frame()?;
        }
        if let Some(frame) = self.frames.last() {
            if frame.kind != Kind::Root {
                return Self::err(frame.open, RegexErrorKind::UnclosedGroup);
            }
        }
        self.finish()
    }

    /// Whole-pattern checks that need every group seen.
    fn finish(&mut self) -> Result<Info, RegexError> {
        if let (Some(named), Some(unnamed)) = (self.first_named, self.first_unnamed) {
            return Self::err(named.max(unnamed), RegexErrorKind::MixedGroups);
        }
        if self.backrefs == Backrefs::Own {
            if let (Some(_), Some(at)) = (self.first_named, self.first_backref) {
                return Self::err(at, RegexErrorKind::NumberedBackrefWithNamedGroups);
            }
            if let Some((group, at)) = self.max_backref {
                if group > self.groups {
                    return Self::err(
                        at,
                        RegexErrorKind::BackreferenceOutOfRange {
                            group,
                            groups: self.groups,
                        },
                    );
                }
            }
        }
        self.names.sort_unstable();
        for pair in self.names.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Self::err(pair[0].1.max(pair[1].1), RegexErrorKind::DuplicateGroupName);
            }
        }
        for &(name, at) in &self.named_refs {
            if self
                .names
                .binary_search_by(|&(probe, _)| probe.cmp(name))
                .is_err()
            {
                return Self::err(at, RegexErrorKind::UndefinedGroupName);
            }
        }
        Ok(Info {
            groups: self.groups,
            max_backref: match self.backrefs {
                Backrefs::Own => None,
                Backrefs::Begin => self.max_backref,
            },
        })
    }

    /// Skips whitespace and `#` comments in extended mode.
    fn skip_extended(&mut self) {
        while let Some(b) = self.peek() {
            match b {
                b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C => self.pos += 1,
                b'#' => {
                    while let Some(c) = self.peek() {
                        self.pos += 1;
                        if c == b'\n' {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
    }

    fn atom(&mut self, len: Len) {
        let top = self.top();
        top.flush();
        top.last = Last::Atom(len);
    }

    fn assertion(&mut self) {
        let top = self.top();
        top.flush();
        top.last = Last::Assertion;
    }

    /// Consumes a lazy `?` or possessive `+` after `*`, `+`, or `?`.
    fn modifier_after_simple(&mut self) {
        if matches!(self.peek(), Some(b'?' | b'+')) {
            self.pos += 1;
        }
    }

    /// Applies a quantifier at `at` to the pending atom. `fixed` is the
    /// repetition count of a `{n}` or `{n,n}` quantifier.
    fn quantify(&mut self, at: usize, fixed: Option<u32>) -> Result<(), RegexError> {
        let top = self.top();
        let len = match top.last {
            Last::Nothing => return Self::err(at, RegexErrorKind::NothingToRepeat),
            Last::Assertion => return Self::err(at, RegexErrorKind::QuantifiedAssertion),
            Last::Quantified(_) => return Self::err(at, RegexErrorKind::RepeatedQuantifier),
            Last::Atom(len) => len,
        };
        let len = match (len, fixed) {
            (Some(len), Some(count)) => len.checked_mul(count),
            _ => None,
        };
        top.last = Last::Quantified(len);
        Ok(())
    }

    /// `{n}`, `{n,}`, or `{n,m}`.
    fn interval(&mut self) -> Result<(), RegexError> {
        let at = self.pos;
        self.pos += 1;
        let min = self.number().ok_or(RegexError {
            offset: at,
            kind: RegexErrorKind::InvalidInterval,
        })?;
        let max = if self.peek() == Some(b',') {
            self.pos += 1;
            if self.peek() == Some(b'}') {
                None
            } else {
                Some(self.number().ok_or(RegexError {
                    offset: at,
                    kind: RegexErrorKind::InvalidInterval,
                })?)
            }
        } else {
            Some(min)
        };
        let exact = self
            .bytes
            .get(at..self.pos)
            .is_some_and(|s| !s.contains(&b','));
        if self.peek() != Some(b'}')
            || min > MAX_REPEAT
            || max.is_some_and(|m| m > MAX_REPEAT || m < min)
        {
            return Self::err(at, RegexErrorKind::InvalidInterval);
        }
        self.pos += 1;
        match self.peek() {
            Some(b'+') => return Self::err(at, RegexErrorKind::AmbiguousQuantifier),
            Some(b'?') if exact => return Self::err(at, RegexErrorKind::AmbiguousQuantifier),
            Some(b'?') => self.pos += 1,
            _ => {}
        }
        let fixed = match max {
            Some(max) if max == min => Some(min),
            _ => None,
        };
        self.quantify(at, fixed)
    }

    /// A decimal number of at most six digits (anything longer is above
    /// [`MAX_REPEAT`] and refused by the caller), or `None` if no digit.
    fn number(&mut self) -> Option<u32> {
        let start = self.pos;
        let mut value: u32 = 0;
        while let Some(d @ b'0'..=b'9') = self.peek() {
            value = value.saturating_mul(10).saturating_add(u32::from(d - b'0'));
            self.pos += 1;
        }
        (self.pos > start).then_some(value)
    }

    /// `|`: each top-level alternative of a look-behind must be fixed.
    fn alternative(&mut self) -> Result<(), RegexError> {
        let top = self.top();
        let (open, kind) = (top.open, top.kind);
        if top.end_alternative().is_none() && kind == Kind::Lookbehind {
            return Self::err(open, RegexErrorKind::LookbehindNotFixed);
        }
        Ok(())
    }

    fn open_group(&mut self) -> Result<(), RegexError> {
        let at = self.pos;
        self.depth_ok(at, 1)?;
        let extended = self.extended();
        self.pos += 1;
        if self.peek() != Some(b'?') {
            return self.push_capture(at, Kind::Capture, extended);
        }
        self.pos += 1;
        let kind = match self.peek() {
            Some(b':') => Kind::NonCapture,
            Some(b'=' | b'!') => Kind::Lookahead,
            Some(b'>') => Kind::Atomic,
            Some(b'<') => match self.peek_at(1) {
                Some(b'=' | b'!') => {
                    self.pos += 1;
                    Kind::Lookbehind
                }
                _ => {
                    self.pos += 1;
                    let name = self.group_name(at)?;
                    if self.peek() != Some(b'>') {
                        return Self::err(at, RegexErrorKind::InvalidGroupName);
                    }
                    self.pos += 1;
                    self.names.push((name, at));
                    return self.push_capture(at, Kind::Named, extended);
                }
            },
            Some(b'#') => return self.comment(at),
            Some(b'i' | b'm' | b'x' | b'-') => return self.options(at, extended),
            _ => return Self::err(at, RegexErrorKind::UnknownGroup),
        };
        self.pos += 1;
        if kind == Kind::Lookbehind {
            self.lookbehinds += 1;
        }
        self.push_frame(at, kind, extended);
        Ok(())
    }

    fn push_frame(&mut self, at: usize, kind: Kind, extended: bool) {
        self.top().flush();
        self.frames.push(Frame::new(kind, at, extended));
    }

    fn push_capture(&mut self, at: usize, kind: Kind, extended: bool) -> Result<(), RegexError> {
        if self.lookbehinds > 0 {
            return Self::err(at, RegexErrorKind::CaptureInLookbehind);
        }
        self.groups = self.groups.checked_add(1).ok_or(RegexError {
            offset: at,
            kind: RegexErrorKind::TooManyGroups,
        })?;
        let first = if kind == Kind::Named {
            &mut self.first_named
        } else {
            &mut self.first_unnamed
        };
        if first.is_none() {
            *first = Some(at);
        }
        self.push_frame(at, kind, extended);
        Ok(())
    }

    /// An ASCII identifier, ending at the current position's terminator.
    fn group_name(&mut self, at: usize) -> Result<&'p str, RegexError> {
        let start = self.pos;
        while let Some(b) = self.peek() {
            let ok =
                b == b'_' || b.is_ascii_alphabetic() || (self.pos > start && b.is_ascii_digit());
            if !ok {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start {
            return Self::err(at, RegexErrorKind::InvalidGroupName);
        }
        Ok(self.text.get(start..self.pos).unwrap_or(""))
    }

    /// `(?#...)`: skipped entirely, like Oniguruma's tokenizer does. A
    /// backslash escapes the next character.
    fn comment(&mut self, at: usize) -> Result<(), RegexError> {
        self.pos += 1;
        while let Some(b) = self.peek() {
            match b {
                b'\\' => self.pos += 1 + self.peek_at(1).map_or(0, char_len),
                b')' => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => self.pos += char_len(b),
            }
        }
        Self::err(at, RegexErrorKind::UnclosedComment)
    }

    /// `(?imx-imx)` or `(?imx-imx:...)`.
    fn options(&mut self, at: usize, extended: bool) -> Result<(), RegexError> {
        let mut on = true;
        let mut letters = 0;
        let mut negated_letters = 0;
        let mut mode = extended;
        loop {
            match self.peek() {
                Some(b'-') if on => on = false,
                Some(c @ (b'i' | b'm' | b'x')) => {
                    if c == b'x' {
                        mode = on;
                    }
                    letters += 1;
                    if !on {
                        negated_letters += 1;
                    }
                }
                Some(b')') | Some(b':') => break,
                _ => return Self::err(at, RegexErrorKind::UnknownOption),
            }
            self.pos += 1;
        }
        if letters == 0 || (!on && negated_letters == 0) {
            return Self::err(at, RegexErrorKind::UnknownOption);
        }
        let colon = self.peek() == Some(b':');
        self.pos += 1;
        if colon {
            self.push_frame(at, Kind::NonCapture, mode);
        } else {
            // The options apply to the rest of the enclosing group, which
            // Oniguruma parses as a group of its own.
            self.push_frame(at, Kind::Implicit, mode);
        }
        Ok(())
    }

    fn close_paren(&mut self) -> Result<(), RegexError> {
        let at = self.pos;
        while self.frames.last().is_some_and(|f| f.kind == Kind::Implicit) {
            self.close_frame()?;
        }
        if self.frames.last().is_none_or(|f| f.kind == Kind::Root) {
            return Self::err(at, RegexErrorKind::UnmatchedParen);
        }
        self.pos += 1;
        self.close_frame()
    }

    /// Pops the innermost group and records it in its parent.
    fn close_frame(&mut self) -> Result<(), RegexError> {
        let Some(mut frame) = self.frames.pop() else {
            return Ok(());
        };
        let last_len = frame.end_alternative();
        if frame.kind == Kind::Lookbehind {
            if last_len.is_none() {
                return Self::err(frame.open, RegexErrorKind::LookbehindNotFixed);
            }
            self.lookbehinds = self.lookbehinds.saturating_sub(1);
        }
        let len = frame.length();
        let parent = self.top();
        // A group whose alternatives differ in length is variable, which
        // makes an enclosing look-behind alternative variable too.
        parent.last = match frame.kind {
            Kind::Lookahead | Kind::Lookbehind => Last::Assertion,
            _ => Last::Atom(len),
        };
        Ok(())
    }

    fn escape(&mut self) -> Result<(), RegexError> {
        let at = self.pos;
        let Some(c) = self.peek_at(1) else {
            return Self::err(at, RegexErrorKind::TrailingBackslash);
        };
        match c {
            b'A' | b'z' | b'Z' | b'G' | b'b' | b'B' => {
                self.pos += 2;
                self.assertion();
            }
            b'1'..=b'9' => self.backref(at)?,
            b'k' => self.named_backref(at)?,
            b'w' | b'W' | b'd' | b'D' | b's' | b'S' | b'h' | b'H' => {
                self.pos += 2;
                self.atom(Some(1));
            }
            b'p' | b'P' => {
                self.property(at)?;
                self.atom(Some(1));
            }
            _ => {
                let _char = self.char_escape(at)?;
                self.atom(Some(1));
            }
        }
        Ok(())
    }

    fn backref(&mut self, at: usize) -> Result<(), RegexError> {
        self.pos += 1;
        let group = self.number().unwrap_or(0);
        // `number` saturates at u32::MAX, which no pattern has as many groups.
        if self.lookbehinds > 0 {
            return Self::err(at, RegexErrorKind::CaptureInLookbehind);
        }
        if self.first_backref.is_none() {
            self.first_backref = Some(at);
        }
        if self.max_backref.is_none_or(|(max, _)| group > max) {
            self.max_backref = Some((group, at));
        }
        self.atom(None);
        Ok(())
    }

    fn named_backref(&mut self, at: usize) -> Result<(), RegexError> {
        self.pos += 2;
        if self.peek() != Some(b'<') {
            return Self::err(at, RegexErrorKind::InvalidGroupName);
        }
        self.pos += 1;
        let name = self.group_name(at)?;
        if self.peek() != Some(b'>') {
            return Self::err(at, RegexErrorKind::InvalidGroupName);
        }
        self.pos += 1;
        if self.lookbehinds > 0 {
            return Self::err(at, RegexErrorKind::CaptureInLookbehind);
        }
        self.named_refs.push((name, at));
        self.atom(None);
        Ok(())
    }

    /// `\p{Name}`, `\p{^Name}`, `\P{Name}`.
    fn property(&mut self, at: usize) -> Result<(), RegexError> {
        self.pos += 2;
        if self.peek() != Some(b'{') {
            return Self::err(at, RegexErrorKind::MalformedProperty);
        }
        self.pos += 1;
        if self.peek() == Some(b'^') {
            self.pos += 1;
        }
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b == b'}' {
                break;
            }
            if !(b.is_ascii_alphanumeric() || matches!(b, b'_' | b' ' | b'-')) {
                return Self::err(at, RegexErrorKind::MalformedProperty);
            }
            self.pos += 1;
        }
        if self.peek() != Some(b'}') {
            return Self::err(at, RegexErrorKind::MalformedProperty);
        }
        let name = self.bytes.get(start..self.pos).unwrap_or(&[]);
        self.pos += 1;
        if known_property(name) {
            Ok(())
        } else {
            Self::err(at, RegexErrorKind::UnknownProperty)
        }
    }

    /// An escape that stands for one character: control escapes, hex
    /// escapes, and escaped ASCII punctuation. Returns the character.
    fn char_escape(&mut self, at: usize) -> Result<char, RegexError> {
        let Some(c) = self.peek_at(1) else {
            return Self::err(at, RegexErrorKind::TrailingBackslash);
        };
        let simple = match c {
            b't' => Some('\t'),
            b'n' => Some('\n'),
            b'r' => Some('\r'),
            b'f' => Some('\u{c}'),
            b'v' => Some('\u{b}'),
            b'a' => Some('\u{7}'),
            b'e' => Some('\u{1b}'),
            _ => None,
        };
        if let Some(ch) = simple {
            self.pos += 2;
            return Ok(ch);
        }
        match c {
            b'x' => self.hex_escape(at),
            b'u' => self.unicode_escape(at),
            b'0'..=b'9' => Self::err(at, RegexErrorKind::UnknownEscape),
            b'\\'
                if self.backrefs == Backrefs::Begin
                    && self.peek_at(2).is_some_and(|d| d.is_ascii_digit()) =>
            {
                Self::err(at, RegexErrorKind::BackslashBeforeDigit)
            }
            0x21..=0x2F | 0x3A..=0x40 | 0x5B..=0x60 | 0x7B..=0x7E | b' ' => {
                self.pos += 2;
                Ok(char::from(c))
            }
            _ => Self::err(at, RegexErrorKind::UnknownEscape),
        }
    }

    /// `\xHH` (00-7F) or `\x{H...}`.
    fn hex_escape(&mut self, at: usize) -> Result<char, RegexError> {
        self.pos += 2;
        if self.peek() == Some(b'{') {
            self.pos += 1;
            let start = self.pos;
            let mut value: u32 = 0;
            while let Some(d) = self.peek().and_then(hex_value) {
                value = value.saturating_mul(16).saturating_add(d);
                self.pos += 1;
            }
            let digits = self.pos - start;
            if digits == 0 || digits > 8 || self.peek() != Some(b'}') {
                return Self::err(at, RegexErrorKind::InvalidHexEscape);
            }
            self.pos += 1;
            return char::from_u32(value).ok_or(RegexError {
                offset: at,
                kind: RegexErrorKind::InvalidCodePoint,
            });
        }
        let (Some(hi), Some(lo)) = (
            self.peek().and_then(hex_value),
            self.peek_at(1).and_then(hex_value),
        ) else {
            return Self::err(at, RegexErrorKind::InvalidHexEscape);
        };
        let value = hi * 16 + lo;
        if value > 0x7F {
            return Self::err(at, RegexErrorKind::InvalidCodePoint);
        }
        self.pos += 2;
        char::from_u32(value).ok_or(RegexError {
            offset: at,
            kind: RegexErrorKind::InvalidCodePoint,
        })
    }

    /// `\uHHHH`.
    fn unicode_escape(&mut self, at: usize) -> Result<char, RegexError> {
        self.pos += 2;
        let mut value: u32 = 0;
        for _ in 0..4 {
            let Some(d) = self.peek().and_then(hex_value) else {
                return Self::err(at, RegexErrorKind::InvalidHexEscape);
            };
            value = value * 16 + d;
            self.pos += 1;
        }
        char::from_u32(value).ok_or(RegexError {
            offset: at,
            kind: RegexErrorKind::InvalidCodePoint,
        })
    }

    /// A character class, from its `[` to its matching `]`.
    fn class(&mut self) -> Result<(), RegexError> {
        let outer = self.pos;
        // Per nested class: items since its start or its last `&&`, and the
        // last single character, which may start a range.
        let mut stack: Vec<ClassFrame> = Vec::new();
        self.open_class(&mut stack)?;
        loop {
            let at = self.pos;
            let Some(b) = self.peek() else {
                return Self::err(outer, RegexErrorKind::UnclosedClass);
            };
            let Some(top) = stack.last_mut() else {
                return Ok(());
            };
            match b {
                b']' => {
                    if top.items == 0 {
                        return Self::err(at, RegexErrorKind::EmptyClass);
                    }
                    self.pos += 1;
                    let _closed = stack.pop();
                    match stack.last_mut() {
                        None => return Ok(()),
                        Some(parent) => parent.set_item(),
                    }
                }
                b'[' if self.peek_at(1) == Some(b':') => {
                    self.posix(at)?;
                    top.set_item();
                }
                b'[' => self.open_class(&mut stack)?,
                b'&' if self.peek_at(1) == Some(b'&') => {
                    if top.items == 0 {
                        return Self::err(at, RegexErrorKind::EmptyClass);
                    }
                    top.items = 0;
                    top.endpoint = None;
                    self.pos += 2;
                }
                b'-' => {
                    if top.items == 0 || self.peek_at(1) == Some(b']') {
                        self.pos += 1;
                        top.set_item();
                    } else if let Some((lo, lo_at)) = top.endpoint {
                        self.pos += 1;
                        let hi = self.range_end(at)?;
                        if hi < lo {
                            return Self::err(lo_at, RegexErrorKind::InvalidRange);
                        }
                        top.set_item();
                    } else {
                        return Self::err(at, RegexErrorKind::InvalidRange);
                    }
                }
                b'\\' => {
                    let single = self.class_escape(at)?;
                    // The borrow of `top` ended at the call; look it up again.
                    if let Some(top) = stack.last_mut() {
                        top.set_item();
                        top.endpoint = single.map(|c| (c, at));
                    }
                }
                0 => return Self::err(at, RegexErrorKind::NulCharacter),
                _ => {
                    let c = self.next_char();
                    top.set_item();
                    top.endpoint = Some((c, at));
                }
            }
        }
    }

    fn open_class(&mut self, stack: &mut Vec<ClassFrame>) -> Result<(), RegexError> {
        let at = self.pos;
        self.depth_ok(at, stack.len() + 1)?;
        self.pos += 1;
        if self.peek() == Some(b'^') {
            self.pos += 1;
        }
        stack.push(ClassFrame {
            items: 0,
            endpoint: None,
        });
        Ok(())
    }

    /// The character after `-` in a range: a literal or a one-character
    /// escape.
    fn range_end(&mut self, dash: usize) -> Result<char, RegexError> {
        let at = self.pos;
        match self.peek() {
            None => Self::err(dash, RegexErrorKind::InvalidRange),
            Some(b'[' | b']' | b'-') => Self::err(dash, RegexErrorKind::InvalidRange),
            Some(b'&') if self.peek_at(1) == Some(b'&') => {
                Self::err(dash, RegexErrorKind::InvalidRange)
            }
            Some(0) => Self::err(at, RegexErrorKind::NulCharacter),
            Some(b'\\') => match self.class_escape(at)? {
                Some(c) => Ok(c),
                None => Self::err(dash, RegexErrorKind::InvalidRange),
            },
            Some(_) => Ok(self.next_char()),
        }
    }

    /// An escape inside a class: `Some(c)` for one character, `None` for a
    /// set (`\w`, `\p{...}`).
    fn class_escape(&mut self, at: usize) -> Result<Option<char>, RegexError> {
        match self.peek_at(1) {
            None => Self::err(at, RegexErrorKind::TrailingBackslash),
            Some(b'w' | b'W' | b'd' | b'D' | b's' | b'S' | b'h' | b'H') => {
                self.pos += 2;
                Ok(None)
            }
            Some(b'p' | b'P') => {
                self.property(at)?;
                Ok(None)
            }
            Some(b'b' | b'B' | b'A' | b'z' | b'Z' | b'G' | b'k') => {
                Self::err(at, RegexErrorKind::UnknownEscape)
            }
            Some(_) => self.char_escape(at).map(Some),
        }
    }

    /// `[:name:]` or `[:^name:]` inside a class.
    fn posix(&mut self, at: usize) -> Result<(), RegexError> {
        let rest = self.bytes.get(at + 2..).unwrap_or(&[]);
        let rest = rest.strip_prefix(b"^").unwrap_or(rest);
        let end = rest
            .iter()
            .position(|&b| !b.is_ascii_lowercase())
            .unwrap_or(rest.len());
        let (name, tail) = rest.split_at(end);
        if !tail.starts_with(b":]") || !POSIX.contains(&name) {
            return Self::err(at, RegexErrorKind::UnknownPosixClass);
        }
        // `[:` + optional `^` + name + `:]`.
        self.pos = at + 2 + usize::from(self.bytes.get(at + 2) == Some(&b'^')) + name.len() + 2;
        Ok(())
    }

    fn next_char(&mut self) -> char {
        let c = self
            .text
            .get(self.pos..)
            .and_then(|s| s.chars().next())
            .unwrap_or('\0');
        self.pos += c.len_utf8();
        c
    }
}

#[derive(Clone, Copy)]
struct ClassFrame {
    items: u32,
    endpoint: Option<(char, usize)>,
}

impl ClassFrame {
    fn set_item(&mut self) {
        self.items = self.items.saturating_add(1);
        self.endpoint = None;
    }
}

/// The length in bytes of the UTF-8 character starting with `first`.
fn char_len(first: u8) -> usize {
    match first {
        0xF0..=0xFF => 4,
        0xE0..=0xEF => 3,
        0xC0..=0xDF => 2,
        _ => 1,
    }
}

fn hex_value(b: u8) -> Option<u32> {
    char::from(b).to_digit(16)
}

/// POSIX bracket names Oniguruma accepts in `[[:name:]]`.
const POSIX: &[&[u8]] = &[
    b"alnum", b"alpha", b"ascii", b"blank", b"cntrl", b"digit", b"graph", b"lower", b"print",
    b"punct", b"space", b"upper", b"xdigit", b"word",
];

/// Property names accepted in `\p{...}`, normalized: lowercase, with spaces,
/// underscores, and hyphens removed, the way Oniguruma compares them.
const PROPERTIES: &[&str] = &[
    // General categories.
    "c",
    "cc",
    "cf",
    "cn",
    "co",
    "cs",
    "l",
    "ll",
    "lm",
    "lo",
    "lt",
    "lu",
    "m",
    "mc",
    "me",
    "mn",
    "n",
    "nd",
    "nl",
    "no",
    "p",
    "pc",
    "pd",
    "pe",
    "pf",
    "pi",
    "po",
    "ps",
    "s",
    "sc",
    "sk",
    "sm",
    "so",
    "z",
    "zl",
    "zp",
    "zs",
    // POSIX-like classes.
    "alnum",
    "alpha",
    "ascii",
    "blank",
    "cntrl",
    "digit",
    "graph",
    "lower",
    "print",
    "punct",
    "space",
    "upper",
    "xdigit",
    "word",
    "any",
    "assigned",
    // Binary properties.
    "alphabetic",
    "lowercase",
    "uppercase",
    "whitespace",
    "idstart",
    "idcontinue",
    "xidstart",
    "xidcontinue",
    // Scripts.
    "arabic",
    "armenian",
    "bengali",
    "bopomofo",
    "cherokee",
    "common",
    "cyrillic",
    "devanagari",
    "ethiopic",
    "georgian",
    "greek",
    "gujarati",
    "gurmukhi",
    "han",
    "hangul",
    "hebrew",
    "hiragana",
    "inherited",
    "kannada",
    "katakana",
    "khmer",
    "lao",
    "latin",
    "malayalam",
    "mongolian",
    "myanmar",
    "oriya",
    "sinhala",
    "syriac",
    "tamil",
    "telugu",
    "thaana",
    "thai",
    "tibetan",
];

/// Whether `name` (as written in `\p{...}`) is a supported property.
fn known_property(name: &[u8]) -> bool {
    let mut normalized = [0u8; 32];
    let mut len = 0;
    for &b in name {
        if matches!(b, b' ' | b'_' | b'-') {
            continue;
        }
        let Some(slot) = normalized.get_mut(len) else {
            return false;
        };
        *slot = b.to_ascii_lowercase();
        len += 1;
    }
    let normalized = normalized.get(..len).unwrap_or(&[]);
    PROPERTIES.iter().any(|p| p.as_bytes() == normalized)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    fn kind(pattern: &str) -> RegexErrorKind {
        match validate_regex(pattern) {
            Err(e) => e.kind,
            Ok(groups) => panic!("{pattern:?} was accepted with {groups} groups"),
        }
    }

    #[test]
    fn test_accepts_common_grammar_patterns() {
        let ok = [
            (r"\b(if|else|while)\b", 1),
            (r#""(?:[^"\\]|\\.)*""#, 0),
            (r"(?<!\w)(\d+)(?:\.(\d+))?", 2),
            (r"^\s*(#)\s*(include)\b", 2),
            (r"\G(?=\S)", 0),
            (r"(?i:select|from)", 0),
            (r"(?<name>[a-z]+)\s*=\s*\k<name>", 1),
            (r"[\p{L}_][\p{L}\p{N}_]*", 0),
            (r"[[:alpha:][:digit:]]", 0),
            (r"[a-z&&[^aeiou]]", 0),
            (r"\x{1F600}|\u00e9|\x7f", 0),
            (r"a{2}b{1,}c{0,3}?", 0),
            (r"a*+b++c?+", 0),
            (r"(?>a+)b", 0),
            (r"(?<=ab|c)d", 0),
            (r"(?<=a(?:b|c))d", 0),
            (r"(?<=\d{3})x", 0),
            ("}", 0),
            (r"[-a]|[a-]|[\]\[]", 0),
            (r"(?#comment \) here)x", 0),
            ("(?x) a + # spaces and a comment ( ) [\n b", 0),
            (r"(?x) [ ]", 0),
        ];
        for (pattern, groups) in ok {
            assert_eq!(validate_regex(pattern), Ok(groups), "{pattern}");
        }
    }

    #[test]
    fn test_rejects_with_offsets() {
        let cases = [
            ("ab\\", 2, RegexErrorKind::TrailingBackslash),
            (r"a\q", 1, RegexErrorKind::UnknownEscape),
            (r"\0", 0, RegexErrorKind::UnknownEscape),
            (r"x\xZZ", 1, RegexErrorKind::InvalidHexEscape),
            (r"\xff", 0, RegexErrorKind::InvalidCodePoint),
            (r"\x{110000}", 0, RegexErrorKind::InvalidCodePoint),
            (r"\uD800", 0, RegexErrorKind::InvalidCodePoint),
            (r"\pL", 0, RegexErrorKind::MalformedProperty),
            (r"\p{Klingon}", 0, RegexErrorKind::UnknownProperty),
            ("a(b", 1, RegexErrorKind::UnclosedGroup),
            ("a)b", 1, RegexErrorKind::UnmatchedParen),
            ("(?(1)a|b)", 0, RegexErrorKind::UnknownGroup),
            ("(?iq)", 0, RegexErrorKind::UnknownOption),
            ("(?q)", 0, RegexErrorKind::UnknownGroup),
            ("(?-)", 0, RegexErrorKind::UnknownOption),
            ("a(?#x", 1, RegexErrorKind::UnclosedComment),
            ("(?<1a>x)", 0, RegexErrorKind::InvalidGroupName),
            ("(?<a>x)(?<a>y)", 7, RegexErrorKind::DuplicateGroupName),
            ("(?<a>x)(y)", 7, RegexErrorKind::MixedGroups),
            (
                r"(?<a>x)\1",
                7,
                RegexErrorKind::NumberedBackrefWithNamedGroups,
            ),
            (r"(?<a>x)\k<b>", 7, RegexErrorKind::UndefinedGroupName),
            ("[abc", 0, RegexErrorKind::UnclosedClass),
            ("[]a]", 1, RegexErrorKind::EmptyClass),
            ("[^]", 2, RegexErrorKind::EmptyClass),
            ("[a&&]", 4, RegexErrorKind::EmptyClass),
            ("a]", 1, RegexErrorKind::UnescapedBracket),
            ("[z-a]", 1, RegexErrorKind::InvalidRange),
            (r"[a-\w]", 2, RegexErrorKind::InvalidRange),
            (r"[\w-a]", 3, RegexErrorKind::InvalidRange),
            ("[[:alpah:]]", 1, RegexErrorKind::UnknownPosixClass),
            ("*a", 0, RegexErrorKind::NothingToRepeat),
            ("(|*)", 2, RegexErrorKind::NothingToRepeat),
            ("a**", 2, RegexErrorKind::RepeatedQuantifier),
            ("a{2}{3}", 4, RegexErrorKind::RepeatedQuantifier),
            ("a{2}?", 1, RegexErrorKind::AmbiguousQuantifier),
            ("a{1,2}+", 1, RegexErrorKind::AmbiguousQuantifier),
            ("a{", 1, RegexErrorKind::InvalidInterval),
            ("a{,2}", 1, RegexErrorKind::InvalidInterval),
            ("a{3,2}", 1, RegexErrorKind::InvalidInterval),
            ("a{100001}", 1, RegexErrorKind::InvalidInterval),
            ("^*", 1, RegexErrorKind::QuantifiedAssertion),
            ("(?=a)+", 5, RegexErrorKind::QuantifiedAssertion),
            ("(?<=a+)b", 0, RegexErrorKind::LookbehindNotFixed),
            ("(?<=a|b*)c", 0, RegexErrorKind::LookbehindNotFixed),
            ("(?<=(?:ab|c))d", 0, RegexErrorKind::LookbehindNotFixed),
            ("(?<=(a))b", 4, RegexErrorKind::CaptureInLookbehind),
            ("a\0", 1, RegexErrorKind::NulCharacter),
        ];
        for (pattern, offset, expected) in cases {
            let err = validate_regex(pattern).unwrap_err();
            assert_eq!((err.offset, err.kind), (offset, expected), "{pattern:?}");
        }
    }

    #[test]
    fn test_backreferences() {
        assert_eq!(validate_regex(r"(a)(b)\2\1"), Ok(2));
        assert_eq!(
            kind(r"(a)\2"),
            RegexErrorKind::BackreferenceOutOfRange {
                group: 2,
                groups: 1
            }
        );
        // In `end` mode the numbers name `begin` groups; the caller checks.
        let info = check(r"\2(x)\10", Backrefs::Begin).unwrap();
        assert_eq!(info.groups, 1);
        assert_eq!(info.max_backref, Some((10, 5)));
        assert_eq!(
            check(r"\\1", Backrefs::Begin).unwrap_err().kind,
            RegexErrorKind::BackslashBeforeDigit
        );
        assert!(check(r"\\1", Backrefs::Own).is_ok());
        assert_eq!(
            check(r"[\\1]", Backrefs::Begin).unwrap_err().kind,
            RegexErrorKind::BackslashBeforeDigit
        );
    }

    #[test]
    fn test_isolated_option_extends_to_enclosing_group() {
        // `(?x)` turns on extended mode up to the enclosing `)` only.
        assert_eq!(validate_regex("((?x) a # c\n) b"), Ok(1));
        assert_eq!(kind("((?x) a ) #)"), RegexErrorKind::UnmatchedParen);
        // `a(?i)b|cd` is `a(?i:b|cd)` in Oniguruma, so in a look-behind the
        // alternatives belong to a nested group and must be the same length.
        assert_eq!(kind("(?<=a(?i)b|cd)"), RegexErrorKind::LookbehindNotFixed);
        assert_eq!(validate_regex("(?<=a(?i)b|c)"), Ok(0));
        assert_eq!(kind("(?i)*"), RegexErrorKind::NothingToRepeat);
    }

    #[test]
    fn test_extended_mode_comments_hide_metacharacters() {
        assert_eq!(validate_regex("(?x) a # ( [ unbalanced"), Ok(0));
        assert_eq!(validate_regex("(?x:a # )\n)"), Ok(0));
        assert_eq!(validate_regex(r"(?x) \# \ "), Ok(0));
        // Not in extended mode, `#` is an ordinary character.
        assert_eq!(kind("a # ("), RegexErrorKind::UnclosedGroup);
    }

    #[test]
    fn test_depth_limit() {
        let ok = "(".repeat(MAX_REGEX_DEPTH) + &")".repeat(MAX_REGEX_DEPTH);
        assert_eq!(
            validate_regex(&ok),
            Ok(u32::try_from(MAX_REGEX_DEPTH).unwrap())
        );
        let deep = "(?:".repeat(MAX_REGEX_DEPTH + 1) + &")".repeat(MAX_REGEX_DEPTH + 1);
        assert_eq!(kind(&deep), RegexErrorKind::TooDeep);
        let classes = "[".repeat(MAX_REGEX_DEPTH + 1) + "a" + &"]".repeat(MAX_REGEX_DEPTH + 1);
        assert_eq!(kind(&classes), RegexErrorKind::TooDeep);
    }

    #[test]
    fn test_escape_regex() {
        assert_eq!(escape_regex(""), "");
        assert_eq!(escape_regex("abc_09"), "abc_09");
        assert_eq!(escape_regex("a.b*c"), r"a\.b\*c");
        assert_eq!(escape_regex("\\n"), r"\x5cn");
        assert_eq!(escape_regex("\\1"), r"\x5c1");
        // Escaped pieces compose: no `\\` + digit at a seam.
        let joined = escape_regex("a\\") + &escape_regex("1");
        assert!(check(&joined, Backrefs::Begin).is_ok());
        assert_eq!(escape_regex("\0\u{7f}"), r"\x00\x7f");
        assert_eq!(escape_regex("é →"), "é\\ →");
        for text in ["((", "[", "\\", "{1}", "#x", "a b", "\\\\9"] {
            assert_eq!(
                check(&escape_regex(text), Backrefs::Begin).map(|i| i.groups),
                Ok(0)
            );
        }
    }

    #[test]
    fn test_properties_normalize_like_oniguruma() {
        assert!(known_property(b"L"));
        assert!(known_property(b"White_Space"));
        assert!(known_property(b"xid-start"));
        assert!(!known_property(b""));
        assert!(!known_property(&[b'a'; 40]));
    }

    #[test]
    fn test_display() {
        let e = RegexError {
            offset: 4,
            kind: RegexErrorKind::UnclosedClass,
        };
        assert_eq!(
            e.to_string(),
            "at byte 4: unclosed class: `[` has no matching `]`"
        );
    }
}
