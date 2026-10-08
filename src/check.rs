//! Validation of a whole grammar before it is emitted.
//!
//! Rules are visited from an explicit work stack, never by recursion, and
//! nesting is capped at [`MAX_RULE_DEPTH`]. The path of the rule being
//! checked is kept as a stack of segments and rendered as a JSON Pointer only
//! when an error is reported. Repository and injection entries are sorted
//! once, by name; includes are resolved by binary search, and the include
//! graph is searched for cycles with an iterative depth-first search, so the
//! whole check is `O(n log n)` in the size of the grammar.

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    Error, Grammar, RegexError, RegexErrorKind,
    regex::{self, Backrefs, Buffers, Info},
    rule::{Capture, Captures, Include, Kind, Rule, Text},
    scope::{self, Problem},
};

/// How deep rules may nest: regions inside regions, captures with patterns,
/// and pattern lists inside either. Includes do not count.
///
/// Editors build their rule tables recursively, so a much deeper grammar
/// could overflow their stacks; real grammars nest a handful of levels and
/// use repository entries for the rest.
///
/// # Examples
///
/// ```
/// use textmate_lang::{BeginEnd, Error, Grammar, MAX_RULE_DEPTH, Rule};
///
/// let mut rule: Rule = BeginEnd::new("a", "b").into();
/// for _ in 0..MAX_RULE_DEPTH {
///     rule = BeginEnd::new("a", "b").pattern(rule).into();
/// }
/// let err = Grammar::new("source.demo").pattern(rule).to_json().unwrap_err();
/// assert!(matches!(err, Error::TooDeep { .. }));
/// ```
pub const MAX_RULE_DEPTH: usize = 128;

/// Repository and injection entries in emission order (sorted by name).
pub(crate) struct Checked<'g> {
    pub(crate) repository: Vec<&'g (Text, Rule)>,
    pub(crate) injections: Vec<&'g (Text, Rule)>,
}

/// One step of a JSON Pointer.
#[derive(Clone, Copy, Debug)]
enum Seg<'g> {
    Key(&'static str),
    Name(&'g str),
    Index(usize),
    Group(u32),
}

enum Step<'g> {
    Rule(&'g Rule, usize),
    Enter(Seg<'g>),
    Leave,
}

/// Validates `grammar`, returning its entries in emission order.
pub(crate) fn check(grammar: &Grammar) -> Result<Checked<'_>, Error> {
    let repository = sorted(&grammar.repository);
    let mut checker = Checker {
        grammar,
        names: repository.iter().map(|entry| &*entry.0).collect(),
        repository,
        path: Vec::new(),
        work: Vec::new(),
        scratch: Vec::new(),
        buffers: Buffers::default(),
    };
    checker.header()?;
    checker.names()?;
    let injections = sorted(&grammar.injections);
    for (selector, _) in &injections {
        if let Err(problem) = scope::selector(selector) {
            let path = pointer(&[Seg::Key("injections"), Seg::Name(selector)]);
            return Err(selector_error(path, selector, problem));
        }
    }
    if let Some(pair) = injections.windows(2).find(|w| w[0].0 == w[1].0) {
        return Err(Error::DuplicateInjection {
            selector: pair[0].0.to_string(),
        });
    }

    checker.schedule_list(&[Seg::Key("patterns")], &grammar.patterns, 1);
    checker.run()?;
    for i in 0..checker.repository.len() {
        let entry = checker.repository[i];
        checker.schedule_entry("repository", entry);
        checker.run()?;
    }
    for &entry in &injections {
        checker.schedule_entry("injections", entry);
        checker.run()?;
    }
    checker.cycles()?;
    Ok(Checked {
        repository: checker.repository,
        injections,
    })
}

/// Entries sorted by name; the sort is stable, so of two entries with the
/// same name the first added comes first.
fn sorted(entries: &[(Text, Rule)]) -> Vec<&(Text, Rule)> {
    let mut refs: Vec<&(Text, Rule)> = entries.iter().collect();
    refs.sort_by(|a, b| a.0.cmp(&b.0));
    refs
}

/// Renders path segments as a JSON Pointer (RFC 6901).
fn pointer(segments: &[Seg<'_>]) -> String {
    let mut out = String::new();
    for seg in segments {
        out.push('/');
        match seg {
            Seg::Key(key) => out.push_str(key),
            Seg::Name(name) => {
                for c in name.chars() {
                    match c {
                        '~' => out.push_str("~0"),
                        '/' => out.push_str("~1"),
                        _ => out.push(c),
                    }
                }
            }
            Seg::Index(i) => crate::json::push_decimal(&mut out, *i as u64),
            Seg::Group(g) => crate::json::push_decimal(&mut out, u64::from(*g)),
        }
    }
    out
}

fn selector_error(path: String, selector: &str, problem: Problem) -> Error {
    let (offset, reason) = match problem {
        Problem::At(offset, reason) => (offset, reason),
        // Selectors have no substitutions.
        Problem::Group(_) => (0, "capture substitutions are not allowed in selectors"),
    };
    Error::InvalidSelector {
        path,
        selector: selector.to_string(),
        offset,
        reason,
    }
}

struct Checker<'g> {
    grammar: &'g Grammar,
    /// Repository entries sorted by name.
    repository: Vec<&'g (Text, Rule)>,
    /// Their names, in the same order: contiguous, so a binary search
    /// touches only the probed names' bytes.
    names: Vec<&'g str>,
    path: Vec<Seg<'g>>,
    work: Vec<Step<'g>>,
    /// Reused for sorting capture group numbers.
    scratch: Vec<u32>,
    /// Reused by the regex checker.
    buffers: Buffers,
}

impl<'g> Checker<'g> {
    fn here(&self, extra: &[Seg<'g>]) -> String {
        let mut segments = self.path.clone();
        segments.extend_from_slice(extra);
        pointer(&segments)
    }

    /// `scopeName`, `fileTypes`, `firstLineMatch`, `injectionSelector`.
    fn header(&mut self) -> Result<(), Error> {
        let g = self.grammar;
        if let Err(problem) = scope::single(&g.scope_name) {
            return Err(self.scope_error(&[Seg::Key("scopeName")], &g.scope_name, problem, 0));
        }
        for (i, file_type) in g.file_types.iter().enumerate() {
            if file_type.is_empty() {
                return Err(Error::InvalidName {
                    path: pointer(&[Seg::Key("fileTypes"), Seg::Index(i)]),
                    name: String::new(),
                    reason: "file types cannot be empty",
                });
            }
        }
        if let Some(regex) = &g.first_line_match {
            let _groups = self.regex(regex, "firstLineMatch", Backrefs::Own, 0)?;
        }
        if let Some(selector) = &g.injection_selector {
            if let Err(problem) = scope::selector(selector) {
                let path = pointer(&[Seg::Key("injectionSelector")]);
                return Err(selector_error(path, selector, problem));
            }
        }
        Ok(())
    }

    /// Repository names: well formed and unique.
    fn names(&self) -> Result<(), Error> {
        for (name, _) in &self.repository {
            if let Err(reason) = scope::repository_name(name) {
                return Err(Error::InvalidName {
                    path: pointer(&[Seg::Key("repository"), Seg::Name(name)]),
                    name: name.to_string(),
                    reason,
                });
            }
        }
        if let Some(pair) = self.repository.windows(2).find(|w| w[0].0 == w[1].0) {
            return Err(Error::DuplicateRepository {
                name: pair[0].0.to_string(),
            });
        }
        Ok(())
    }

    fn schedule_entry(&mut self, key: &'static str, entry: &'g (Text, Rule)) {
        self.work.push(Step::Leave);
        self.work.push(Step::Leave);
        self.work.push(Step::Rule(&entry.1, 1));
        self.work.push(Step::Enter(Seg::Name(&entry.0)));
        self.work.push(Step::Enter(Seg::Key(key)));
    }

    /// Schedules `rules` under the path segments `keys`, each at `depth`.
    fn schedule_list(&mut self, keys: &[Seg<'g>], rules: &'g [Rule], depth: usize) {
        for _ in keys {
            self.work.push(Step::Leave);
        }
        for (i, rule) in rules.iter().enumerate().rev() {
            self.work.push(Step::Leave);
            self.work.push(Step::Rule(rule, depth));
            self.work.push(Step::Enter(Seg::Index(i)));
        }
        for key in keys.iter().rev() {
            self.work.push(Step::Enter(*key));
        }
    }

    /// Schedules the patterns of each capture in `captures`.
    fn schedule_captures(&mut self, key: &'static str, captures: &'g Captures, depth: usize) {
        for (group, capture) in captures.iter().rev() {
            if !capture.patterns.is_empty() {
                let keys = [Seg::Key(key), Seg::Group(*group), Seg::Key("patterns")];
                self.schedule_list(&keys, &capture.patterns, depth + 1);
            }
        }
    }

    fn run(&mut self) -> Result<(), Error> {
        while let Some(step) = self.work.pop() {
            match step {
                Step::Enter(seg) => self.path.push(seg),
                Step::Leave => {
                    let _left = self.path.pop();
                }
                Step::Rule(rule, depth) => self.rule(rule, depth)?,
            }
        }
        Ok(())
    }

    fn rule(&mut self, rule: &'g Rule, depth: usize) -> Result<(), Error> {
        if depth > MAX_RULE_DEPTH {
            return Err(Error::TooDeep {
                path: self.here(&[]),
            });
        }
        match &rule.0 {
            Kind::Match(m) => {
                let info = self.regex(&m.regex, "match", Backrefs::Own, 0)?;
                self.scope(m.scope.as_deref(), "name", info.groups)?;
                self.captures("captures", &m.captures, info.groups)?;
                self.schedule_captures("captures", &m.captures, depth);
            }
            Kind::BeginEnd(r) => {
                let begin = self.regex(&r.begin, "begin", Backrefs::Own, 0)?;
                let end = self.regex(&r.end, "end", Backrefs::Begin, begin.groups)?;
                self.region_names(r.scope.as_deref(), r.content_scope.as_deref(), begin)?;
                self.captures("beginCaptures", &r.begin_captures, begin.groups)?;
                self.captures("endCaptures", &r.end_captures, end.groups)?;
                self.shared_captures(&r.captures, &r.begin_captures, &r.end_captures, begin, end)?;
                self.schedule_list(&[Seg::Key("patterns")], &r.patterns, depth + 1);
                self.schedule_captures("captures", &r.captures, depth);
                self.schedule_captures("endCaptures", &r.end_captures, depth);
                self.schedule_captures("beginCaptures", &r.begin_captures, depth);
            }
            Kind::BeginWhile(r) => {
                let begin = self.regex(&r.begin, "begin", Backrefs::Own, 0)?;
                let while_ = self.regex(&r.while_, "while", Backrefs::Begin, begin.groups)?;
                self.region_names(r.scope.as_deref(), r.content_scope.as_deref(), begin)?;
                self.captures("beginCaptures", &r.begin_captures, begin.groups)?;
                self.captures("whileCaptures", &r.while_captures, while_.groups)?;
                self.shared_captures(
                    &r.captures,
                    &r.begin_captures,
                    &r.while_captures,
                    begin,
                    while_,
                )?;
                self.schedule_list(&[Seg::Key("patterns")], &r.patterns, depth + 1);
                self.schedule_captures("captures", &r.captures, depth);
                self.schedule_captures("whileCaptures", &r.while_captures, depth);
                self.schedule_captures("beginCaptures", &r.begin_captures, depth);
            }
            Kind::Include(include) => self.include(include)?,
            Kind::Patterns(rules) => self.schedule_list(&[Seg::Key("patterns")], rules, depth + 1),
        }
        Ok(())
    }

    /// Checks a pattern under `key`. In [`Backrefs::Begin`] mode, numbered
    /// back-references must name one of `begin_groups`.
    fn regex(
        &mut self,
        text: &str,
        key: &'static str,
        backrefs: Backrefs,
        begin_groups: u32,
    ) -> Result<Info, Error> {
        if text.is_empty() {
            return Err(Error::EmptyRegex {
                path: self.here(&[Seg::Key(key)]),
            });
        }
        let checked = regex::check_in(text, backrefs, &mut self.buffers);
        let invalid = |error| Error::InvalidRegex {
            path: self.here(&[Seg::Key(key)]),
            error,
        };
        let info = checked.map_err(invalid)?;
        if let Some((group, offset)) = info.max_backref {
            if group > begin_groups {
                let kind = RegexErrorKind::BackreferenceOutOfRange {
                    group,
                    groups: begin_groups,
                };
                return Err(invalid(RegexError { offset, kind }));
            }
        }
        Ok(info)
    }

    /// A region's `name` and `contentName`, whose substitutions name `begin`
    /// groups.
    fn region_names(
        &self,
        name: Option<&str>,
        content: Option<&str>,
        begin: Info,
    ) -> Result<(), Error> {
        self.scope(name, "name", begin.groups)?;
        self.scope(content, "contentName", begin.groups)
    }

    fn scope(&self, scope: Option<&str>, key: &'static str, groups: u32) -> Result<(), Error> {
        match scope {
            Some(text) => scope::list(text, Some(groups))
                .map_err(|problem| self.scope_error(&[Seg::Key(key)], text, problem, groups)),
            None => Ok(()),
        }
    }

    fn scope_error(&self, at: &[Seg<'g>], text: &str, problem: Problem, groups: u32) -> Error {
        let path = self.here(at);
        match problem {
            Problem::At(offset, reason) => Error::InvalidScope {
                path,
                scope: text.to_string(),
                offset,
                reason,
            },
            Problem::Group(group) => Error::CaptureOutOfRange {
                path,
                group,
                groups,
            },
        }
    }

    /// A captures map: unique groups that exist, with valid scopes.
    fn captures(
        &mut self,
        key: &'static str,
        captures: &'g Captures,
        groups: u32,
    ) -> Result<(), Error> {
        if captures.is_empty() {
            return Ok(());
        }
        self.scratch.clear();
        self.scratch
            .extend(captures.iter().map(|(group, _)| *group));
        self.scratch.sort_unstable();
        if let Some(pair) = self.scratch.windows(2).find(|w| w[0] == w[1]) {
            let group = pair[0];
            return Err(Error::DuplicateCapture {
                path: self.here(&[Seg::Key(key)]),
                group,
            });
        }
        for (group, Capture { scope, .. }) in captures {
            let at = [Seg::Key(key), Seg::Group(*group)];
            if *group > groups {
                return Err(Error::CaptureOutOfRange {
                    path: self.here(&at),
                    group: *group,
                    groups,
                });
            }
            if let Some(text) = scope {
                let at = [Seg::Key(key), Seg::Group(*group), Seg::Key("name")];
                scope::list(text, Some(groups))
                    .map_err(|problem| self.scope_error(&at, text, problem, groups))?;
            }
        }
        Ok(())
    }

    /// A region's `captures`, which apply to each side without its own.
    fn shared_captures(
        &mut self,
        shared: &'g Captures,
        begin_side: &Captures,
        other_side: &Captures,
        begin: Info,
        other: Info,
    ) -> Result<(), Error> {
        if shared.is_empty() {
            return Ok(());
        }
        let groups = match (begin_side.is_empty(), other_side.is_empty()) {
            (true, true) => begin.groups.min(other.groups),
            (true, false) => begin.groups,
            (false, true) => other.groups,
            (false, false) => {
                return Err(Error::ShadowedCaptures {
                    path: self.here(&[]),
                });
            }
        };
        self.captures("captures", shared, groups)
    }

    fn include(&self, include: &Include) -> Result<(), Error> {
        let at = [Seg::Key("include")];
        match include {
            Include::SelfRef | Include::Base => Ok(()),
            Include::Repository(name) => self.entry(name),
            Include::Scope(scope) => {
                scope::single(scope).map_err(|problem| self.scope_error(&at, scope, problem, 0))
            }
            Include::ScopeEntry(scope, name) => {
                scope::single(scope).map_err(|problem| self.scope_error(&at, scope, problem, 0))?;
                if **scope == *self.grammar.scope_name {
                    self.entry(name)
                } else {
                    scope::repository_name(name).map_err(|reason| Error::InvalidName {
                        path: self.here(&at),
                        name: name.to_string(),
                        reason,
                    })
                }
            }
        }
    }

    /// An include of this grammar's repository entry `name`.
    fn entry(&self, name: &str) -> Result<(), Error> {
        let at = [Seg::Key("include")];
        scope::repository_name(name).map_err(|reason| Error::InvalidName {
            path: self.here(&at),
            name: name.to_string(),
            reason,
        })?;
        if self.lookup(name).is_none() {
            return Err(Error::UndefinedInclude {
                path: self.here(&at),
                name: name.to_string(),
            });
        }
        Ok(())
    }

    fn lookup(&self, name: &str) -> Option<usize> {
        self.names.binary_search(&name).ok()
    }

    /// The include-graph node an include leads to, if it is in this grammar:
    /// 0 for the top-level patterns, `i + 1` for repository entry `i`.
    fn target(&self, include: &Include) -> Option<usize> {
        let own = &*self.grammar.scope_name;
        match include {
            Include::SelfRef | Include::Base => Some(0),
            Include::Repository(name) => self.lookup(name).map(|i| i + 1),
            Include::Scope(scope) => (**scope == *own).then_some(0),
            Include::ScopeEntry(scope, name) if **scope == *own => self.lookup(name).map(|i| i + 1),
            Include::ScopeEntry(..) => None,
        }
    }

    /// The rules a node expands into when a tokenizer collects its
    /// patterns, if it is a pattern list: the top-level patterns, or a
    /// repository entry that is a list of patterns or a single include.
    fn body(&self, node: usize) -> Option<&'g [Rule]> {
        if node == 0 {
            return Some(&self.grammar.patterns);
        }
        let entry = self.repository.get(node - 1)?;
        match &entry.1.0 {
            Kind::Patterns(rules) => Some(rules),
            Kind::Include(_) => Some(core::slice::from_ref(&entry.1)),
            _ => None,
        }
    }

    /// Refuses a cycle of pattern lists that include one another.
    fn cycles(&self) -> Result<(), Error> {
        let nodes = self.repository.len() + 1;
        let mut edges: Vec<(usize, usize)> = Vec::new();
        let mut stack: Vec<&Rule> = Vec::new();
        for node in 0..nodes {
            let Some(body) = self.body(node) else {
                continue;
            };
            stack.extend(body);
            while let Some(rule) = stack.pop() {
                match &rule.0 {
                    Kind::Patterns(rules) => stack.extend(rules),
                    Kind::Include(include) => {
                        if let Some(target) = self.target(include) {
                            if self.body(target).is_some() {
                                edges.push((node, target));
                            }
                        }
                    }
                    // A region contributes only its `begin`; a match only
                    // itself. Neither expands further.
                    _ => {}
                }
            }
        }
        edges.sort_unstable();
        edges.dedup();
        // Compressed adjacency: the edges of node `n` are
        // `edges[start[n]..start[n + 1]]`.
        let mut start = Vec::with_capacity(nodes + 1);
        let mut at = 0;
        for node in 0..=nodes {
            while edges.get(at).is_some_and(|&(from, _)| from < node) {
                at += 1;
            }
            start.push(at);
        }
        match find_cycle(nodes, &start, &edges) {
            Some(cycle) => Err(Error::IncludeCycle {
                cycle: cycle.into_iter().map(|node| self.node_name(node)).collect(),
            }),
            None => Ok(()),
        }
    }

    fn node_name(&self, node: usize) -> String {
        match node.checked_sub(1).and_then(|i| self.repository.get(i)) {
            Some(entry) => {
                let mut name = String::with_capacity(entry.0.len() + 1);
                name.push('#');
                name.push_str(&entry.0);
                name
            }
            None => "$self".to_string(),
        }
    }
}

/// Finds a cycle by iterative depth-first search, returning its nodes with
/// the first repeated at the end.
fn find_cycle(nodes: usize, start: &[usize], edges: &[(usize, usize)]) -> Option<Vec<usize>> {
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;
    let mut color = alloc::vec![WHITE; nodes];
    // (node, index of its next edge to follow)
    let mut path: Vec<(usize, usize)> = Vec::new();
    for root in 0..nodes {
        if color[root] != WHITE {
            continue;
        }
        color[root] = GRAY;
        path.push((root, start.get(root).copied().unwrap_or(0)));
        while let Some(top) = path.last_mut() {
            let node = top.0;
            let end = start.get(node + 1).copied().unwrap_or(0);
            if top.1 < end {
                let target = edges.get(top.1).map_or(node, |&(_, to)| to);
                top.1 += 1;
                match color.get(target).copied() {
                    Some(GRAY) => {
                        let from = path.iter().position(|&(n, _)| n == target).unwrap_or(0);
                        let mut cycle: Vec<usize> = path[from..].iter().map(|&(n, _)| n).collect();
                        cycle.push(target);
                        return Some(cycle);
                    }
                    Some(WHITE) => {
                        color[target] = GRAY;
                        path.push((target, start.get(target).copied().unwrap_or(0)));
                    }
                    _ => {}
                }
            } else {
                color[node] = BLACK;
                let _done = path.pop();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    fn csr(nodes: usize, mut edges: Vec<(usize, usize)>) -> Option<Vec<usize>> {
        edges.sort_unstable();
        let mut start = Vec::new();
        let mut at = 0;
        for node in 0..=nodes {
            while edges.get(at).is_some_and(|&(from, _)| from < node) {
                at += 1;
            }
            start.push(at);
        }
        find_cycle(nodes, &start, &edges)
    }

    #[test]
    fn test_find_cycle() {
        assert_eq!(csr(3, vec![(0, 1), (1, 2)]), None);
        assert_eq!(csr(3, vec![(0, 1), (1, 2), (2, 1)]), Some(vec![1, 2, 1]));
        assert_eq!(csr(1, vec![(0, 0)]), Some(vec![0, 0]));
        assert_eq!(csr(4, vec![(0, 1), (0, 2), (1, 3), (2, 3)]), None);
        assert_eq!(csr(0, vec![]), None);
    }

    #[test]
    fn test_find_cycle_long_chain_is_iterative() {
        let n = 200_000;
        let mut edges: Vec<(usize, usize)> = (0..n - 1).map(|i| (i, i + 1)).collect();
        assert_eq!(csr(n, edges.clone()), None);
        edges.push((n - 1, 0));
        assert_eq!(csr(n, edges).map(|c| c.len()), Some(n + 1));
    }

    #[test]
    fn test_pointer_escapes_names() {
        let p = pointer(&[
            Seg::Key("injections"),
            Seg::Name("a/b~c"),
            Seg::Index(3),
            Seg::Group(7),
        ]);
        assert_eq!(p, "/injections/a~1b~0c/3/7");
    }
}
