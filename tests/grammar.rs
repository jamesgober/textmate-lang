//! Integration tests: exact output, every error, include cycles, limits, and
//! scale.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use textmate_lang::{
    BeginEnd, BeginWhile, Capture, Error, Grammar, MAX_RULE_DEPTH, Match, RegexError,
    RegexErrorKind, Rule, Style,
};

fn demo() -> Grammar {
    Grammar::new("source.demo")
        .name("Demo")
        .file_type("demo")
        .file_type("dm")
        .first_line_match(r"^#!.*\bdemo\b")
        .pattern(Rule::include("comments"))
        .pattern(Rule::include("strings"))
        .pattern(
            Match::new(r"\b(fn)\s+([a-z_]\w*)")
                .capture(2, "entity.name.function.demo")
                .capture(1, "storage.type.function.demo"),
        )
        .repository(
            "strings",
            BeginEnd::new("\"", "\"")
                .scope("string.quoted.double.demo")
                .begin_capture(0, "punctuation.definition.string.begin.demo")
                .end_capture(0, "punctuation.definition.string.end.demo")
                .pattern(Match::new(r"\\.").scope("constant.character.escape.demo")),
        )
        .repository(
            "comments",
            Rule::patterns([
                Match::new("//.*$")
                    .scope("comment.line.double-slash.demo")
                    .into(),
                BeginEnd::new(r"/\*", r"\*/")
                    .scope("comment.block.demo")
                    .apply_end_pattern_last(true)
                    .into(),
            ]),
        )
}

const DEMO_PRETTY: &str = r##"{
  "name": "Demo",
  "scopeName": "source.demo",
  "fileTypes": [
    "demo",
    "dm"
  ],
  "firstLineMatch": "^#!.*\\bdemo\\b",
  "patterns": [
    {
      "include": "#comments"
    },
    {
      "include": "#strings"
    },
    {
      "match": "\\b(fn)\\s+([a-z_]\\w*)",
      "captures": {
        "1": {
          "name": "storage.type.function.demo"
        },
        "2": {
          "name": "entity.name.function.demo"
        }
      }
    }
  ],
  "repository": {
    "comments": {
      "patterns": [
        {
          "name": "comment.line.double-slash.demo",
          "match": "//.*$"
        },
        {
          "name": "comment.block.demo",
          "begin": "/\\*",
          "end": "\\*/",
          "applyEndPatternLast": 1
        }
      ]
    },
    "strings": {
      "name": "string.quoted.double.demo",
      "begin": "\"",
      "beginCaptures": {
        "0": {
          "name": "punctuation.definition.string.begin.demo"
        }
      },
      "end": "\"",
      "endCaptures": {
        "0": {
          "name": "punctuation.definition.string.end.demo"
        }
      },
      "patterns": [
        {
          "name": "constant.character.escape.demo",
          "match": "\\\\."
        }
      ]
    }
  }
}
"##;

#[test]
fn test_pretty_output_is_exact() {
    assert_eq!(demo().to_json().unwrap(), DEMO_PRETTY);
}

#[test]
fn test_compact_output_is_pretty_without_whitespace() {
    let compact = demo().to_json_compact().unwrap();
    // No string in the demo contains a space or a newline, so removing all
    // layout whitespace from the pretty form gives the compact form.
    let stripped: String = DEMO_PRETTY.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(compact, stripped);
    assert!(!compact.contains('\n'));
}

#[test]
fn test_output_does_not_depend_on_insertion_order() {
    let a = Grammar::new("source.x")
        .repository("b", Match::new("b"))
        .repository(
            "a",
            Match::new("(a)(b)").capture(2, "two").capture(1, "one"),
        )
        .injection("R:source.y", Rule::include("a"))
        .injection("L:source.y", Rule::include("b"));
    let b = Grammar::new("source.x")
        .injection("L:source.y", Rule::include("b"))
        .repository(
            "a",
            Match::new("(a)(b)").capture(1, "one").capture(2, "two"),
        )
        .injection("R:source.y", Rule::include("a"))
        .repository("b", Match::new("b"));
    for style in [Style::Pretty, Style::Compact] {
        let (mut x, mut y) = (String::new(), String::new());
        a.write_json(&mut x, style).unwrap();
        b.write_json(&mut y, style).unwrap();
        assert_eq!(x, y);
    }
    let json = a.to_json_compact().unwrap();
    assert!(json.contains(
        r##""injections":{"L:source.y":{"include":"#b"},"R:source.y":{"include":"#a"}}"##
    ));
}

#[test]
fn test_every_rule_kind_and_field() {
    let json = Grammar::new("text.demo")
        .injection_selector("L:text.html -comment")
        .pattern(
            BeginWhile::new(r"^(>)\s?", r"^(>)\s?")
                .scope("markup.quote.demo")
                .content_scope("meta.quote.content.demo")
                .begin_capture(1, "punctuation.begin.demo")
                .while_capture(1, "punctuation.while.demo")
                .pattern(Rule::include_self()),
        )
        .pattern(
            BeginEnd::new("(<)(\\w+)", "(</)(\\2)(>)")
                .capture(1, "punctuation.tag.demo")
                .pattern(Rule::include_base())
                .pattern(Rule::include_scope("source.js"))
                .pattern(Rule::include_scope_entry("source.css", "property"))
                .pattern(Rule::include_scope_entry("text.demo", "inline")),
        )
        .repository(
            "inline",
            Match::new(r"(\*)(\w+)(\*)").capture(
                0,
                Capture::new()
                    .scope("markup.bold.demo")
                    .pattern(Rule::include("inline")),
            ),
        )
        .to_json_compact()
        .unwrap();
    for needle in [
        r#""injectionSelector":"L:text.html -comment""#,
        r#""name":"markup.quote.demo","contentName":"meta.quote.content.demo","begin":"^(>)\\s?","beginCaptures":{"1":{"name":"punctuation.begin.demo"}},"while":"^(>)\\s?","whileCaptures":{"1":{"name":"punctuation.while.demo"}},"patterns":[{"include":"$self"}]"#,
        r#"{"begin":"(<)(\\w+)","end":"(</)(\\2)(>)","captures":{"1":{"name":"punctuation.tag.demo"}},"patterns":[{"include":"$base"},{"include":"source.js"},{"include":"source.css#property"},{"include":"text.demo#inline"}]}"#,
        r##""inline":{"match":"(\\*)(\\w+)(\\*)","captures":{"0":{"name":"markup.bold.demo","patterns":[{"include":"#inline"}]}}}"##,
    ] {
        assert!(json.contains(needle), "missing {needle}\nin {json}");
    }
}

#[test]
fn test_hostile_strings_are_escaped() {
    let name = "x\",\"scopeName\":\"evil\"}\u{0}\u{1f}\u{2028}\u{2029}\\";
    let json = Grammar::new("source.x")
        .name(name)
        .file_type("\"]}")
        .to_json_compact()
        .unwrap();
    assert!(json.starts_with(r#"{"name":"x\",\"scopeName\":\"evil\"}\u0000\u001f"#));
    assert!(!json.contains('\u{2028}') && !json.contains('\u{2029}'));
    assert!(json.contains(r#""fileTypes":["\"]}"]"#));
    assert_eq!(json.matches("\"scopeName\":").count(), 1);
}

fn err(grammar: Grammar) -> Error {
    grammar.validate().expect_err("grammar should be refused")
}

#[test]
fn test_regex_errors_point_into_the_output() {
    let e = err(Grammar::new("source.x").repository(
        "r",
        BeginEnd::new("a", "b")
            .pattern(Match::new("ok"))
            .pattern(Match::new("x[")),
    ));
    assert_eq!(
        e,
        Error::InvalidRegex {
            path: "/repository/r/patterns/1/match".into(),
            error: RegexError {
                offset: 1,
                kind: RegexErrorKind::UnclosedClass
            },
        }
    );
    let e = err(Grammar::new("source.x").first_line_match("+"));
    assert!(matches!(e, Error::InvalidRegex { ref path, .. } if path == "/firstLineMatch"));
    let e = err(Grammar::new("source.x").pattern(Match::new("")));
    assert_eq!(
        e,
        Error::EmptyRegex {
            path: "/patterns/0/match".into()
        }
    );
    let e = err(Grammar::new("source.x").pattern(BeginEnd::new("a", "")));
    assert_eq!(
        e,
        Error::EmptyRegex {
            path: "/patterns/0/end".into()
        }
    );
    let e = err(Grammar::new("source.x").pattern(BeginWhile::new("a", "")));
    assert_eq!(
        e,
        Error::EmptyRegex {
            path: "/patterns/0/while".into()
        }
    );
}

#[test]
fn test_end_backreferences_name_begin_groups() {
    let ok = Grammar::new("source.x").pattern(BeginEnd::new(r"(['`])(\w)", r"\2\1"));
    assert_eq!(ok.validate(), Ok(()));
    let e = err(Grammar::new("source.x").pattern(BeginEnd::new(r"(a)", r"x\2")));
    assert_eq!(
        e,
        Error::InvalidRegex {
            path: "/patterns/0/end".into(),
            error: RegexError {
                offset: 1,
                kind: RegexErrorKind::BackreferenceOutOfRange {
                    group: 2,
                    groups: 1
                },
            },
        }
    );
    let e = err(Grammar::new("source.x").pattern(BeginWhile::new("a", r"\1")));
    assert!(matches!(e, Error::InvalidRegex { ref path, .. } if path == "/patterns/0/while"));
    // A literal backslash before a digit would be misread by VS Code.
    let e = err(Grammar::new("source.x").pattern(BeginEnd::new("(a)", r"\\1")));
    assert!(matches!(
        e,
        Error::InvalidRegex {
            error: RegexError {
                kind: RegexErrorKind::BackslashBeforeDigit,
                ..
            },
            ..
        }
    ));
    // Named groups in `begin` and numbered references in `end` coexist.
    let ok = Grammar::new("source.x").pattern(BeginEnd::new(r"(?<q>')", r"\1"));
    assert_eq!(ok.validate(), Ok(()));
}

#[test]
fn test_scope_errors() {
    let e = err(Grammar::new("source x"));
    assert_eq!(
        e,
        Error::InvalidScope {
            path: "/scopeName".into(),
            scope: "source x".into(),
            offset: 6,
            reason: "a single scope cannot contain spaces",
        }
    );
    let e = err(Grammar::new("source.x").pattern(Match::new("a").scope("bad..scope")));
    assert!(
        matches!(e, Error::InvalidScope { ref path, offset: 4, .. } if path == "/patterns/0/name")
    );
    let e = err(Grammar::new("source.x").pattern(Match::new("(a)").capture(1, "x y.")));
    assert!(
        matches!(e, Error::InvalidScope { ref path, offset: 3, .. } if path == "/patterns/0/captures/1/name")
    );
    let e = err(Grammar::new("source.x").pattern(Match::new("(a)").scope("k.$2")));
    assert_eq!(
        e,
        Error::CaptureOutOfRange {
            path: "/patterns/0/name".into(),
            group: 2,
            groups: 1
        }
    );
    let e = err(Grammar::new("source.x").pattern(BeginEnd::new("a", "(b)").content_scope("c.$1")));
    assert_eq!(
        e,
        Error::CaptureOutOfRange {
            path: "/patterns/0/contentName".into(),
            group: 1,
            groups: 0
        }
    );
    let e = err(Grammar::new("source.x").pattern(Rule::include_scope("source.$1")));
    assert!(matches!(e, Error::InvalidScope { ref path, .. } if path == "/patterns/0/include"));
}

#[test]
fn test_capture_errors() {
    let e =
        err(Grammar::new("source.x").pattern(Match::new("(a)").capture(1, "a").capture(1, "b")));
    assert_eq!(
        e,
        Error::DuplicateCapture {
            path: "/patterns/0/captures".into(),
            group: 1
        }
    );
    let e = err(Grammar::new("source.x").pattern(BeginEnd::new("(a)", "b").begin_capture(2, "x")));
    assert_eq!(
        e,
        Error::CaptureOutOfRange {
            path: "/patterns/0/beginCaptures/2".into(),
            group: 2,
            groups: 1
        }
    );
    // Shared captures must exist on every side they apply to.
    let e = err(Grammar::new("source.x").pattern(BeginEnd::new("(a)", "b").capture(1, "x")));
    assert_eq!(
        e,
        Error::CaptureOutOfRange {
            path: "/patterns/0/captures/1".into(),
            group: 1,
            groups: 0
        }
    );
    let ok = Grammar::new("source.x").pattern(
        BeginEnd::new("(a)", "b")
            .end_capture(0, "e")
            .capture(1, "x"),
    );
    assert_eq!(ok.validate(), Ok(()));
    let e = err(Grammar::new("source.x").pattern(
        BeginWhile::new("a", "b")
            .begin_capture(0, "x")
            .while_capture(0, "y")
            .capture(0, "z"),
    ));
    assert_eq!(
        e,
        Error::ShadowedCaptures {
            path: "/patterns/0".into()
        }
    );
}

#[test]
fn test_name_and_selector_errors() {
    let e = err(Grammar::new("source.x").repository("has space", Match::new("a")));
    assert!(matches!(e, Error::InvalidName { ref path, .. } if path == "/repository/has space"));
    let e = err(Grammar::new("source.x").file_type("rs").file_type(""));
    assert!(matches!(e, Error::InvalidName { ref path, .. } if path == "/fileTypes/1"));
    let e = err(Grammar::new("source.x").pattern(Rule::include("")));
    assert!(matches!(e, Error::InvalidName { ref path, .. } if path == "/patterns/0/include"));
    let e = err(Grammar::new("source.x").pattern(Rule::include_scope_entry("source.y", "a#b")));
    assert!(matches!(e, Error::InvalidName { .. }));
    let e = err(Grammar::new("source.x")
        .repository("a", Match::new("a"))
        .repository("a", Match::new("b")));
    assert_eq!(e, Error::DuplicateRepository { name: "a".into() });
    let e = err(Grammar::new("source.x")
        .injection("L:a", Rule::include_self())
        .injection("L:a", Rule::include_self()));
    assert_eq!(
        e,
        Error::DuplicateInjection {
            selector: "L:a".into()
        }
    );
    let e = err(Grammar::new("source.x").injection_selector("L:source.y,"));
    assert!(
        matches!(e, Error::InvalidSelector { ref path, offset: 11, .. } if path == "/injectionSelector")
    );
    let e = err(Grammar::new("source.x").injection("a/b", Rule::include_self()));
    assert!(
        matches!(e, Error::InvalidSelector { ref path, offset: 1, .. } if path == "/injections/a~1b")
    );
}

#[test]
fn test_undefined_includes() {
    let e =
        err(Grammar::new("source.x")
            .pattern(BeginEnd::new("a", "b").pattern(Rule::include("missing"))));
    assert_eq!(
        e,
        Error::UndefinedInclude {
            path: "/patterns/0/patterns/0/include".into(),
            name: "missing".into()
        }
    );
    // An include of this grammar's own scope must resolve too.
    let e = err(Grammar::new("source.x").pattern(Rule::include_scope_entry("source.x", "nope")));
    assert!(matches!(e, Error::UndefinedInclude { .. }));
    // Another grammar's entries cannot be checked here.
    let ok = Grammar::new("source.x").pattern(Rule::include_scope_entry("source.y", "nope"));
    assert_eq!(ok.validate(), Ok(()));
}

fn cycle(grammar: Grammar) -> Vec<String> {
    match grammar.validate() {
        Err(Error::IncludeCycle { cycle }) => cycle,
        other => panic!("expected a cycle, got {other:?}"),
    }
}

#[test]
fn test_include_cycles_that_consume_nothing_are_refused() {
    let g = Grammar::new("source.x")
        .repository("a", Rule::patterns([Rule::include("b")]))
        .repository(
            "b",
            Rule::patterns([Match::new("x").into(), Rule::include("a")]),
        );
    assert_eq!(cycle(g), ["#a", "#b", "#a"]);

    let g = Grammar::new("source.x").pattern(Rule::include_self());
    assert_eq!(cycle(g), ["$self", "$self"]);

    let g = Grammar::new("source.x").pattern(Rule::include_base());
    assert_eq!(cycle(g), ["$self", "$self"]);

    let g = Grammar::new("source.x").repository("a", Rule::include("a"));
    assert_eq!(cycle(g), ["#a", "#a"]);

    let g = Grammar::new("source.x")
        .pattern(Rule::patterns([Rule::patterns([Rule::include("a")])]))
        .repository("a", Rule::include_scope("source.x"));
    assert_eq!(cycle(g), ["$self", "#a", "$self"]);

    let g = Grammar::new("source.x")
        .repository("a", Rule::include_scope_entry("source.x", "b"))
        .repository("b", Rule::patterns([Rule::include("a")]));
    assert_eq!(cycle(g), ["#a", "#b", "#a"]);
}

#[test]
fn test_recursion_through_regions_and_captures_is_fine() {
    // Nested parentheses: the region includes the entry that contains it.
    let g = Grammar::new("source.x")
        .pattern(Rule::include("expr"))
        .repository(
            "expr",
            Rule::patterns([Rule::include("parens"), Match::new(r"\d+").into()]),
        )
        .repository(
            "parens",
            BeginEnd::new(r"\(", r"\)").pattern(Rule::include("expr")),
        );
    assert_eq!(g.validate(), Ok(()));

    let g =
        Grammar::new("source.x").pattern(BeginEnd::new(r"\{", r"\}").pattern(Rule::include_self()));
    assert_eq!(g.validate(), Ok(()));

    let g = Grammar::new("source.x").repository(
        "m",
        Match::new("(a+)").capture(1, Capture::new().pattern(Rule::include("m"))),
    );
    assert_eq!(g.validate(), Ok(()));

    // A list including a match entry: the match does not expand.
    let g = Grammar::new("source.x")
        .pattern(Rule::include("list"))
        .repository(
            "list",
            Rule::patterns([Rule::include("word"), Rule::include("word")]),
        )
        .repository("word", Match::new(r"\w+"));
    assert_eq!(g.validate(), Ok(()));

    // External includes are not followed.
    let g = Grammar::new("source.x").pattern(Rule::include_scope("source.y"));
    assert_eq!(g.validate(), Ok(()));
}

#[test]
fn test_depth_limit() {
    let nest = |levels: usize| {
        let mut rule: Rule = Match::new("x").into();
        for _ in 1..levels {
            rule = BeginEnd::new("a", "b").pattern(rule).into();
        }
        Grammar::new("source.x").pattern(rule)
    };
    assert_eq!(nest(MAX_RULE_DEPTH).validate(), Ok(()));
    assert!(nest(MAX_RULE_DEPTH).to_json().is_ok());
    let e = nest(MAX_RULE_DEPTH + 1).validate().unwrap_err();
    let Error::TooDeep { path } = e else {
        panic!("{e:?}")
    };
    assert_eq!(
        path,
        "/patterns/0".to_string() + &"/patterns/0".repeat(MAX_RULE_DEPTH)
    );
    // Far past the limit: refused, not a stack overflow.
    assert!(matches!(
        nest(500_000).validate(),
        Err(Error::TooDeep { .. })
    ));
}

#[test]
fn test_write_json_appends_nothing_on_error() {
    let mut out = String::from("keep");
    let bad = Grammar::new("source.x").pattern(Match::new("("));
    assert!(bad.write_json(&mut out, Style::Pretty).is_err());
    assert_eq!(out, "keep");
    Grammar::new("source.x")
        .write_json(&mut out, Style::Compact)
        .unwrap();
    assert_eq!(out, r#"keep{"scopeName":"source.x","patterns":[]}"#);
}

#[test]
fn test_errors_display() {
    let e = err(Grammar::new("source.x").pattern(Match::new("a]")));
    assert_eq!(
        e.to_string(),
        "invalid regular expression at `/patterns/0/match`, at byte 1: unbalanced `]`; write `\\]` for the character"
    );
}

#[test]
fn test_scale_100k_entries() {
    let n = 100_000;
    let mut g = Grammar::new("source.big");
    for i in 0..n {
        let name = format!("r{i}");
        g = g.pattern(Rule::include(name.clone())).repository(
            name,
            Match::new(r"\b(kw)_(\d+)\b")
                .scope("keyword.big")
                .capture(2, "constant.numeric.big"),
        );
    }
    let json = g.to_json_compact().unwrap();
    assert_eq!(json.matches(r##""include":"#r"##).count(), n);
    // A chain of n list entries, then closed into a cycle.
    let mut chain = Grammar::new("source.chain").pattern(Rule::include("c0"));
    for i in 0..n - 1 {
        chain = chain.repository(
            format!("c{i}"),
            Rule::patterns([Rule::include(format!("c{}", i + 1))]),
        );
    }
    let open = chain
        .clone()
        .repository(format!("c{}", n - 1), Match::new("end"));
    assert_eq!(open.validate(), Ok(()));
    let closed = chain.repository(format!("c{}", n - 1), Rule::include("c0"));
    assert_eq!(cycle(closed).len(), n + 1);
}

#[test]
fn test_scale_long_patterns() {
    let words: Vec<String> = (0..100_000).map(|i| format!("w{i}")).collect();
    let pattern = format!(r"\b(?:{})\b", words.join("|"));
    assert_eq!(textmate_lang::validate_regex(&pattern), Ok(0));
    let json = Grammar::new("source.x")
        .pattern(Match::new(pattern))
        .to_json()
        .unwrap();
    assert!(json.len() > 600_000);
}
