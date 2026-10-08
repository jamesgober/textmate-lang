//! The grammar: a scope name, top-level patterns, a repository of named
//! rules, and injections.

use alloc::{string::String, vec::Vec};

use crate::{Error, Rule, Style, check, json, rule::Text};

/// A TextMate grammar, built in Rust and emitted as `tmLanguage.json`.
///
/// A grammar is a scope name (`source.demo`) and a list of top-level
/// patterns — the [`Rule`]s the tokenizer tries at each position — plus a
/// repository of named rules that patterns include by name, and optionally
/// injections into other grammars. Build one by chaining: every method takes
/// the grammar by value and returns it, and nothing is checked until it is
/// emitted.
///
/// [`to_json`](Grammar::to_json) validates the whole grammar — every regular
/// expression against the [supported Oniguruma subset](crate::validate_regex),
/// every scope name, every include, every capture — and returns the JSON, or
/// the first problem as an [`Error`] that says where it is. Output is
/// deterministic: the same grammar always produces the same bytes, whatever
/// order its repository entries, injections, and captures were added in.
///
/// # Examples
///
/// ```
/// use textmate_lang::{BeginEnd, Grammar, Match, Rule};
///
/// let json = Grammar::new("source.demo")
///     .name("Demo")
///     .file_type("demo")
///     .pattern(Rule::include("comments"))
///     .pattern(Rule::include("strings"))
///     .pattern(Match::new(r"\b(if|else|while|return)\b").scope("keyword.control.demo"))
///     .pattern(Match::new(r"\b\d+(\.\d+)?\b").scope("constant.numeric.demo"))
///     .repository("comments", Match::new("#.*$").scope("comment.line.number-sign.demo"))
///     .repository(
///         "strings",
///         BeginEnd::new("\"", "\"")
///             .scope("string.quoted.double.demo")
///             .pattern(Match::new(r"\\.").scope("constant.character.escape.demo")),
///     )
///     .to_json()?;
///
/// assert!(json.starts_with("{\n  \"name\": \"Demo\",\n  \"scopeName\": \"source.demo\","));
/// assert!(json.contains("\"match\": \"\\\\b(if|else|while|return)\\\\b\""));
/// # Ok::<(), textmate_lang::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grammar {
    pub(crate) scope_name: Text,
    pub(crate) name: Option<Text>,
    pub(crate) file_types: Vec<Text>,
    pub(crate) first_line_match: Option<Text>,
    pub(crate) injection_selector: Option<Text>,
    pub(crate) patterns: Vec<Rule>,
    pub(crate) repository: Vec<(Text, Rule)>,
    pub(crate) injections: Vec<(Text, Rule)>,
}

impl Grammar {
    /// Starts an empty grammar whose root scope is `scope_name`
    /// (`"scopeName"`).
    ///
    /// The scope name is how editors and other grammars refer to this one:
    /// by convention `source.<language>` for programming languages and
    /// `text.<format>` for markup. It is a single scope without spaces or
    /// substitutions ([`Error::InvalidScope`]), checked when the grammar is
    /// emitted.
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `scope_name` | The root scope, such as `source.rust`: a `&'static str` (not copied) or a `String`. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::Grammar;
    ///
    /// let json = Grammar::new("source.toml").to_json_compact()?;
    /// assert_eq!(json, r#"{"scopeName":"source.toml","patterns":[]}"#);
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn new(scope_name: impl Into<Text>) -> Grammar {
        Grammar {
            scope_name: scope_name.into(),
            name: None,
            file_types: Vec::new(),
            first_line_match: None,
            injection_selector: None,
            patterns: Vec::new(),
            repository: Vec::new(),
            injections: Vec::new(),
        }
    }

    /// The grammar's root scope, as given to [`new`](Grammar::new).
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::Grammar;
    ///
    /// assert_eq!(Grammar::new("source.demo").scope_name(), "source.demo");
    /// ```
    #[must_use]
    pub fn scope_name(&self) -> &str {
        &self.scope_name
    }

    /// Sets the display name (`"name"`), shown in editors' language
    /// pickers. Any text; it is escaped on output.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::Grammar;
    ///
    /// let json = Grammar::new("source.demo").name("Demo \"Lang\"").to_json_compact()?;
    /// assert!(json.starts_with(r#"{"name":"Demo \"Lang\"","#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn name(mut self, name: impl Into<Text>) -> Grammar {
        self.name = Some(name.into());
        self
    }

    /// Adds a file extension the grammar applies to (`"fileTypes"`), such as
    /// `rs` or `tmLanguage.json`. Must not be empty ([`Error::InvalidName`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::Grammar;
    ///
    /// let json = Grammar::new("source.demo").file_type("dm").file_type("demo").to_json_compact()?;
    /// assert!(json.contains(r#""fileTypes":["dm","demo"]"#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn file_type(mut self, extension: impl Into<Text>) -> Grammar {
        self.file_types.push(extension.into());
        self
    }

    /// Sets a pattern matched against a file's first line to recognize the
    /// language when the extension does not (`"firstLineMatch"`), such as a
    /// shebang.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::Grammar;
    ///
    /// let json = Grammar::new("source.demo").first_line_match(r"^#!.*\bdemo\b").to_json()?;
    /// assert!(json.contains(r#""firstLineMatch": "^#!.*\\bdemo\\b""#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn first_line_match(mut self, regex: impl Into<Text>) -> Grammar {
        self.first_line_match = Some(regex.into());
        self
    }

    /// Makes this an injection grammar (`"injectionSelector"`): its
    /// top-level patterns are injected into the scopes the selector matches
    /// in other grammars, as VS Code does for grammars that contribute
    /// `injectTo`.
    ///
    /// | Parameter | Meaning |
    /// |---|---|
    /// | `selector` | A scope selector, such as `L:source.js -comment -string`. |
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Grammar, Match};
    ///
    /// let json = Grammar::new("todo.injection")
    ///     .injection_selector("L:comment")
    ///     .pattern(Match::new(r"\bTODO\b").scope("keyword.todo"))
    ///     .to_json()?;
    /// assert!(json.contains(r#""injectionSelector": "L:comment""#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn injection_selector(mut self, selector: impl Into<Text>) -> Grammar {
        self.injection_selector = Some(selector.into());
        self
    }

    /// Adds a top-level pattern (`"patterns"`). Patterns are tried in the
    /// order they are added: at each position the earliest match wins, and
    /// the first pattern wins a tie.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Grammar, Match};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(Match::new("==|!=").scope("keyword.operator.comparison.demo"))
    ///     .pattern(Match::new("=").scope("keyword.operator.assignment.demo"))
    ///     .to_json()?;
    /// let comparison = json.find("comparison").unwrap();
    /// let assignment = json.find("assignment").unwrap();
    /// assert!(comparison < assignment);
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn pattern(mut self, rule: impl Into<Rule>) -> Grammar {
        self.patterns.push(rule.into());
        self
    }

    /// Adds a repository entry called `name` (`"repository"`), which rules
    /// include with [`Rule::include`]. Names are ASCII letters, digits, `_`,
    /// `-`, and `.` ([`Error::InvalidName`]) and unique
    /// ([`Error::DuplicateRepository`]). Entries are emitted sorted by name.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Grammar, Match, Rule};
    ///
    /// let mut grammar = Grammar::new("source.demo");
    /// for (name, regex) in [("numbers", r"\d+"), ("booleans", r"\b(true|false)\b")] {
    ///     grammar = grammar
    ///         .pattern(Rule::include(name))
    ///         .repository(name, Match::new(regex).scope(format!("constant.{name}.demo")));
    /// }
    /// let json = grammar.to_json()?;
    /// assert!(json.find("\"booleans\": {").unwrap() < json.find("\"numbers\": {").unwrap());
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn repository(mut self, name: impl Into<Text>, rule: impl Into<Rule>) -> Grammar {
        self.repository.push((name.into(), rule.into()));
        self
    }

    /// Adds an injection (`"injections"`): `rule` is tried, inside this
    /// grammar, wherever the scope stack matches `selector`. Selectors are
    /// unique ([`Error::DuplicateInjection`]); entries are emitted sorted.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Grammar, Match, Rule};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .injection(
    ///         "L:string.quoted.demo",
    ///         Rule::patterns([Match::new(r"\{\w+\}").scope("variable.interpolation.demo").into()]),
    ///     )
    ///     .to_json()?;
    /// assert!(json.contains(r#""L:string.quoted.demo": {"#));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    #[must_use]
    pub fn injection(mut self, selector: impl Into<Text>, rule: impl Into<Rule>) -> Grammar {
        self.injections.push((selector.into(), rule.into()));
        self
    }

    /// Checks the whole grammar without emitting it.
    ///
    /// # Errors
    ///
    /// The first problem found, as an [`Error`]; see its documentation for
    /// the order of the checks.
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Error, Grammar, Match};
    ///
    /// let grammar = Grammar::new("source.demo").pattern(Match::new("(a)").capture(2, "x"));
    /// assert_eq!(
    ///     grammar.validate(),
    ///     Err(Error::CaptureOutOfRange { path: "/patterns/0/captures/2".into(), group: 2, groups: 1 }),
    /// );
    /// ```
    pub fn validate(&self) -> Result<(), Error> {
        check::check(self).map(|_| ())
    }

    /// Validates the grammar and returns it as pretty-printed
    /// `tmLanguage.json`: two-space indentation, a final newline. The
    /// Tier-1 call: build a grammar, call this, write the file.
    ///
    /// # Errors
    ///
    /// The first problem found, as an [`Error`].
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Grammar, Match};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(Match::new(r"\bdemo\b").scope("keyword.demo"))
    ///     .to_json()?;
    /// assert_eq!(json, r#"{
    ///   "scopeName": "source.demo",
    ///   "patterns": [
    ///     {
    ///       "name": "keyword.demo",
    ///       "match": "\\bdemo\\b"
    ///     }
    ///   ]
    /// }
    /// "#);
    /// // std::fs::write("syntaxes/demo.tmLanguage.json", json)?;
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    pub fn to_json(&self) -> Result<String, Error> {
        let mut out = String::new();
        self.write_json(&mut out, Style::Pretty)?;
        Ok(out)
    }

    /// Validates the grammar and returns it as compact JSON, with no
    /// whitespace.
    ///
    /// # Errors
    ///
    /// The first problem found, as an [`Error`].
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Grammar, Match};
    ///
    /// let json = Grammar::new("source.demo")
    ///     .pattern(Match::new(r"\bdemo\b").scope("keyword.demo"))
    ///     .to_json_compact()?;
    /// assert_eq!(json, r#"{"scopeName":"source.demo","patterns":[{"name":"keyword.demo","match":"\\bdemo\\b"}]}"#);
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    pub fn to_json_compact(&self) -> Result<String, Error> {
        let mut out = String::new();
        self.write_json(&mut out, Style::Compact)?;
        Ok(out)
    }

    /// Validates the grammar and appends it to `out` in `style`, reusing the
    /// buffer's capacity. On an error nothing is appended.
    ///
    /// # Errors
    ///
    /// The first problem found, as an [`Error`].
    ///
    /// # Examples
    ///
    /// ```
    /// use textmate_lang::{Grammar, Style};
    ///
    /// let mut out = String::with_capacity(4096);
    /// for scope in ["source.a", "source.b"] {
    ///     out.clear();
    ///     Grammar::new(scope).write_json(&mut out, Style::Compact)?;
    ///     assert!(out.contains(scope));
    /// }
    /// assert!(Grammar::new("bad scope").write_json(&mut out, Style::Compact).is_err());
    /// assert!(out.contains("source.b"));
    /// # Ok::<(), textmate_lang::Error>(())
    /// ```
    pub fn write_json(&self, out: &mut String, style: Style) -> Result<(), Error> {
        let checked = check::check(self)?;
        json::write(self, &checked, out, style == Style::Pretty);
        Ok(())
    }
}
