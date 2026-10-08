//! The Tier-1 path: describe a small language's highlighting, call one
//! method, get `tmLanguage.json`.
//!
//! `cargo run --example calc > calc.tmLanguage.json`

use textmate_lang::{BeginEnd, Grammar, Match, Rule, escape_regex};

fn main() -> Result<(), textmate_lang::Error> {
    // Keyword and operator lists usually come from data; escape them.
    let keywords = ["let", "fn", "if", "else", "return"].join("|");
    let operators: Vec<String> = ["==", "!=", "<=", ">=", "+", "-", "*", "/", "="]
        .iter()
        .map(|op| escape_regex(op))
        .collect();

    let json = Grammar::new("source.calc")
        .name("Calc")
        .file_type("calc")
        .pattern(Rule::include("comments"))
        .pattern(Rule::include("strings"))
        .pattern(Rule::include("expression"))
        .repository(
            "comments",
            BeginEnd::new("#", "$")
                .scope("comment.line.number-sign.calc")
                .begin_capture(0, "punctuation.definition.comment.calc"),
        )
        .repository(
            "strings",
            BeginEnd::new("\"", "\"")
                .scope("string.quoted.double.calc")
                .pattern(Match::new(r"\\.").scope("constant.character.escape.calc")),
        )
        .repository(
            "expression",
            Rule::patterns([
                Match::new(format!(r"\b(?:{keywords})\b"))
                    .scope("keyword.control.calc")
                    .into(),
                Match::new(r"\b(fn)\s+([a-z_]\w*)")
                    .capture(1, "storage.type.function.calc")
                    .capture(2, "entity.name.function.calc")
                    .into(),
                Match::new(r"\b\d+(?:\.\d+)?\b")
                    .scope("constant.numeric.calc")
                    .into(),
                Match::new(operators.join("|"))
                    .scope("keyword.operator.calc")
                    .into(),
                // Parentheses nest: the region includes the list it is in.
                BeginEnd::new(r"\(", r"\)")
                    .scope("meta.group.calc")
                    .pattern(Rule::include("expression"))
                    .into(),
            ]),
        )
        .to_json()?;

    print!("{json}");
    Ok(())
}
