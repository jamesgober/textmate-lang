//! Property tests.
//!
//! - **No injection.** Random grammars whose names, file types, and literal
//!   pattern text are arbitrary strings — quotes, backslashes, control
//!   characters, line separators, any Unicode — are emitted in both styles
//!   and read back by a strict RFC 8259 parser written here. The value read
//!   back is exactly the value a direct reference construction expects: no
//!   string ever changes the structure of the output.
//! - **Determinism.** The order repository entries, injections, and captures
//!   are added in never changes the output.
//! - **The regex checker** accepts every pattern printed from a random
//!   syntax tree of the subset, with the right group count; refuses each with
//!   a closing parenthesis removed; never panics on arbitrary input; and
//!   reports offsets on character boundaries. `escape_regex` output is always
//!   accepted, in `match` and in `end` patterns.
//! - **Include cycles** are reported exactly when a reference reachability
//!   computation finds one, and the reported cycle is a real one.
//! - **Scope names** are accepted exactly when a reference reading of the
//!   naming rules accepts them.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use textmate_lang::{
    BeginEnd, BeginWhile, Capture, Error, Grammar, Match, Rule, Style, escape_regex, validate_regex,
};

// --- A strict JSON reader ----------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum J {
    Bool(bool),
    Null,
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

fn obj(members: Vec<(&str, J)>) -> J {
    J::Obj(
        members
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

fn s(text: &str) -> J {
    J::Str(text.to_string())
}

/// RFC 8259, strictly: no trailing commas, no duplicate keys, no raw control
/// characters in strings, no lone surrogates, only the four whitespace
/// characters, nothing after the value.
struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

fn parse(text: &str) -> Result<J, String> {
    let mut r = Reader {
        b: text.as_bytes(),
        i: 0,
    };
    r.ws();
    let v = r.value()?;
    r.ws();
    if r.i != r.b.len() {
        return Err(format!("trailing input at {}", r.i));
    }
    Ok(v)
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected {:?} at {}", c as char, self.i))
        }
    }

    fn value(&mut self) -> Result<J, String> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(J::Str),
            Some(b't') => self.literal("true", J::Bool(true)),
            Some(b'f') => self.literal("false", J::Bool(false)),
            Some(b'n') => self.literal("null", J::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            other => Err(format!("unexpected {other:?} at {}", self.i)),
        }
    }

    fn literal(&mut self, word: &str, v: J) -> Result<J, String> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(format!("bad literal at {}", self.i))
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        self.i - start
    }

    fn number(&mut self) -> Result<J, String> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                let _ = self.digits();
            }
            _ => return Err("bad number".into()),
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if self.digits() == 0 {
                return Err("bad fraction".into());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return Err("bad exponent".into());
            }
        }
        Ok(J::Num(
            String::from_utf8(self.b[start..self.i].to_vec()).unwrap(),
        ))
    }

    fn array(&mut self) -> Result<J, String> {
        self.expect(b'[')?;
        self.ws();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(J::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(J::Arr(items));
                }
                _ => return Err(format!("bad array at {}", self.i)),
            }
        }
    }

    fn object(&mut self) -> Result<J, String> {
        self.expect(b'{')?;
        self.ws();
        let mut members: Vec<(String, J)> = Vec::new();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(J::Obj(members));
        }
        loop {
            self.ws();
            let key = self.string()?;
            if members.iter().any(|(k, _)| *k == key) {
                return Err(format!("duplicate key {key:?}"));
            }
            self.ws();
            self.expect(b':')?;
            self.ws();
            let v = self.value()?;
            members.push((key, v));
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(J::Obj(members));
                }
                _ => return Err(format!("bad object at {}", self.i)),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self.b.get(self.i..self.i + 4).ok_or("short \\u escape")?;
        let text = std::str::from_utf8(digits).map_err(|e| e.to_string())?;
        if !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("bad \\u escape".into());
        }
        self.i += 4;
        Ok(u32::from_str_radix(text, 16).unwrap())
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err("unterminated string".into());
            };
            match b {
                b'"' => {
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.i += 1;
                    let Some(e) = self.peek() else {
                        return Err("bad escape".into());
                    };
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let c = if (0xD800..0xDC00).contains(&hi) {
                                if !self.b[self.i..].starts_with(b"\\u") {
                                    return Err("lone high surrogate".into());
                                }
                                self.i += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err("bad low surrogate".into());
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return Err("lone low surrogate".into());
                            } else {
                                hi
                            };
                            out.push(char::from_u32(c).ok_or("bad code point")?);
                        }
                        _ => return Err(format!("bad escape \\{}", e as char)),
                    }
                }
                0x00..=0x1F => return Err(format!("raw control character at {}", self.i)),
                _ => {
                    let rest = std::str::from_utf8(&self.b[self.i..]).unwrap();
                    let c = rest.chars().next().unwrap();
                    out.push(c);
                    self.i += c.len_utf8();
                }
            }
        }
    }
}

#[test]
fn test_reader_is_strict() {
    assert_eq!(
        parse(r#"{"a":[1,-2.5e3,true,null,"x\ny"]}"#).unwrap(),
        obj(vec![(
            "a",
            J::Arr(vec![
                J::Num("1".into()),
                J::Num("-2.5e3".into()),
                J::Bool(true),
                J::Null,
                s("x\ny"),
            ])
        )])
    );
    for bad in [
        "{\"a\":1,}",
        "[1,]",
        "{\"a\":1,\"a\":2}",
        "\"\u{1}\"",
        r#""\ud800""#,
        r#""\udc00""#,
        r#""\x""#,
        "[1] x",
        "01",
        "\u{a0}1",
        "{a:1}",
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
    assert_eq!(parse(r#""😀""#).unwrap(), s("\u{1F600}"));
}

// --- A model of grammars -----------------------------------------------------

/// A capture group's handling.
#[derive(Clone, Debug)]
struct C {
    scope: Option<String>,
    patterns: Vec<M>,
}

/// A pattern: its text and number of groups.
#[derive(Clone, Debug)]
struct Pat {
    text: String,
    groups: u32,
}

#[derive(Clone, Debug)]
enum Inc {
    Repo(usize),
    SelfRef,
    Base,
    Scope,
    ScopeEntry(usize),
}

#[derive(Clone, Debug)]
enum M {
    Match {
        pat: Pat,
        scope: Option<String>,
        caps: Vec<(u32, C)>,
    },
    Region {
        begin: Pat,
        /// `end` or `while`, and which.
        end: Pat,
        is_while: bool,
        scope: Option<String>,
        content: Option<String>,
        begin_caps: Vec<(u32, C)>,
        end_caps: Vec<(u32, C)>,
        last: bool,
        patterns: Vec<M>,
    },
    Include(Inc),
    Patterns(Vec<M>),
}

#[derive(Debug)]
struct Model {
    name: Option<String>,
    file_types: Vec<String>,
    patterns: Vec<M>,
    repository: Vec<(String, M)>,
    injections: Vec<(String, M)>,
}

const OTHER: &str = "source.other";

fn resolve(inc: &Inc, names: &[String]) -> String {
    let pick = |k: usize| names.get(k % names.len().max(1)).cloned();
    match inc {
        Inc::Repo(k) => match pick(*k) {
            Some(name) => format!("#{name}"),
            None => OTHER.to_string(),
        },
        Inc::SelfRef => "$self".into(),
        Inc::Base => "$base".into(),
        Inc::Scope => OTHER.into(),
        Inc::ScopeEntry(k) => format!("{OTHER}#e{k}"),
    }
}

fn to_capture(c: &C, names: &[String]) -> Capture {
    let mut capture = Capture::new();
    if let Some(scope) = &c.scope {
        capture = capture.scope(scope.clone());
    }
    for m in &c.patterns {
        capture = capture.pattern(to_rule(m, names));
    }
    capture
}

fn to_rule(m: &M, names: &[String]) -> Rule {
    match m {
        M::Match { pat, scope, caps } => {
            let mut r = Match::new(pat.text.clone());
            if let Some(scope) = scope {
                r = r.scope(scope.clone());
            }
            for (g, c) in caps {
                r = r.capture(*g, to_capture(c, names));
            }
            r.into()
        }
        M::Region {
            begin,
            end,
            is_while: false,
            scope,
            content,
            begin_caps,
            end_caps,
            last,
            patterns,
        } => {
            let mut r =
                BeginEnd::new(begin.text.clone(), end.text.clone()).apply_end_pattern_last(*last);
            if let Some(scope) = scope {
                r = r.scope(scope.clone());
            }
            if let Some(content) = content {
                r = r.content_scope(content.clone());
            }
            for (g, c) in begin_caps {
                r = r.begin_capture(*g, to_capture(c, names));
            }
            for (g, c) in end_caps {
                r = r.end_capture(*g, to_capture(c, names));
            }
            for p in patterns {
                r = r.pattern(to_rule(p, names));
            }
            r.into()
        }
        M::Region {
            begin,
            end,
            is_while: true,
            scope,
            content,
            begin_caps,
            end_caps,
            patterns,
            ..
        } => {
            let mut r = BeginWhile::new(begin.text.clone(), end.text.clone());
            if let Some(scope) = scope {
                r = r.scope(scope.clone());
            }
            if let Some(content) = content {
                r = r.content_scope(content.clone());
            }
            for (g, c) in begin_caps {
                r = r.begin_capture(*g, to_capture(c, names));
            }
            for (g, c) in end_caps {
                r = r.while_capture(*g, to_capture(c, names));
            }
            for p in patterns {
                r = r.pattern(to_rule(p, names));
            }
            r.into()
        }
        M::Include(inc) => match inc {
            Inc::Repo(k) => match names.get(k % names.len().max(1)) {
                Some(name) => Rule::include(name.clone()),
                None => Rule::include_scope(OTHER),
            },
            Inc::SelfRef => Rule::include_self(),
            Inc::Base => Rule::include_base(),
            Inc::Scope => Rule::include_scope(OTHER),
            Inc::ScopeEntry(k) => Rule::include_scope_entry(OTHER, format!("e{k}")),
        },
        M::Patterns(list) => Rule::patterns(list.iter().map(|p| to_rule(p, names))),
    }
}

/// Builds the grammar, adding repository entries, injections, and captures
/// in the order `order` gives (a permutation key per item).
fn to_grammar(model: &Model, order: &[u32]) -> Grammar {
    let names: Vec<String> = model.repository.iter().map(|(n, _)| n.clone()).collect();
    let mut g = Grammar::new("source.test");
    if let Some(name) = &model.name {
        g = g.name(name.clone());
    }
    for ft in &model.file_types {
        g = g.file_type(ft.clone());
    }
    for m in &model.patterns {
        g = g.pattern(to_rule(m, &names));
    }
    let key = |i: usize| order.get(i % order.len().max(1)).copied().unwrap_or(0);
    let mut repo: Vec<(usize, &(String, M))> = model.repository.iter().enumerate().collect();
    repo.sort_by_key(|(i, _)| (key(*i), *i));
    for (_, (name, m)) in repo {
        g = g.repository(name.clone(), to_rule(m, &names));
    }
    let mut inj: Vec<(usize, &(String, M))> = model.injections.iter().enumerate().collect();
    inj.sort_by_key(|(i, _)| (key(i + 3), *i));
    for (_, (selector, m)) in inj {
        g = g.injection(selector.clone(), to_rule(m, &names));
    }
    g
}

// --- The expected JSON, built directly --------------------------------------

fn caps_json(caps: &[(u32, C)], names: &[String]) -> Option<J> {
    if caps.is_empty() {
        return None;
    }
    let mut sorted: Vec<&(u32, C)> = caps.iter().collect();
    sorted.sort_by_key(|(g, _)| *g);
    Some(J::Obj(
        sorted
            .into_iter()
            .map(|(g, c)| {
                let mut members = Vec::new();
                if let Some(scope) = &c.scope {
                    members.push(("name".to_string(), s(scope)));
                }
                if !c.patterns.is_empty() {
                    members.push(("patterns".to_string(), rules_json(&c.patterns, names)));
                }
                (g.to_string(), J::Obj(members))
            })
            .collect(),
    ))
}

fn rules_json(rules: &[M], names: &[String]) -> J {
    J::Arr(rules.iter().map(|m| rule_json(m, names)).collect())
}

fn rule_json(m: &M, names: &[String]) -> J {
    let mut out: Vec<(String, J)> = Vec::new();
    let mut put = |k: &str, v: Option<J>| {
        if let Some(v) = v {
            out.push((k.to_string(), v));
        }
    };
    match m {
        M::Include(inc) => put("include", Some(s(&resolve(inc, names)))),
        M::Patterns(list) => put(
            "patterns",
            (!list.is_empty()).then(|| rules_json(list, names)),
        ),
        M::Match { pat, scope, caps } => {
            put("name", scope.as_deref().map(s));
            put("match", Some(s(&pat.text)));
            put("captures", caps_json(caps, names));
        }
        M::Region {
            begin,
            end,
            is_while,
            scope,
            content,
            begin_caps,
            end_caps,
            last,
            patterns,
        } => {
            put("name", scope.as_deref().map(s));
            put("contentName", content.as_deref().map(s));
            put("begin", Some(s(&begin.text)));
            put("beginCaptures", caps_json(begin_caps, names));
            let (key, caps_key) = if *is_while {
                ("while", "whileCaptures")
            } else {
                ("end", "endCaptures")
            };
            put(key, Some(s(&end.text)));
            put(caps_key, caps_json(end_caps, names));
            if *last && !is_while {
                put("applyEndPatternLast", Some(J::Num("1".into())));
            }
            put(
                "patterns",
                (!patterns.is_empty()).then(|| rules_json(patterns, names)),
            );
        }
    }
    J::Obj(out)
}

fn grammar_json(model: &Model) -> J {
    let names: Vec<String> = model.repository.iter().map(|(n, _)| n.clone()).collect();
    let mut members: Vec<(&str, J)> = Vec::new();
    if let Some(name) = &model.name {
        members.push(("name", s(name)));
    }
    members.push(("scopeName", s("source.test")));
    if !model.file_types.is_empty() {
        members.push((
            "fileTypes",
            J::Arr(model.file_types.iter().map(|f| s(f)).collect()),
        ));
    }
    members.push(("patterns", rules_json(&model.patterns, &names)));
    for (key, entries) in [
        ("repository", &model.repository),
        ("injections", &model.injections),
    ] {
        if entries.is_empty() {
            continue;
        }
        let mut sorted: Vec<&(String, M)> = entries.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        members.push((
            key,
            J::Obj(
                sorted
                    .into_iter()
                    .map(|(n, m)| (n.clone(), rule_json(m, &names)))
                    .collect(),
            ),
        ));
    }
    obj(members)
}

// --- Strategies ---------------------------------------------------------------

/// Text that stresses the escaper: quotes, backslashes, control characters,
/// line separators, and arbitrary Unicode.
fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just('"'),
            Just('\\'),
            Just('/'),
            Just('\n'),
            Just('\u{0}'),
            Just('\u{1f}'),
            Just('\u{7f}'),
            Just('\u{2028}'),
            Just('\u{2029}'),
            Just('{'),
            Just('}'),
            Just(','),
            Just(':'),
            Just('1'),
            any::<char>(),
        ],
        0..12,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

/// A pattern of literal pieces, some wrapped in groups.
fn pat() -> impl Strategy<Value = Pat> {
    prop::collection::vec((text(), any::<bool>()), 1..4).prop_map(|pieces| {
        let mut out = String::new();
        let mut groups = 0;
        for (piece, group) in pieces {
            let lit = if piece.is_empty() {
                "x".to_string()
            } else {
                escape_regex(&piece)
            };
            if group {
                groups += 1;
                out.push('(');
                out.push_str(&lit);
                out.push(')');
            } else {
                out.push_str(&lit);
            }
        }
        Pat { text: out, groups }
    })
}

fn scope() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_+-]{0,4}(\\.[a-z0-9_][a-z0-9_+-]{0,4}){0,3}( [a-z][a-z0-9]{0,4}(\\.[a-z0-9]{1,4}){0,2})?(\\.\\$0)?"
}

fn repo_name() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_][a-zA-Z0-9_.-]{0,8}"
}

/// Captures of groups `0..=groups`, each at most once.
fn caps(groups: u32, inner: BoxedStrategy<M>) -> impl Strategy<Value = Vec<(u32, C)>> {
    prop::collection::btree_map(
        0..=groups,
        (
            prop::option::of(scope()),
            prop::collection::vec(inner, 0..2),
        ),
        0..3,
    )
    .prop_map(|map| {
        map.into_iter()
            .map(|(g, (scope, patterns))| (g, C { scope, patterns }))
            .collect()
    })
}

fn include(in_region: bool) -> impl Strategy<Value = Inc> {
    let base = prop_oneof![
        any::<u8>().prop_map(|k| Inc::Repo(usize::from(k))),
        Just(Inc::Scope),
        any::<u8>().prop_map(|k| Inc::ScopeEntry(usize::from(k))),
    ];
    if in_region {
        prop_oneof![base, Just(Inc::SelfRef), Just(Inc::Base)].boxed()
    } else {
        base.boxed()
    }
}

/// A rule that validation accepts. Includes of `$self` and `$base` appear
/// only inside regions and captures, and repository entries are never
/// pattern lists, so no include cycle can arise.
fn rule(entry: bool) -> BoxedStrategy<M> {
    let matching = pat().prop_flat_map(|p| {
        let groups = p.groups;
        (
            Just(p),
            prop::option::of(scope()),
            caps(groups, Just(M::Include(Inc::Scope)).boxed()),
        )
            .prop_map(|(pat, scope, caps)| M::Match { pat, scope, caps })
    });
    let leaf = if entry {
        matching.boxed()
    } else {
        prop_oneof![matching, include(false).prop_map(M::Include)].boxed()
    };
    leaf.prop_recursive(3, 24, 4, move |inner| {
        let region_inner = prop_oneof![inner.clone(), include(true).prop_map(M::Include)].boxed();
        let region = (pat(), pat(), any::<bool>(), any::<bool>()).prop_flat_map(
            move |(begin, end, is_while, last)| {
                let (bg, eg) = (begin.groups, end.groups);
                (
                    Just((begin, end, is_while, last)),
                    prop::option::of(scope()),
                    prop::option::of(scope()),
                    caps(bg, region_inner.clone()),
                    caps(eg, region_inner.clone()),
                    prop::collection::vec(region_inner.clone(), 0..3),
                )
                    .prop_map(
                        |((begin, end, is_while, last), scope, content, bc, ec, patterns)| {
                            M::Region {
                                begin,
                                end,
                                is_while,
                                scope,
                                content,
                                begin_caps: bc,
                                end_caps: ec,
                                last,
                                patterns,
                            }
                        },
                    )
            },
        );
        if entry {
            region.boxed()
        } else {
            prop_oneof![
                region,
                prop::collection::vec(inner, 0..3).prop_map(M::Patterns)
            ]
            .boxed()
        }
    })
    .boxed()
}

fn model() -> impl Strategy<Value = Model> {
    (
        prop::option::of(text()),
        prop::collection::vec(text().prop_filter("non-empty", |t| !t.is_empty()), 0..3),
        prop::collection::vec(rule(false), 0..4),
        prop::collection::btree_map(repo_name(), rule(true), 0..4),
        prop::collection::btree_map(
            prop_oneof![
                Just("L:source.a".to_string()),
                Just("R:text.b -comment".to_string()),
                Just("source.c, (meta.d | string.e)".to_string()),
            ],
            rule(false),
            0..2,
        ),
    )
        .prop_map(
            |(name, file_types, patterns, repository, injections)| Model {
                name,
                file_types,
                patterns,
                repository: repository.into_iter().collect(),
                injections: injections.into_iter().collect(),
            },
        )
}

// --- Regex syntax trees --------------------------------------------------------

#[derive(Clone, Debug)]
enum R {
    Lit(String),
    Class(String, bool),
    Dot,
    Shorthand(&'static str),
    Anchor(&'static str),
    Group(&'static str, Box<R>),
    Look(&'static str, Box<R>),
    Alt(Vec<R>),
    Seq(Vec<R>),
    Quant(Box<R>, &'static str),
}

impl R {
    fn groups(&self) -> u32 {
        match self {
            R::Group(kind, inner) => u32::from(*kind == "(") + inner.groups(),
            R::Look(_, inner) | R::Quant(inner, _) => inner.groups(),
            R::Alt(list) | R::Seq(list) => list.iter().map(R::groups).sum(),
            _ => 0,
        }
    }

    fn count_parens(&self) -> usize {
        match self {
            R::Group(_, inner) | R::Look(_, inner) => 1 + inner.count_parens(),
            R::Quant(inner, _) => inner.count_parens(),
            R::Alt(list) | R::Seq(list) => list.iter().map(R::count_parens).sum(),
            _ => 0,
        }
    }

    /// Prints the pattern; the `skip`-th closing parenthesis (in print
    /// order) is left out if `skip` is set.
    fn print(&self, out: &mut String, skip: &mut Option<usize>) {
        match self {
            R::Lit(text) => out.push_str(&escape_regex(text)),
            R::Class(text, negated) => {
                out.push('[');
                if *negated {
                    out.push('^');
                }
                out.push_str(&escape_regex(text));
                out.push(']');
            }
            R::Dot => out.push('.'),
            R::Shorthand(text) | R::Anchor(text) => out.push_str(text),
            R::Group(open, inner) | R::Look(open, inner) => {
                out.push_str(open);
                inner.print(out, skip);
                match skip {
                    Some(0) => *skip = None,
                    Some(n) => {
                        *n -= 1;
                        out.push(')');
                    }
                    None => out.push(')'),
                }
            }
            R::Alt(list) => {
                for (i, r) in list.iter().enumerate() {
                    if i > 0 {
                        out.push('|');
                    }
                    r.print(out, skip);
                }
            }
            R::Seq(list) => list.iter().for_each(|r| r.print(out, skip)),
            R::Quant(inner, q) => {
                inner.print(out, skip);
                out.push_str(q);
            }
        }
    }
}

fn nonempty_text() -> impl Strategy<Value = String> {
    text().prop_map(|t| if t.is_empty() { "a".into() } else { t })
}

/// One character: a quantifier applies to it as a whole.
fn one_char() -> impl Strategy<Value = String> {
    any::<char>().prop_map(String::from)
}

/// Fixed-length content for a look-behind.
fn fixed() -> impl Strategy<Value = R> {
    prop::collection::vec(
        prop_oneof![
            nonempty_text().prop_map(R::Lit),
            nonempty_text().prop_map(|t| R::Class(t, false)),
            Just(R::Dot),
            Just(R::Shorthand(r"\w")),
            Just(R::Anchor(r"\b")),
        ],
        1..4,
    )
    .prop_map(R::Seq)
}

fn regex_tree() -> impl Strategy<Value = R> {
    let atom = prop_oneof![
        one_char().prop_map(R::Lit),
        nonempty_text().prop_map(|t| R::Class(t, false)),
        nonempty_text().prop_map(|t| R::Class(t, true)),
        Just(R::Dot),
        Just(R::Shorthand(r"\d")),
        Just(R::Shorthand(r"\h")),
        Just(R::Shorthand(r"\p{L}")),
    ];
    let quantifier = prop_oneof![
        Just(""),
        Just("*"),
        Just("+?"),
        Just("?+"),
        Just("{2}"),
        Just("{1,3}?"),
        Just("{0,}"),
    ];
    let leaf = prop_oneof![
        (atom, quantifier).prop_map(|(a, q)| if q.is_empty() {
            a
        } else {
            R::Quant(Box::new(a), q)
        }),
        nonempty_text().prop_map(R::Lit),
        prop_oneof![Just("^"), Just("$"), Just(r"\G"), Just(r"\b"), Just(r"\A")]
            .prop_map(R::Anchor),
        fixed().prop_map(|f| R::Look("(?<=", Box::new(f))),
        fixed().prop_map(|f| R::Look("(?<!", Box::new(f))),
    ];
    leaf.prop_recursive(4, 32, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 1..4).prop_map(R::Seq),
            prop::collection::vec(inner.clone(), 1..4).prop_map(R::Alt),
            (
                prop_oneof![
                    Just("("),
                    Just("(?:"),
                    Just("(?>"),
                    Just("(?i:"),
                    Just("(?-x:")
                ],
                inner.clone(),
                prop_oneof![Just(""), Just("*"), Just("+"), Just("{2,5}")],
            )
                .prop_map(|(open, r, q)| {
                    let group = R::Group(open, Box::new(r));
                    if q.is_empty() {
                        group
                    } else {
                        R::Quant(Box::new(group), q)
                    }
                }),
            (prop_oneof![Just("(?="), Just("(?!")], inner)
                .prop_map(|(o, r)| R::Look(o, Box::new(r))),
        ]
    })
}

// --- Include graphs ------------------------------------------------------------

#[derive(Clone, Debug)]
enum Entry {
    Match,
    List(Vec<Option<usize>>, Vec<Option<usize>>),
    Include(Option<usize>),
    Region(Vec<Option<usize>>),
}

/// `None` is `$self`; `Some(i)` repository entry `i`.
fn target() -> impl Strategy<Value = Option<usize>> {
    prop::option::weighted(0.85, 0..6usize)
}

fn graph() -> impl Strategy<Value = (Vec<Option<usize>>, Vec<Entry>)> {
    let entry = prop_oneof![
        Just(Entry::Match),
        (
            prop::collection::vec(target(), 0..3),
            prop::collection::vec(target(), 0..2)
        )
            .prop_map(|(a, b)| Entry::List(a, b)),
        target().prop_map(Entry::Include),
        prop::collection::vec(target(), 0..3).prop_map(Entry::Region),
    ];
    (
        prop::collection::vec(target(), 0..3),
        prop::collection::vec(entry, 1..7),
    )
}

fn graph_grammar(top: &[Option<usize>], entries: &[Entry]) -> Grammar {
    let n = entries.len();
    let inc = |t: &Option<usize>| match t {
        None => Rule::include_self(),
        Some(i) => Rule::include(format!("e{}", i % n)),
    };
    let mut g = Grammar::new("source.g");
    for t in top {
        g = g.pattern(inc(t));
    }
    for (i, e) in entries.iter().enumerate() {
        let rule: Rule = match e {
            Entry::Match => Match::new("x").into(),
            Entry::List(a, b) => {
                Rule::patterns(a.iter().map(inc).chain([Rule::patterns(b.iter().map(inc))]))
            }
            Entry::Include(t) => inc(t),
            Entry::Region(list) => list
                .iter()
                .fold(BeginEnd::new("a", "b"), |r, t| r.pattern(inc(t)))
                .into(),
        };
        g = g.repository(format!("e{i}"), rule);
    }
    g
}

/// The reference: nodes are `$self` (0) and entries (`i + 1`); an edge runs
/// from a pattern list to each pattern list it includes, directly or inside
/// nested lists. Returns the edge matrix and whether any node reaches itself.
fn reference_cycle(top: &[Option<usize>], entries: &[Entry]) -> (Vec<Vec<bool>>, bool) {
    let n = entries.len();
    let node = |t: &Option<usize>| t.map_or(0, |i| i % n + 1);
    let is_list =
        |v: usize| v == 0 || matches!(entries[v - 1], Entry::List(..) | Entry::Include(_));
    let mut edge = vec![vec![false; n + 1]; n + 1];
    let mut add = |from: usize, targets: &[Option<usize>]| {
        for t in targets {
            let to = node(t);
            if is_list(to) {
                edge[from][to] = true;
            }
        }
    };
    add(0, top);
    for (i, e) in entries.iter().enumerate() {
        match e {
            Entry::List(a, b) => {
                add(i + 1, a);
                add(i + 1, b);
            }
            Entry::Include(t) => add(i + 1, std::slice::from_ref(t)),
            Entry::Match | Entry::Region(_) => {}
        }
    }
    let mut reach = edge.clone();
    for k in 0..=n {
        for i in 0..=n {
            for j in 0..=n {
                if reach[i][k] && reach[k][j] {
                    reach[i][j] = true;
                }
            }
        }
    }
    let cyclic = (0..=n).any(|v| reach[v][v]);
    (edge, cyclic)
}

// --- Scope names ----------------------------------------------------------------

/// The naming rules, read directly: scopes separated by single spaces;
/// segments separated by dots; a segment is letters, digits, `_`, `+`, `-`,
/// and substitutions, not starting with `+` or `-`; substitutions name
/// groups up to `groups`.
fn reference_scope_ok(text: &str, groups: u32) -> bool {
    !text.is_empty()
        && text.split(' ').all(|scope| {
            !scope.is_empty()
                && scope
                    .split('.')
                    .all(|segment| reference_segment_ok(segment, groups))
        })
}

fn reference_segment_ok(segment: &str, groups: u32) -> bool {
    let b = segment.as_bytes();
    if b.is_empty() {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' {
            let rest = &segment[i + 1..];
            let (inner, braced) = match rest.strip_prefix('{') {
                Some(r) => (r, true),
                None => (rest, false),
            };
            let digits: String = inner.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                return false;
            }
            let after = &inner[digits.len()..];
            let tail = if braced {
                if let Some(t) = after.strip_prefix(":/downcase}") {
                    t
                } else if let Some(t) = after.strip_prefix(":/upcase}") {
                    t
                } else {
                    return false;
                }
            } else {
                after
            };
            if digits
                .parse::<u64>()
                .map_or(true, |g| g > u64::from(groups))
            {
                return false;
            }
            i = segment.len() - tail.len();
        } else if b[i].is_ascii_alphanumeric()
            || b[i] == b'_'
            || ((b[i] == b'+' || b[i] == b'-') && i > 0)
        {
            i += 1;
        } else {
            return false;
        }
    }
    true
}

// --- Properties ------------------------------------------------------------------

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    #[test]
    fn prop_emitted_json_is_exactly_the_model(m in model(), order in prop::collection::vec(any::<u32>(), 1..8)) {
        let expected = grammar_json(&m);
        let grammar = to_grammar(&m, &order);
        let pretty = grammar.to_json().unwrap();
        let compact = grammar.to_json_compact().unwrap();
        prop_assert_eq!(parse(&pretty).unwrap(), expected.clone());
        prop_assert_eq!(parse(&compact).unwrap(), expected);
        prop_assert!(pretty.ends_with("}\n"), "missing final newline");
        let separators = ['\u{2028}', '\u{2029}'];
        prop_assert!(!compact.contains(separators), "raw line separator");
        prop_assert!(!compact.bytes().any(|b| b < 0x20), "raw control character");
    }

    #[test]
    fn prop_output_is_independent_of_insertion_order(
        m in model(),
        a in prop::collection::vec(any::<u32>(), 1..8),
        b in prop::collection::vec(any::<u32>(), 1..8),
    ) {
        for style in [Style::Pretty, Style::Compact] {
            let (mut x, mut y) = (String::new(), String::new());
            to_grammar(&m, &a).write_json(&mut x, style).unwrap();
            to_grammar(&m, &b).write_json(&mut y, style).unwrap();
            prop_assert_eq!(x, y);
        }
    }

    #[test]
    fn prop_regex_trees_validate(r in regex_tree()) {
        let mut text = String::new();
        r.print(&mut text, &mut None);
        prop_assert_eq!(validate_regex(&text), Ok(r.groups()), "{}", text);
    }

    #[test]
    fn prop_removing_a_closing_paren_is_refused(r in regex_tree(), pick in any::<usize>()) {
        // Wrapped, so every tree has at least one parenthesis to remove.
        let r = R::Group("(?:", Box::new(r));
        let parens = r.count_parens();
        let mut text = String::new();
        r.print(&mut text, &mut Some(pick % parens));
        // Usually `UnclosedGroup`; the text after the missing `)` can
        // trip another check first (a capture inside a look-behind that
        // now extends further, a quantifier that now follows another).
        prop_assert!(validate_regex(&text).is_err(), "{}", text);
    }

    #[test]
    fn prop_checker_never_panics(
        text in "[()\\[\\]{}|*+?^$.\\\\a-z0-9:<>=!#&,\\- \nkxpPuG_]{0,48}",
    ) {
        match validate_regex(&text) {
            Ok(_) => {}
            Err(e) => {
                prop_assert!(e.offset <= text.len());
                prop_assert!(text.is_char_boundary(e.offset));
            }
        }
        // The same text as an `end` pattern, where back-references name
        // `begin` groups.
        let _ = Grammar::new("source.x").pattern(BeginEnd::new("(a)(b)", text.clone())).validate();
    }

    #[test]
    fn prop_checker_offsets_on_any_text(text in any::<String>()) {
        if let Err(e) = validate_regex(&text) {
            prop_assert!(e.offset <= text.len() && text.is_char_boundary(e.offset));
        }
    }

    #[test]
    fn prop_escaped_text_is_always_accepted(t in text(), u in text()) {
        // Concatenations of escaped text are escaped text too.
        let escaped = escape_regex(&t) + &escape_regex(&u);
        prop_assert_eq!(validate_regex(&escaped), Ok(0));
        let x = format!("(?x){escaped}");
        prop_assert_eq!(validate_regex(&x), Ok(0));
        if !escaped.is_empty() {
            let g = Grammar::new("source.x").pattern(BeginEnd::new("(q)", escaped.clone()));
            prop_assert_eq!(g.validate(), Ok(()));
        }
    }

    #[test]
    fn prop_include_cycles_match_the_reference((top, entries) in graph()) {
        let (edge, cyclic) = reference_cycle(&top, &entries);
        let result = graph_grammar(&top, &entries).validate();
        match result {
            Ok(()) => prop_assert!(!cyclic),
            Err(Error::IncludeCycle { cycle }) => {
                prop_assert!(cyclic);
                prop_assert_eq!(cycle.first(), cycle.last());
                let node = |name: &String| -> usize {
                    if name == "$self" { 0 } else { name[2..].parse::<usize>().unwrap() + 1 }
                };
                for pair in cycle.windows(2) {
                    prop_assert!(edge[node(&pair[0])][node(&pair[1])], "{:?}", cycle);
                }
            }
            Err(other) => prop_assert!(false, "unexpected {other:?}"),
        }
    }

    #[test]
    fn prop_scope_names_match_the_reference(
        text in "([a-c1_+.$ -]|\\$1|\\$2|\\$\\{1:/upcase\\}|\\$\\{0:/lower\\}){0,10}",
    ) {
        let accepted = Grammar::new("source.x").pattern(Match::new("(a)").scope(text.clone())).validate();
        let expected = reference_scope_ok(&text, 1);
        prop_assert_eq!(accepted.is_ok(), expected, "{:?}: {:?}", text, accepted);
        if let Err(e) = accepted {
            let kind_ok = matches!(e, Error::InvalidScope { .. } | Error::CaptureOutOfRange { .. });
            prop_assert!(kind_ok, "unexpected error kind");
        }
    }
}

/// The generators reach what the properties are about.
#[test]
fn test_generators_are_not_vacuous() {
    use proptest::{strategy::ValueTree, test_runner::TestRunner};
    let mut runner = TestRunner::deterministic();
    let mut saw_region = false;
    let mut saw_capture_patterns = false;
    let mut saw_hostile = false;
    for _ in 0..300 {
        let m = model().new_tree(&mut runner).unwrap().current();
        let json = to_grammar(&m, &[0]).to_json_compact().unwrap();
        saw_region |= json.contains("\"begin\"");
        saw_capture_patterns |= json.contains("\"captures\":{\"")
            && json.contains("\"patterns\":[{\"include\":\"source.other\"}]");
        saw_hostile |= json.contains("\\u2028") || json.contains("\\u0000");
    }
    assert!(saw_region && saw_capture_patterns && saw_hostile);
    let mut cyclic = 0;
    for _ in 0..300 {
        let (top, entries) = graph().new_tree(&mut runner).unwrap().current();
        cyclic += usize::from(reference_cycle(&top, &entries).1);
    }
    assert!(cyclic > 30 && cyclic < 270, "{cyclic}");
}
