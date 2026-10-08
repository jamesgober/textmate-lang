//! Emitting `tmLanguage.json`.
//!
//! A small JSON writer of our own, and the emitter that drives it from an
//! explicit work stack. Every string that reaches the output — names,
//! patterns, scopes, selectors, file types — goes through [`json_string`],
//! whatever validation already established about it, so no input can add,
//! remove, or reshape a JSON value.
//!
//! Key order is fixed. The grammar object's keys come in the order `name`,
//! `scopeName`, `fileTypes`, `firstLineMatch`, `injectionSelector`,
//! `patterns`, `repository`, `injections`; a rule's in the order `include`,
//! `name`, `contentName`, `match`, `begin`, `beginCaptures`, `end`,
//! `endCaptures`, `while`, `whileCaptures`, `captures`,
//! `applyEndPatternLast`, `patterns`. Absent and empty fields are left out,
//! except the grammar's `patterns`, which is always written. Repository and
//! injection entries are sorted by name, captures by group number.

use alloc::{string::String, vec::Vec};

use crate::{
    Grammar,
    check::Checked,
    rule::{Capture, Captures, Include, Kind, Rule},
};

/// How [`Grammar::write_json`] lays out its output.
///
/// # Examples
///
/// ```
/// use textmate_lang::{Grammar, Style};
///
/// let grammar = Grammar::new("source.demo");
/// let mut out = String::new();
/// grammar.write_json(&mut out, Style::Compact)?;
/// assert_eq!(out, r#"{"scopeName":"source.demo","patterns":[]}"#);
///
/// out.clear();
/// grammar.write_json(&mut out, Style::Pretty)?;
/// assert_eq!(out, "{\n  \"scopeName\": \"source.demo\",\n  \"patterns\": []\n}\n");
/// # Ok::<(), textmate_lang::Error>(())
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Style {
    /// Two-space indentation, one member per line, `"key": value`, and a
    /// final newline: the layout of hand-written grammar files.
    #[default]
    Pretty,
    /// No whitespace at all: the smallest output, for embedding or
    /// shipping.
    Compact,
}

/// Appends a decimal number without going through `core::fmt`.
pub(crate) fn push_decimal(out: &mut String, mut value: u64) {
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    loop {
        at -= 1;
        // `value % 10` is below 10, so the cast is exact.
        digits[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for &d in digits.get(at..).unwrap_or(&[]) {
        out.push(char::from(d));
    }
}

/// Appends `parts`, concatenated, as one JSON string literal.
///
/// Escaped: `"`, `\`, every control character (`\n`, `\r`, `\t`, `\b`, `\f`,
/// or `\u00XX`), and U+2028 and U+2029, which are valid in JSON but end a
/// line in JavaScript, where grammars are often loaded. Everything else is
/// copied in bulk between escapes. The characters searched for are ASCII or
/// start with the byte `0xE2`, never a continuation byte, so every split is
/// on a character boundary.
pub(crate) fn json_string(out: &mut String, parts: &[&str]) {
    out.push('"');
    for text in parts {
        let bytes = text.as_bytes();
        let mut from = 0;
        let mut at = 0;
        while let Some(&b) = bytes.get(at) {
            let (escape, width): (&str, usize) = match b {
                b'"' => ("\\\"", 1),
                b'\\' => ("\\\\", 1),
                b'\n' => ("\\n", 1),
                b'\r' => ("\\r", 1),
                b'\t' => ("\\t", 1),
                0x08 => ("\\b", 1),
                0x0C => ("\\f", 1),
                0x00..=0x1F => {
                    out.push_str(text.get(from..at).unwrap_or(""));
                    out.push_str("\\u00");
                    out.push(hex(b >> 4));
                    out.push(hex(b & 0x0F));
                    at += 1;
                    from = at;
                    continue;
                }
                0xE2 => match bytes.get(at + 1..at + 3) {
                    Some([0x80, 0xA8]) => ("\\u2028", 3),
                    Some([0x80, 0xA9]) => ("\\u2029", 3),
                    _ => {
                        at += 1;
                        continue;
                    }
                },
                _ => {
                    at += 1;
                    continue;
                }
            };
            out.push_str(text.get(from..at).unwrap_or(""));
            out.push_str(escape);
            at += width;
            from = at;
        }
        out.push_str(text.get(from..).unwrap_or(""));
    }
    out.push('"');
}

fn hex(nibble: u8) -> char {
    char::from(b"0123456789abcdef"[usize::from(nibble & 0x0F)])
}

/// Layout state: the output, the style, the indentation level, and whether
/// the innermost open object or array is still empty.
struct Writer<'o> {
    out: &'o mut String,
    pretty: bool,
    level: usize,
    empty: bool,
}

impl Writer<'_> {
    fn newline(&mut self) {
        if self.pretty {
            self.out.push('\n');
            for _ in 0..self.level {
                self.out.push_str("  ");
            }
        }
    }

    fn open(&mut self, bracket: char) {
        self.out.push(bracket);
        self.level += 1;
        self.empty = true;
    }

    /// Closes the innermost object or array. Its parent is not empty: it
    /// holds the value just closed.
    fn close(&mut self, bracket: char) {
        self.level = self.level.saturating_sub(1);
        if !self.empty {
            self.newline();
        }
        self.out.push(bracket);
        self.empty = false;
    }

    /// Starts the next member or element.
    fn next(&mut self) {
        if !self.empty {
            self.out.push(',');
        }
        self.empty = false;
        self.newline();
    }

    fn key(&mut self, parts: &[&str]) {
        self.next();
        json_string(self.out, parts);
        self.out.push_str(if self.pretty { ": " } else { ":" });
    }

    fn string_member(&mut self, key: &str, value: &[&str]) {
        self.key(&[key]);
        json_string(self.out, value);
    }

    fn group_key(&mut self, group: u32) {
        self.next();
        self.out.push('"');
        push_decimal(self.out, u64::from(group));
        self.out.push_str(if self.pretty { "\": " } else { "\":" });
    }
}

/// One step of the emitter.
enum Task<'g> {
    Rule(&'g Rule),
    Capture(&'g Capture),
    /// `"key": "value"`, if there is a value.
    Str(&'static str, Option<&'g str>),
    /// `"key": [rules]`, if there are any.
    Rules(&'static str, &'g [Rule]),
    /// `"key": {captures}`, if there are any.
    Captures(&'static str, &'g Captures),
    /// `"applyEndPatternLast": 1`, if set.
    Flag(&'static str, bool),
    /// A key naming a repository entry or an injection selector.
    Key(&'g str),
    Group(u32),
    /// The separator before an array element.
    Element,
    Close(char),
}

/// Appends the validated grammar to `out`.
pub(crate) fn write(grammar: &Grammar, checked: &Checked<'_>, out: &mut String, pretty: bool) {
    out.reserve(256 + 160 * (grammar.patterns.len() + checked.repository.len()));
    let mut emitter = Emitter {
        w: Writer {
            out,
            pretty,
            level: 0,
            empty: true,
        },
        work: Vec::new(),
        sorted: Vec::new(),
    };
    emitter.grammar(grammar, checked);
    if pretty {
        emitter.w.out.push('\n');
    }
}

struct Emitter<'o, 'g> {
    w: Writer<'o>,
    work: Vec<Task<'g>>,
    /// Reused to sort each captures map.
    sorted: Vec<&'g (u32, Capture)>,
}

impl<'g> Emitter<'_, 'g> {
    fn grammar(&mut self, g: &'g Grammar, checked: &Checked<'g>) {
        self.w.open('{');
        if let Some(name) = &g.name {
            self.w.string_member("name", &[name]);
        }
        self.w.string_member("scopeName", &[&g.scope_name]);
        if !g.file_types.is_empty() {
            self.w.key(&["fileTypes"]);
            self.w.open('[');
            for file_type in &g.file_types {
                self.w.next();
                json_string(self.w.out, &[file_type]);
            }
            self.w.close(']');
        }
        if let Some(regex) = &g.first_line_match {
            self.w.string_member("firstLineMatch", &[regex]);
        }
        if let Some(selector) = &g.injection_selector {
            self.w.string_member("injectionSelector", &[selector]);
        }
        // `patterns` is always written: loaders and the tmLanguage schema
        // expect it.
        self.w.key(&["patterns"]);
        self.w.open('[');
        self.push_elements(&g.patterns);
        self.run();
        self.w.close(']');
        for (key, entries) in [
            ("repository", &checked.repository),
            ("injections", &checked.injections),
        ] {
            if entries.is_empty() {
                continue;
            }
            self.w.key(&[key]);
            self.w.open('{');
            self.work.push(Task::Close('}'));
            for (name, rule) in entries.iter().rev() {
                self.work.push(Task::Rule(rule));
                self.work.push(Task::Key(name));
            }
            self.run();
        }
        self.w.close('}');
    }

    /// Schedules `rules` as array elements, in order.
    fn push_elements(&mut self, rules: &'g [Rule]) {
        for rule in rules.iter().rev() {
            self.work.push(Task::Rule(rule));
            self.work.push(Task::Element);
        }
    }

    fn run(&mut self) {
        while let Some(task) = self.work.pop() {
            match task {
                Task::Rule(rule) => self.rule(rule),
                Task::Capture(capture) => {
                    self.w.open('{');
                    self.work.push(Task::Close('}'));
                    self.work.push(Task::Rules("patterns", &capture.patterns));
                    self.work.push(Task::Str("name", capture.scope.as_deref()));
                }
                Task::Str(key, Some(value)) => self.w.string_member(key, &[value]),
                Task::Str(_, None) => {}
                Task::Rules(key, rules) => {
                    if !rules.is_empty() {
                        self.w.key(&[key]);
                        self.w.open('[');
                        self.work.push(Task::Close(']'));
                        self.push_elements(rules);
                    }
                }
                Task::Captures(key, captures) => self.captures(key, captures),
                Task::Flag(key, true) => {
                    self.w.key(&[key]);
                    self.w.out.push('1');
                }
                Task::Flag(_, false) => {}
                Task::Key(name) => self.w.key(&[name]),
                Task::Group(group) => self.w.group_key(group),
                Task::Element => self.w.next(),
                Task::Close(bracket) => self.w.close(bracket),
            }
        }
    }

    /// Writes a rule's opening and schedules its fields. Fields are pushed
    /// in reverse key order, so they pop in key order.
    fn rule(&mut self, rule: &'g Rule) {
        self.w.open('{');
        self.work.push(Task::Close('}'));
        match &rule.0 {
            Kind::Include(include) => self.include(include),
            Kind::Patterns(rules) => self.work.push(Task::Rules("patterns", rules)),
            Kind::Match(m) => self.work.extend([
                Task::Captures("captures", &m.captures),
                Task::Str("match", Some(&m.regex)),
                Task::Str("name", m.scope.as_deref()),
            ]),
            Kind::BeginEnd(r) => self.work.extend([
                Task::Rules("patterns", &r.patterns),
                Task::Flag("applyEndPatternLast", r.apply_end_pattern_last),
                Task::Captures("captures", &r.captures),
                Task::Captures("endCaptures", &r.end_captures),
                Task::Str("end", Some(&r.end)),
                Task::Captures("beginCaptures", &r.begin_captures),
                Task::Str("begin", Some(&r.begin)),
                Task::Str("contentName", r.content_scope.as_deref()),
                Task::Str("name", r.scope.as_deref()),
            ]),
            Kind::BeginWhile(r) => self.work.extend([
                Task::Rules("patterns", &r.patterns),
                Task::Captures("captures", &r.captures),
                Task::Captures("whileCaptures", &r.while_captures),
                Task::Str("while", Some(&r.while_)),
                Task::Captures("beginCaptures", &r.begin_captures),
                Task::Str("begin", Some(&r.begin)),
                Task::Str("contentName", r.content_scope.as_deref()),
                Task::Str("name", r.scope.as_deref()),
            ]),
        }
    }

    /// `"include": "..."`, written in pieces so no string is built.
    fn include(&mut self, include: &'g Include) {
        let value: [&str; 3] = match include {
            Include::Repository(name) => ["#", name, ""],
            Include::SelfRef => ["$self", "", ""],
            Include::Base => ["$base", "", ""],
            Include::Scope(scope) => [scope, "", ""],
            Include::ScopeEntry(scope, name) => [scope, "#", name],
        };
        self.w.string_member("include", &value);
    }

    /// Writes a captures map's opening and schedules its entries in group
    /// order.
    fn captures(&mut self, key: &'static str, captures: &'g Captures) {
        if captures.is_empty() {
            return;
        }
        self.w.key(&[key]);
        self.w.open('{');
        self.work.push(Task::Close('}'));
        self.sorted.clear();
        self.sorted.extend(captures.iter());
        self.sorted.sort_unstable_by_key(|(group, _)| *group);
        for (group, capture) in self.sorted.drain(..).rev() {
            self.work.push(Task::Capture(capture));
            self.work.push(Task::Group(*group));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quoted(text: &str) -> String {
        let mut out = String::new();
        json_string(&mut out, &[text]);
        out
    }

    #[test]
    fn test_json_string_escapes() {
        assert_eq!(quoted("plain"), "\"plain\"");
        assert_eq!(quoted(""), "\"\"");
        assert_eq!(quoted(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(quoted("\n\r\t\u{8}\u{c}"), r#""\n\r\t\b\f""#);
        assert_eq!(quoted("\u{0}\u{1}\u{1f}"), r#""\u0000\u0001\u001f""#);
        assert_eq!(quoted("x\u{2028}y\u{2029}z"), r#""x\u2028y\u2029z""#);
        assert_eq!(quoted("é→\u{7f}/✓"), "\"é→\u{7f}/✓\"");
        let mut out = String::new();
        json_string(&mut out, &["a\"", "", "#b\n"]);
        assert_eq!(out, r##""a\"#b\n""##);
    }

    #[test]
    fn test_push_decimal() {
        for (value, text) in [
            (0, "0"),
            (7, "7"),
            (10, "10"),
            (u64::MAX, "18446744073709551615"),
        ] {
            let mut out = String::new();
            push_decimal(&mut out, value);
            assert_eq!(out, text);
        }
    }

    #[test]
    fn test_writer_layout() {
        let mut out = String::new();
        let mut w = Writer {
            out: &mut out,
            pretty: true,
            level: 0,
            empty: true,
        };
        w.open('{');
        w.key(&["a"]);
        w.open('[');
        w.close(']');
        w.key(&["b"]);
        w.open('[');
        w.next();
        w.out.push('1');
        w.close(']');
        w.close('}');
        assert_eq!(out, "{\n  \"a\": [],\n  \"b\": [\n    1\n  ]\n}");
    }
}
