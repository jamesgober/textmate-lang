//! What validation reports: each mistake is located by a JSON Pointer into
//! the output it would have produced, and regex errors carry a byte offset.
//!
//! `cargo run --example errors`

use textmate_lang::{BeginEnd, Grammar, Match, Rule};

fn main() {
    let mistakes = [
        Grammar::new("source.demo").pattern(Match::new(r"\b(if|else\b")),
        Grammar::new("source.demo").pattern(Match::new("x{2}?")),
        Grammar::new("source.demo").pattern(BeginEnd::new("(['\"])", r"\2")),
        Grammar::new("source.demo").pattern(Match::new("(a)").capture(3, "x.y")),
        Grammar::new("source.demo").pattern(Match::new("a").scope("keyword..demo")),
        Grammar::new("source.demo").pattern(Rule::include("strings")),
        Grammar::new("source.demo")
            .repository("a", Rule::patterns([Rule::include("b")]))
            .repository("b", Rule::patterns([Rule::include("a")])),
        Grammar::new("source.demo").injection_selector("L:source.js -"),
    ];
    for grammar in mistakes {
        match grammar.to_json() {
            Ok(_) => println!("accepted"),
            Err(error) => println!("{error}"),
        }
    }
}
