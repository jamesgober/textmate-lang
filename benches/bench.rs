//! Benchmarks: validation and emission of large grammars, and the regex
//! checker on long patterns.
//!
//! `cargo bench --bench bench`

#![allow(clippy::unwrap_used, missing_docs)]

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use textmate_lang::{BeginEnd, Grammar, Match, Rule, Style, escape_regex, validate_regex};

/// A grammar with `n` repository entries, every one included from the top
/// level: alternately a keyword match with captures and a string-like region
/// with nested patterns and captures.
fn large(n: usize) -> Grammar {
    let mut g = Grammar::new("source.large")
        .name("Large")
        .file_type("large");
    for i in 0..n {
        let name = format!("entry-{i}");
        g = g.pattern(Rule::include(name.clone()));
        g = if i % 2 == 0 {
            g.repository(
                name,
                Match::new(format!(r"\b(kw{i})\s*(\()(?=\w)"))
                    .scope("keyword.other.large")
                    .capture(1, "entity.name.function.large")
                    .capture(2, "punctuation.paren.large"),
            )
        } else {
            g.repository(
                name,
                BeginEnd::new(r#"(r?)(")"#, r#"""#)
                    .scope("string.quoted.double.large")
                    .begin_capture(2, "punctuation.definition.string.begin.large")
                    .end_capture(0, "punctuation.definition.string.end.large")
                    .pattern(
                        Match::new(r"\\(?:[nrt\\]|x\h{2}|u\{\h{1,6}\})")
                            .scope("constant.character.escape.large"),
                    )
                    .pattern(Rule::include(format!("entry-{}", i - 1))),
            )
        };
    }
    g
}

fn bench_emission(c: &mut Criterion) {
    let n = 100_000;
    let grammar = large(n);
    let bytes = grammar.to_json().unwrap().len();
    let mut group = c.benchmark_group("large_100k");
    group.sample_size(10);
    group.throughput(Throughput::Bytes(bytes as u64));
    group.bench_function("validate", |b| {
        b.iter(|| black_box(&grammar).validate().unwrap())
    });
    let mut out = String::with_capacity(bytes + 1024);
    group.bench_function("to_json_pretty", |b| {
        b.iter(|| {
            out.clear();
            black_box(&grammar)
                .write_json(&mut out, Style::Pretty)
                .unwrap();
            out.len()
        })
    });
    group.bench_function("to_json_compact", |b| {
        b.iter(|| {
            out.clear();
            black_box(&grammar)
                .write_json(&mut out, Style::Compact)
                .unwrap();
            out.len()
        })
    });
    group.finish();

    // The size of a typical hand-written language grammar.
    let grammar = large(300);
    let mut group = c.benchmark_group("typical_300");
    group.throughput(Throughput::Bytes(grammar.to_json().unwrap().len() as u64));
    group.bench_function("to_json_pretty", |b| {
        b.iter(|| black_box(&grammar).to_json().unwrap())
    });
    group.finish();
}

fn bench_regex(c: &mut Criterion) {
    // An alternation of 100,000 keywords, about 1.3 MiB.
    let words: Vec<String> = (0..100_000).map(|i| format!("keyword{i:06}")).collect();
    let keywords = format!(r"\b(?:{})\b", words.join("|"));
    // Mixed constructs: classes, groups, look-around, quantifiers, properties.
    let mixed =
        r#"(?<!\w)(?:0[xX]\h+|\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)(?=[^\p{L}_]|$)|"(?:[^"\\]|\\.)*"|"#
            .repeat(5_000);
    let mixed = mixed.trim_end_matches('|');
    let mut text = String::new();
    for i in 0..100_000 {
        text.push_str("a.b+c*");
        text.push_str(&i.to_string());
        text.push('\\');
    }

    let mut group = c.benchmark_group("regex");
    group.throughput(Throughput::Bytes(keywords.len() as u64));
    group.bench_function("validate_keywords", |b| {
        b.iter(|| validate_regex(black_box(&keywords)).unwrap())
    });
    group.throughput(Throughput::Bytes(mixed.len() as u64));
    group.bench_function("validate_mixed", |b| {
        b.iter(|| validate_regex(black_box(mixed)).unwrap())
    });
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("escape", |b| b.iter(|| escape_regex(black_box(&text))));
    group.finish();
}

criterion_group!(benches, bench_emission, bench_regex);
criterion_main!(benches);
