//! The rules of a TextMate grammar: matches, begin/end and begin/while
//! regions, includes, and pattern lists.

use alloc::{borrow::Cow, vec::Vec};

/// Text held by the model: borrowed from a `&'static str`, or owned.
pub(crate) type Text = Cow<'static, str>;

/// Captures: group numbers and what to do with the captured text.
pub(crate) type Captures = Vec<(u32, Capture)>;

/// One entry of a pattern list: what the tokenizer tries at each position.
///
/// A rule is one of:
///
/// - a [`Match`]: one regular expression, scoped as a whole and by capture
///   group (`"match"` in the JSON);
/// - a [`BeginEnd`]: a region from a `begin` match to an `end` match, with
///   its own patterns inside (`"begin"`/`"end"`);
/// - a [`BeginWhile`]: a region that starts at a `begin` match and continues
///   on each following line while a `while` pattern matches at its start
///   (`"begin"`/`"while"`), as Markdown block quotes do;
/// - an include of a repository entry, the grammar itself, the base grammar,
///   or another grammar (`"include"`);
/// - a list of rules tried together ([`Rule::patterns`]).
///
/// [`Match`], [`BeginEnd`], and [`BeginWhile`] convert into a `Rule` with
/// `into()`, and every method that takes a rule accepts them directly.
///
/// # Examples
///
/// ```
/// use textmate_lang::{BeginEnd, Grammar, Match, Rule};
///
/// let json = Grammar::new("source.demo")
///     .pattern(Rule::include("comments"))
///     .pattern(Match::new(r"\b(let|fn)\b").scope("keyword.other.demo"))
///     .repository(
///         "comments",
///         Rule::patterns([
///             Match::new("//.*$").scope("comment.line.double-slash.demo").into(),
///             BeginEnd::new(r"/\*", r"\*/").scope("comment.block.demo").into(),
///         ]),
///     )
///     .to_json()?;
/// assert!(json.contains(r##""include": "#comments""##));
/// # Ok::<(), textmate_lang::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule(pub(crate) Kind);

/// What a [`Rule`] is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Match(Match),
    BeginEnd(BeginEnd),
    BeginWhile(BeginWhile),
    Include(Include),
    Patterns(Vec<Rule>),
}

/// The target of an include.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Include {
    /// `#name`: an entry of this grammar's repository.
    Repository(Text),
    /// `$self`: this grammar's top-level patterns.
    SelfRef,
    /// `$base`: the top-level patterns of the grammar being highlighted,
    /// which is this one unless it is embedded in another.
    Base,
    /// `scope`: another grammar's top-level patterns.
    Scope(Text),
    /// `scope#name`: an entry of another grammar's repository.
    ScopeEntry(Text, Text),
}

impl Rule {
    /// Includes the repository entry called `name` (`"include": "#name"`).
    ///
    /// The entry must exist ([`Error::UndefinedInclude`](crate::Error::UndefinedInclude)),
    /// checked when the grammar is emitted. The entry's patterns are tried as
    /// if they were written in place of the include.
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `name` | A repository entry's name, without the `#`. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Grammar, Match, Rule};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(Rule::include("numbers"))
    ///     .repository("numbers", Match::new(r"\d+").scope("constant.numeric.demo"))
    ///     .to_json()?;
    /// assert!(json.contains(r##""include": "#numbers""##));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn include(name: impl Into<Text>) -> Rule {
        Rule(Kind::Include(Include::Repository(name.into())))
    }

    /// Includes this grammar's top-level patterns (`"include": "$self"`),
    /// typically inside a region whose contents are the language again, such
    /// as the body of a block.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginEnd, Grammar, Rule};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(BeginEnd::new(r"\{", r"\}").scope("meta.block.demo").pattern(Rule::include_self()))
    ///     .to_json()?;
    /// assert!(json.contains(r#""include": "$self""#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn include_self() -> Rule {
        Rule(Kind::Include(Include::SelfRef))
    }

    /// Includes the base grammar's top-level patterns (`"include": "$base"`):
    /// the grammar of the file being highlighted. That is this grammar,
    /// unless this one is embedded in another (a SQL grammar inside a host
    /// language's strings, say), in which case it is the host.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginEnd, Grammar, Rule};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(BeginEnd::new(r"\(", r"\)").pattern(Rule::include_base()))
    ///     .to_json()?;
    /// assert!(json.contains(r#""include": "$base""#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn include_base() -> Rule {
        Rule(Kind::Include(Include::Base))
    }

    /// Includes another grammar's top-level patterns by its scope name
    /// (`"include": "source.js"`), to embed one language in another.
    ///
    /// The other grammar is not available here, so only the scope name's
    /// syntax is checked. Naming this grammar's own scope is the same as
    /// [`include_self`](Rule::include_self).
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `scope` | The other grammar's `scopeName`. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginEnd, Grammar, Rule};
    ///
    /// let json = Grammar::new("text.html.demo")
    ///     .pattern(
    ///         BeginEnd::new("<script>", "</script>")
    ///             .content_scope("source.js.embedded.demo")
    ///             .pattern(Rule::include_scope("source.js")),
    ///     )
    ///     .to_json()?;
    /// assert!(json.contains(r#""include": "source.js""#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn include_scope(scope: impl Into<Text>) -> Rule {
        Rule(Kind::Include(Include::Scope(scope.into())))
    }

    /// Includes one repository entry of another grammar
    /// (`"include": "source.js#expression"`).
    ///
    /// Only the syntax is checked, unless `scope` is this grammar's own
    /// scope name: then the entry must exist, as for
    /// [`include`](Rule::include).
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `scope` | The other grammar's `scopeName`. |
    /// | `name` | The entry in that grammar's repository. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Grammar, Rule};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(Rule::include_scope_entry("source.js", "expression"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""include": "source.js#expression""#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn include_scope_entry(scope: impl Into<Text>, name: impl Into<Text>) -> Rule {
        Rule(Kind::Include(Include::ScopeEntry(
            scope.into(),
            name.into(),
        )))
    }

    /// A list of rules tried together, as one rule (`{"patterns": [...]}`).
    ///
    /// The usual body of a repository entry that groups related rules, so
    /// that one include pulls in all of them. The rules are tried in order;
    /// at each position the earliest match wins, and the first rule in the
    /// list wins a tie.
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `rules` | The rules, in priority order. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Grammar, Match, Rule};
    ///
    /// let literals = Rule::patterns([
    ///     Match::new(r"\b(true|false)\b").scope("constant.language.demo").into(),
    ///     Match::new(r"\b\d+\b").scope("constant.numeric.demo").into(),
    /// ]);
    /// let json = Grammar::new("source.demo")
    ///     .pattern(Rule::include("literals"))
    ///     .repository("literals", literals)
    ///     .to_json_compact()?;
    /// assert!(json.contains(r#""literals":{"patterns":[{"name":"constant.language.demo""#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn patterns(rules: impl IntoIterator<Item = Rule>) -> Rule {
        Rule(Kind::Patterns(rules.into_iter().collect()))
    }
}

impl Drop for Rule {
    /// Drops nested rules from an explicit stack, so dropping a rule nested
    /// arbitrarily deep cannot overflow the call stack.
    fn drop(&mut self) {
        let mut stack = Vec::new();
        self.0.take_children(&mut stack);
        while let Some(mut rule) = stack.pop() {
            rule.0.take_children(&mut stack);
        }
    }
}

impl Kind {
    /// Moves every directly nested rule into `out`.
    fn take_children(&mut self, out: &mut Vec<Rule>) {
        fn captures(list: &mut Captures, out: &mut Vec<Rule>) {
            for (_, capture) in list.iter_mut() {
                out.append(&mut capture.patterns);
            }
        }
        match self {
            Kind::Match(m) => captures(&mut m.captures, out),
            Kind::BeginEnd(r) => {
                out.append(&mut r.patterns);
                captures(&mut r.captures, out);
                captures(&mut r.begin_captures, out);
                captures(&mut r.end_captures, out);
            }
            Kind::BeginWhile(r) => {
                out.append(&mut r.patterns);
                captures(&mut r.captures, out);
                captures(&mut r.begin_captures, out);
                captures(&mut r.while_captures, out);
            }
            Kind::Patterns(rules) => out.append(rules),
            Kind::Include(_) => {}
        }
    }
}

/// What to do with the text a capture group matched: give it a scope, run
/// patterns over it, or both.
///
/// A scope name converts into a `Capture` directly, so the common case is
/// `.capture(1, "entity.name.function.demo")`.
///
/// # Examples
///
/// ```
/// use textmate_lang::{Capture, Grammar, Match, Rule};
///
/// let json = Grammar::new("source.demo")
///     .pattern(
///         Match::new(r#"(import)\s+("[^"]*")"#)
///             .capture(1, "keyword.control.import.demo")
///             .capture(2, Capture::new().scope("string.quoted.demo").pattern(Rule::include("escapes"))),
///     )
///     .repository("escapes", textmate_lang::Match::new(r"\\.").scope("constant.character.escape.demo"))
///     .to_json()?;
/// assert!(json.contains(r#""2": {"#));
/// # Ok::<(), textmate_lang::Error>(())
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capture {
    pub(crate) scope: Option<Text>,
    pub(crate) patterns: Vec<Rule>,
}

impl Capture {
    /// A capture that does nothing until given a scope or patterns.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::Capture;
    ///
    /// let capture = Capture::new().scope("variable.parameter.demo");
    /// assert_eq!(capture, Capture::from("variable.parameter.demo"));
    /// ```
    #[must_use]
    pub fn new() -> Capture {
        Capture::default()
    }

    /// Scopes the captured text (`"name"`). May substitute captured text
    /// with `$n`, `${n:/downcase}`, or `${n:/upcase}`.
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `scope` | One scope name, or several separated by single spaces. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Capture, Grammar, Match};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(Match::new(r"(\w+)!").capture(1, Capture::new().scope("support.function.$1.demo")))
    ///     .to_json()?;
    /// assert!(json.contains("support.function.$1.demo"));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn scope(mut self, scope: impl Into<Text>) -> Capture {
        self.scope = Some(scope.into());
        self
    }

    /// Adds a rule run over the captured text (`"patterns"`), to highlight
    /// inside it.
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `rule` | The rule to add. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Capture, Grammar, Match};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(
    ///         Match::new(r"@(\w+(?:\.\w+)*)")
    ///             .capture(1, Capture::new().pattern(Match::new(r"\.").scope("punctuation.separator.demo"))),
    ///     )
    ///     .to_json()?;
    /// assert!(json.contains("punctuation.separator.demo"));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn pattern(mut self, rule: impl Into<Rule>) -> Capture {
        self.patterns.push(rule.into());
        self
    }
}

impl From<&'static str> for Capture {
    fn from(scope: &'static str) -> Capture {
        Capture::new().scope(scope)
    }
}

impl From<alloc::string::String> for Capture {
    fn from(scope: alloc::string::String) -> Capture {
        Capture::new().scope(scope)
    }
}

impl From<Text> for Capture {
    fn from(scope: Text) -> Capture {
        Capture::new().scope(scope)
    }
}

/// A rule that matches one regular expression (`"match"`).
///
/// The whole match takes the rule's [`scope`](Match::scope); capture groups
/// take their own with [`capture`](Match::capture). Group 0 is the whole
/// match.
///
/// # Examples
///
/// ```
/// use textmate_lang::{Grammar, Match};
///
/// let json = Grammar::new("source.demo")
///     .pattern(
///         Match::new(r"\b(fn)\s+([a-z_]\w*)")
///             .capture(1, "storage.type.function.demo")
///             .capture(2, "entity.name.function.demo"),
///     )
///     .to_json()?;
/// assert!(json.contains(r#""match": "\\b(fn)\\s+([a-z_]\\w*)""#));
/// # Ok::<(), textmate_lang::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub(crate) regex: Text,
    pub(crate) scope: Option<Text>,
    pub(crate) captures: Captures,
}

impl Match {
    /// A rule matching `regex`, an Oniguruma regular expression in the
    /// [supported subset](crate::validate_regex).
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `regex` | The pattern. Must be valid and non-empty, checked on emission. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Error, Grammar, Match, RegexErrorKind};
    ///
    /// let err = Grammar::new("source.demo").pattern(Match::new("(unclosed")).to_json().unwrap_err();
    /// match err {
    ///     Error::InvalidRegex { path, error } => {
    ///         assert_eq!(path, "/patterns/0/match");
    ///         assert_eq!(error.kind, RegexErrorKind::UnclosedGroup);
    ///     }
    ///     other => panic!("{other}"),
    /// }
    /// ```
    #[must_use]
    pub fn new(regex: impl Into<Text>) -> Match {
        Match {
            regex: regex.into(),
            scope: None,
            captures: Vec::new(),
        }
    }

    /// Scopes the whole match (`"name"`). May substitute captured text with
    /// `$n`, `${n:/downcase}`, or `${n:/upcase}`.
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `scope` | One scope name, or several separated by single spaces. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Grammar, Match};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(Match::new(r"\b(if|else)\b").scope("keyword.control.$1.demo"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""name": "keyword.control.$1.demo""#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn scope(mut self, scope: impl Into<Text>) -> Match {
        self.scope = Some(scope.into());
        self
    }

    /// Sets what to do with capture group `group` (`"captures"`).
    ///
    /// The group must exist in the pattern
    /// ([`Error::CaptureOutOfRange`](crate::Error::CaptureOutOfRange)), and
    /// may be set once ([`Error::DuplicateCapture`](crate::Error::DuplicateCapture)).
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `group` | The group number; 0 is the whole match. |
    /// | `capture` | A scope name, or a [`Capture`]. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Grammar, Match};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(Match::new(r"(\w+)\s*(=)").capture(2, "keyword.operator.demo").capture(1, "variable.demo"))
    ///     .to_json_compact()?;
    /// // Captures are emitted in group order.
    /// assert!(json.contains(r#""captures":{"1":{"name":"variable.demo"},"2":"#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn capture(mut self, group: u32, capture: impl Into<Capture>) -> Match {
        self.captures.push((group, capture.into()));
        self
    }
}

/// A region from a `begin` match to an `end` match (`"begin"`/`"end"`).
///
/// Between the two, the region's own [`pattern`](BeginEnd::pattern)s apply
/// instead of the surrounding ones. Numbered back-references in `end`
/// (`\1`) stand for the text the `begin` groups captured, which is how a
/// heredoc or a raw string ends with its opening delimiter.
///
/// # Examples
///
/// ```
/// use textmate_lang::{BeginEnd, Grammar, Match};
///
/// let string = BeginEnd::new("\"", "\"")
///     .scope("string.quoted.double.demo")
///     .begin_capture(0, "punctuation.definition.string.begin.demo")
///     .end_capture(0, "punctuation.definition.string.end.demo")
///     .pattern(Match::new(r"\\.").scope("constant.character.escape.demo"));
/// let raw = BeginEnd::new(r#"r(#*)""#, r#""\1"#).scope("string.quoted.raw.demo");
/// let json = Grammar::new("source.demo").pattern(string).pattern(raw).to_json()?;
/// assert!(json.contains(r#""end": "\"\\1""#));
/// # Ok::<(), textmate_lang::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeginEnd {
    pub(crate) begin: Text,
    pub(crate) end: Text,
    pub(crate) scope: Option<Text>,
    pub(crate) content_scope: Option<Text>,
    pub(crate) captures: Captures,
    pub(crate) begin_captures: Captures,
    pub(crate) end_captures: Captures,
    pub(crate) patterns: Vec<Rule>,
    pub(crate) apply_end_pattern_last: bool,
}

impl BeginEnd {
    /// A region from a match of `begin` to the next match of `end`.
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `begin` | The pattern that opens the region. |
    /// | `end` | The pattern that closes it; `\n` back-references name `begin` groups. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginEnd, Grammar};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(BeginEnd::new(r"/\*", r"\*/").scope("comment.block.demo"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""begin": "/\\*""#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn new(begin: impl Into<Text>, end: impl Into<Text>) -> BeginEnd {
        BeginEnd {
            begin: begin.into(),
            end: end.into(),
            scope: None,
            content_scope: None,
            captures: Vec::new(),
            begin_captures: Vec::new(),
            end_captures: Vec::new(),
            patterns: Vec::new(),
            apply_end_pattern_last: false,
        }
    }

    /// Scopes the whole region, delimiters included (`"name"`). `$n`
    /// substitutions name `begin` groups.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginEnd, Grammar};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(BeginEnd::new(r"\(", r"\)").scope("meta.parens.demo"))
    ///     .to_json()?;
    /// assert!(json.contains("meta.parens.demo"));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn scope(mut self, scope: impl Into<Text>) -> BeginEnd {
        self.scope = Some(scope.into());
        self
    }

    /// Scopes the text between the delimiters only (`"contentName"`). `$n`
    /// substitutions name `begin` groups.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginEnd, Grammar};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(BeginEnd::new("```(\\w+)", "```").content_scope("meta.embedded.block.$1"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""contentName": "meta.embedded.block.$1""#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn content_scope(mut self, scope: impl Into<Text>) -> BeginEnd {
        self.content_scope = Some(scope.into());
        self
    }

    /// Sets capture `group` for both `begin` and `end` (`"captures"`).
    /// Ignored for a side that has its own captures; the group must exist in
    /// each pattern it applies to.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginEnd, Grammar};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(BeginEnd::new(r"\[", r"\]").capture(0, "punctuation.brackets.demo"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""captures": {"#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn capture(mut self, group: u32, capture: impl Into<Capture>) -> BeginEnd {
        self.captures.push((group, capture.into()));
        self
    }

    /// Sets capture `group` of `begin` (`"beginCaptures"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginEnd, Grammar};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(BeginEnd::new(r"(<)(\w+)", "/>").begin_capture(2, "entity.name.tag.demo"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""beginCaptures": {"#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn begin_capture(mut self, group: u32, capture: impl Into<Capture>) -> BeginEnd {
        self.begin_captures.push((group, capture.into()));
        self
    }

    /// Sets capture `group` of `end` (`"endCaptures"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginEnd, Grammar};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(BeginEnd::new("<<", "(>>)").end_capture(1, "punctuation.demo"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""endCaptures": {"#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn end_capture(mut self, group: u32, capture: impl Into<Capture>) -> BeginEnd {
        self.end_captures.push((group, capture.into()));
        self
    }

    /// Adds a rule that applies inside the region (`"patterns"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginEnd, Grammar, Rule};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(BeginEnd::new(r"\{", r"\}").pattern(Rule::include_self()))
    ///     .to_json()?;
    /// assert!(json.contains(r#""patterns": ["#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn pattern(mut self, rule: impl Into<Rule>) -> BeginEnd {
        self.patterns.push(rule.into());
        self
    }

    /// Whether `end` is tried after the region's patterns rather than before
    /// (`"applyEndPatternLast": 1`), so a pattern that matches at the same
    /// position as `end` wins.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginEnd, Grammar};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(BeginEnd::new("<", ">").apply_end_pattern_last(true))
    ///     .to_json()?;
    /// assert!(json.contains(r#""applyEndPatternLast": 1"#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn apply_end_pattern_last(mut self, last: bool) -> BeginEnd {
        self.apply_end_pattern_last = last;
        self
    }
}

/// A region that opens at a `begin` match and continues, line by line, for
/// as long as a `while` pattern matches at the start of each following line
/// (`"begin"`/`"while"`), as Markdown block quotes and lists do.
///
/// Numbered back-references in `while` stand for the text `begin` groups
/// captured.
///
/// # Examples
///
/// ```
/// use textmate_lang::{BeginWhile, Grammar, Rule};
///
/// let quote = BeginWhile::new(r"^\s*(>) ?", r"^\s*(>) ?")
///     .scope("markup.quote.demo")
///     .begin_capture(1, "punctuation.definition.quote.demo")
///     .while_capture(1, "punctuation.definition.quote.demo")
///     .pattern(Rule::include_self());
/// let json = Grammar::new("text.demo").pattern(quote).to_json()?;
/// assert!(json.contains(r#""while": "^\\s*(>) ?""#));
/// # Ok::<(), textmate_lang::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeginWhile {
    pub(crate) begin: Text,
    pub(crate) while_: Text,
    pub(crate) scope: Option<Text>,
    pub(crate) content_scope: Option<Text>,
    pub(crate) captures: Captures,
    pub(crate) begin_captures: Captures,
    pub(crate) while_captures: Captures,
    pub(crate) patterns: Vec<Rule>,
}

impl BeginWhile {
    /// A region opened by `begin` and continued while `while_` matches.
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `begin` | The pattern that opens the region. |
    /// | `while_` | The pattern each following line must start with; `\n` back-references name `begin` groups. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginWhile, Grammar};
    ///
    /// let json = Grammar::new("text.demo")
    ///     .pattern(BeginWhile::new("^(\\s{4})", "^\\1").scope("markup.raw.block.demo"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""while": "^\\1""#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn new(begin: impl Into<Text>, while_: impl Into<Text>) -> BeginWhile {
        BeginWhile {
            begin: begin.into(),
            while_: while_.into(),
            scope: None,
            content_scope: None,
            captures: Vec::new(),
            begin_captures: Vec::new(),
            while_captures: Vec::new(),
            patterns: Vec::new(),
        }
    }

    /// Scopes the whole region (`"name"`). `$n` substitutions name `begin`
    /// groups.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginWhile, Grammar};
    ///
    /// let json = Grammar::new("text.demo")
    ///     .pattern(BeginWhile::new("^>", "^>").scope("markup.quote.demo"))
    ///     .to_json()?;
    /// assert!(json.contains("markup.quote.demo"));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn scope(mut self, scope: impl Into<Text>) -> BeginWhile {
        self.scope = Some(scope.into());
        self
    }

    /// Scopes the region without the `begin` match (`"contentName"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginWhile, Grammar};
    ///
    /// let json = Grammar::new("text.demo")
    ///     .pattern(BeginWhile::new("^>", "^>").content_scope("markup.quote.content.demo"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""contentName": "markup.quote.content.demo""#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn content_scope(mut self, scope: impl Into<Text>) -> BeginWhile {
        self.content_scope = Some(scope.into());
        self
    }

    /// Sets capture `group` for both `begin` and `while` (`"captures"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginWhile, Grammar};
    ///
    /// let json = Grammar::new("text.demo")
    ///     .pattern(BeginWhile::new("^(>)", "^(>)").capture(1, "punctuation.demo"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""captures": {"#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn capture(mut self, group: u32, capture: impl Into<Capture>) -> BeginWhile {
        self.captures.push((group, capture.into()));
        self
    }

    /// Sets capture `group` of `begin` (`"beginCaptures"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginWhile, Grammar};
    ///
    /// let json = Grammar::new("text.demo")
    ///     .pattern(BeginWhile::new("^(>)", "^>").begin_capture(1, "punctuation.demo"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""beginCaptures": {"#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn begin_capture(mut self, group: u32, capture: impl Into<Capture>) -> BeginWhile {
        self.begin_captures.push((group, capture.into()));
        self
    }

    /// Sets capture `group` of `while` (`"whileCaptures"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginWhile, Grammar};
    ///
    /// let json = Grammar::new("text.demo")
    ///     .pattern(BeginWhile::new("^>", "^(>)").while_capture(1, "punctuation.demo"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""whileCaptures": {"#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn while_capture(mut self, group: u32, capture: impl Into<Capture>) -> BeginWhile {
        self.while_captures.push((group, capture.into()));
        self
    }

    /// Adds a rule that applies inside the region (`"patterns"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{BeginWhile, Grammar, Match};
    ///
    /// let json = Grammar::new("text.demo")
    ///     .pattern(BeginWhile::new("^>", "^>").pattern(Match::new(r"\*\*").scope("markup.bold.demo")))
    ///     .to_json()?;
    /// assert!(json.contains("markup.bold.demo"));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn pattern(mut self, rule: impl Into<Rule>) -> BeginWhile {
        self.patterns.push(rule.into());
        self
    }
}

impl From<Match> for Rule {
    fn from(rule: Match) -> Rule {
        Rule(Kind::Match(rule))
    }
}

impl From<BeginEnd> for Rule {
    fn from(rule: BeginEnd) -> Rule {
        Rule(Kind::BeginEnd(rule))
    }
}

impl From<BeginWhile> for Rule {
    fn from(rule: BeginWhile) -> Rule {
        Rule(Kind::BeginWhile(rule))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deep_nesting_drops_without_recursion() {
        let mut rule = Rule::include("leaf");
        for _ in 0..200_000 {
            rule = BeginEnd::new("a", "b")
                .pattern(Rule::patterns([rule]))
                .into();
        }
        drop(rule);
        let mut capture_chain = Rule::include("leaf");
        for _ in 0..100_000 {
            capture_chain = Match::new("(a)")
                .capture(1, Capture::new().pattern(capture_chain))
                .into();
        }
        drop(capture_chain);
    }

    #[test]
    fn test_conversions() {
        let a: Rule = Match::new("x").into();
        assert!(matches!(a.0, Kind::Match(_)));
        let b: Rule = BeginWhile::new("x", "y").into();
        assert!(matches!(b.0, Kind::BeginWhile(_)));
        assert_eq!(
            Capture::from(alloc::string::String::from("s"))
                .scope
                .as_deref(),
            Some("s")
        );
        assert_eq!(
            Capture::from(Text::Borrowed("t")).scope.as_deref(),
            Some("t")
        );
    }
}
