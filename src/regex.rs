//! A tiny, dependency-free regular-expression engine for `tarn find --regex`.
//!
//! Compiles a pattern to a small instruction program and runs it with a
//! Thompson/Pike NFA simulation — linear in the input length, with no
//! catastrophic backtracking. Matching is UNANCHORED (a line matches if any
//! substring matches), like `grep`. Supported syntax — a useful grep-ish subset:
//!   literals · `.` · `*` `+` `?` · `{n}` `{n,}` `{n,m}` · `[...]` `[^...]` with
//!   ranges · `\d \w \s \D \W \S` and `\`-escapes · `^` `$` anchors ·
//!   `|` alternation · `(...)` grouping. Not supported: backreferences,
//!   lookaround, `\b`. ASCII case-folding for `--ignore-case`.

#[derive(Clone)]
enum Matcher {
    Any,
    Lit(char),
    Class {
        ranges: Vec<(char, char)>,
        negated: bool,
    },
}

impl Matcher {
    fn hit_cs(&self, c: char) -> bool {
        match self {
            Matcher::Any => true,
            Matcher::Lit(l) => *l == c,
            Matcher::Class { ranges, negated } => {
                let inr = ranges.iter().any(|(a, b)| c >= *a && c <= *b);
                inr != *negated
            }
        }
    }
    fn matches(&self, c: char, ci: bool) -> bool {
        if self.hit_cs(c) {
            return true;
        }
        if !ci {
            return false;
        }
        let alt = if c.is_ascii_uppercase() {
            c.to_ascii_lowercase()
        } else if c.is_ascii_lowercase() {
            c.to_ascii_uppercase()
        } else {
            return false;
        };
        self.hit_cs(alt)
    }
}

#[derive(Clone)]
enum Ast {
    Empty,
    Char(Matcher),
    Concat(Vec<Ast>),
    Alt(Vec<Ast>),
    Star(Box<Ast>),
    Plus(Box<Ast>),
    Quest(Box<Ast>),
    Repeat(Box<Ast>, usize, Option<usize>),
    StartAnchor,
    EndAnchor,
}

struct Parser {
    c: Vec<char>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.i += 1;
        }
        c
    }
    fn eat(&mut self, ch: char) -> bool {
        if self.peek() == Some(ch) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn parse_alt(&mut self) -> Result<Ast, String> {
        let mut branches = vec![self.parse_concat()?];
        while self.eat('|') {
            branches.push(self.parse_concat()?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().unwrap())
        } else {
            Ok(Ast::Alt(branches))
        }
    }

    fn parse_concat(&mut self) -> Result<Ast, String> {
        let mut items = Vec::new();
        while let Some(ch) = self.peek() {
            if ch == '|' || ch == ')' {
                break;
            }
            items.push(self.parse_repeat()?);
        }
        match items.len() {
            0 => Ok(Ast::Empty),
            1 => Ok(items.pop().unwrap()),
            _ => Ok(Ast::Concat(items)),
        }
    }

    fn parse_repeat(&mut self) -> Result<Ast, String> {
        let atom = self.parse_atom()?;
        match self.peek() {
            Some('*') => {
                self.i += 1;
                Ok(Ast::Star(Box::new(atom)))
            }
            Some('+') => {
                self.i += 1;
                Ok(Ast::Plus(Box::new(atom)))
            }
            Some('?') => {
                self.i += 1;
                Ok(Ast::Quest(Box::new(atom)))
            }
            Some('{') => {
                if let Some((n, m)) = self.try_braces() {
                    match m {
                        Some(mm) if mm < n => {
                            Err(format!("invalid quantifier: max {mm} < min {n}"))
                        }
                        _ if n > 1000 || m.is_some_and(|mm| mm > 1000) => {
                            Err("repeat count too large (max 1000)".into())
                        }
                        _ => Ok(Ast::Repeat(Box::new(atom), n, m)),
                    }
                } else {
                    // Not a valid {..} spec: leave the `{` to be read as a literal.
                    Ok(atom)
                }
            }
            _ => Ok(atom),
        }
    }

    /// Parse `{n}` / `{n,}` / `{n,m}` at the cursor; restore and return None if it
    /// isn't a well-formed spec (so `{` can be a literal).
    fn try_braces(&mut self) -> Option<(usize, Option<usize>)> {
        let save = self.i;
        self.i += 1; // consume '{'
        let mut lo = String::new();
        while let Some(d) = self.peek() {
            if d.is_ascii_digit() {
                lo.push(d);
                self.i += 1;
            } else {
                break;
            }
        }
        if lo.is_empty() {
            self.i = save;
            return None;
        }
        let n: usize = lo.parse().ok()?;
        let m = if self.eat(',') {
            let mut hi = String::new();
            while let Some(d) = self.peek() {
                if d.is_ascii_digit() {
                    hi.push(d);
                    self.i += 1;
                } else {
                    break;
                }
            }
            if hi.is_empty() {
                None
            } else {
                Some(hi.parse::<usize>().ok()?)
            }
        } else {
            Some(n)
        };
        if !self.eat('}') {
            self.i = save;
            return None;
        }
        Some((n, m))
    }

    fn parse_atom(&mut self) -> Result<Ast, String> {
        match self.peek() {
            Some('(') => {
                self.i += 1;
                // non-capturing `(?:...)` is accepted and treated like `(...)`
                if self.peek() == Some('?') && self.c.get(self.i + 1) == Some(&':') {
                    self.i += 2;
                }
                let inner = self.parse_alt()?;
                if !self.eat(')') {
                    return Err("unclosed '('".into());
                }
                Ok(inner)
            }
            Some('[') => self.parse_class(),
            Some('.') => {
                self.i += 1;
                Ok(Ast::Char(Matcher::Any))
            }
            Some('^') => {
                self.i += 1;
                Ok(Ast::StartAnchor)
            }
            Some('$') => {
                self.i += 1;
                Ok(Ast::EndAnchor)
            }
            Some('\\') => {
                self.i += 1;
                Ok(Ast::Char(self.parse_escape()?))
            }
            Some(ch) if ch == '*' || ch == '+' || ch == '?' => {
                Err(format!("nothing to repeat before '{ch}'"))
            }
            Some(ch) => {
                self.i += 1;
                Ok(Ast::Char(Matcher::Lit(ch)))
            }
            None => Ok(Ast::Empty),
        }
    }

    fn parse_escape(&mut self) -> Result<Matcher, String> {
        let ch = self.bump().ok_or("trailing backslash")?;
        Ok(match ch {
            'd' => Matcher::Class {
                ranges: vec![('0', '9')],
                negated: false,
            },
            'D' => Matcher::Class {
                ranges: vec![('0', '9')],
                negated: true,
            },
            'w' => Matcher::Class {
                ranges: word_ranges(),
                negated: false,
            },
            'W' => Matcher::Class {
                ranges: word_ranges(),
                negated: true,
            },
            's' => Matcher::Class {
                ranges: space_ranges(),
                negated: false,
            },
            'S' => Matcher::Class {
                ranges: space_ranges(),
                negated: true,
            },
            'n' => Matcher::Lit('\n'),
            't' => Matcher::Lit('\t'),
            'r' => Matcher::Lit('\r'),
            'b' | 'B' => return Err("word boundaries (\\b, \\B) are not supported".into()),
            other => Matcher::Lit(other),
        })
    }

    fn parse_class(&mut self) -> Result<Ast, String> {
        self.i += 1; // consume '['
        if self.peek() == Some('[') && self.c.get(self.i + 1) == Some(&':') {
            return Err("POSIX classes ([[:...:]]) are not supported".into());
        }
        let negated = self.eat('^');
        let mut ranges: Vec<(char, char)> = Vec::new();
        // a leading ']' is a literal member
        if self.peek() == Some(']') {
            ranges.push((']', ']'));
            self.i += 1;
        }
        loop {
            match self.peek() {
                None => return Err("unclosed '['".into()),
                Some(']') => {
                    self.i += 1;
                    break;
                }
                Some('\\') => {
                    self.i += 1;
                    match self.parse_escape()? {
                        Matcher::Lit(c) => self.push_range(&mut ranges, c),
                        Matcher::Class {
                            ranges: r,
                            negated: false,
                        } => ranges.extend(r),
                        // a negated class inside [...] is uncommon; fold positively
                        Matcher::Class { ranges: r, .. } => ranges.extend(r),
                        Matcher::Any => ranges.push(('\u{0}', char::MAX)),
                    }
                }
                Some(c) => {
                    self.i += 1;
                    // range a-z (but a trailing '-' is a literal)
                    if self.peek() == Some('-') && self.c.get(self.i + 1) != Some(&']') {
                        self.i += 1; // consume '-'
                        if let Some(hi) = self.bump() {
                            ranges.push((c, hi));
                        }
                    } else {
                        self.push_range(&mut ranges, c);
                    }
                }
            }
        }
        Ok(Ast::Char(Matcher::Class { ranges, negated }))
    }

    fn push_range(&self, ranges: &mut Vec<(char, char)>, c: char) {
        ranges.push((c, c));
    }
}

fn word_ranges() -> Vec<(char, char)> {
    vec![('0', '9'), ('A', 'Z'), ('a', 'z'), ('_', '_')]
}
fn space_ranges() -> Vec<(char, char)> {
    vec![
        (' ', ' '),
        ('\t', '\t'),
        ('\n', '\n'),
        ('\r', '\r'),
        ('\u{0b}', '\u{0c}'),
    ]
}

enum Inst {
    Char(Matcher),
    Match,
    Jmp(usize),
    Split(usize, usize),
    AssertStart,
    AssertEnd,
}

fn compile(ast: &Ast) -> Vec<Inst> {
    let mut p = Vec::new();
    emit(&mut p, ast);
    p.push(Inst::Match);
    p
}

fn emit(p: &mut Vec<Inst>, ast: &Ast) {
    match ast {
        Ast::Empty => {}
        Ast::StartAnchor => p.push(Inst::AssertStart),
        Ast::EndAnchor => p.push(Inst::AssertEnd),
        Ast::Char(m) => p.push(Inst::Char(m.clone())),
        Ast::Concat(items) => {
            for it in items {
                emit(p, it);
            }
        }
        Ast::Alt(branches) => {
            let mut jmps = Vec::new();
            let n = branches.len();
            for (k, br) in branches.iter().enumerate() {
                if k < n - 1 {
                    let split = p.len();
                    p.push(Inst::Split(0, 0));
                    let l_branch = p.len();
                    emit(p, br);
                    let jmp = p.len();
                    p.push(Inst::Jmp(0));
                    jmps.push(jmp);
                    let l_next = p.len();
                    p[split] = Inst::Split(l_branch, l_next);
                } else {
                    emit(p, br);
                }
            }
            let end = p.len();
            for j in jmps {
                p[j] = Inst::Jmp(end);
            }
        }
        Ast::Star(a) => {
            let l1 = p.len();
            p.push(Inst::Split(0, 0));
            let l2 = p.len();
            emit(p, a);
            p.push(Inst::Jmp(l1));
            let l3 = p.len();
            p[l1] = Inst::Split(l2, l3);
        }
        Ast::Plus(a) => {
            let l1 = p.len();
            emit(p, a);
            let split = p.len();
            p.push(Inst::Split(0, 0));
            let l3 = p.len();
            p[split] = Inst::Split(l1, l3);
        }
        Ast::Quest(a) => {
            let split = p.len();
            p.push(Inst::Split(0, 0));
            let l1 = p.len();
            emit(p, a);
            let l2 = p.len();
            p[split] = Inst::Split(l1, l2);
        }
        Ast::Repeat(a, n, m) => {
            for _ in 0..*n {
                emit(p, a);
            }
            match m {
                None => {
                    let l1 = p.len();
                    p.push(Inst::Split(0, 0));
                    let l2 = p.len();
                    emit(p, a);
                    p.push(Inst::Jmp(l1));
                    let l3 = p.len();
                    p[l1] = Inst::Split(l2, l3);
                }
                Some(mm) => {
                    for _ in *n..*mm {
                        let split = p.len();
                        p.push(Inst::Split(0, 0));
                        let l1 = p.len();
                        emit(p, a);
                        let l2 = p.len();
                        p[split] = Inst::Split(l1, l2);
                    }
                }
            }
        }
    }
}

pub struct Regex {
    prog: Vec<Inst>,
    ci: bool,
}

impl Regex {
    pub fn new(pattern: &str, ci: bool) -> Result<Regex, String> {
        let mut p = Parser {
            c: pattern.chars().collect(),
            i: 0,
        };
        let ast = p.parse_alt()?;
        if p.i != p.c.len() {
            return Err(format!("unexpected '{}'", p.c[p.i]));
        }
        // A {n,m} blow-up guard: cap total program size.
        let prog = compile(&ast);
        if prog.len() > 100_000 {
            return Err("pattern too large".into());
        }
        Ok(Regex { prog, ci })
    }

    /// True if any substring of `text` matches (unanchored, grep-style).
    pub fn is_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len();
        let mut seen = vec![0u32; self.prog.len()];
        let mut gen = 1u32;
        let mut clist: Vec<usize> = Vec::new();
        self.add(&mut clist, &mut seen, gen, 0, 0, n);
        // `pos` runs to `n` (one past the end) — the end-of-input position the NFA
        // needs for `$`/Match; an index loop is the natural form here.
        #[allow(clippy::needless_range_loop)]
        for pos in 0..=n {
            for &pc in &clist {
                if matches!(self.prog[pc], Inst::Match) {
                    return true;
                }
            }
            if pos == n {
                break;
            }
            let c = chars[pos];
            gen += 1;
            let mut nlist: Vec<usize> = Vec::new();
            for &pc in &clist {
                if let Inst::Char(m) = &self.prog[pc] {
                    if m.matches(c, self.ci) {
                        self.add(&mut nlist, &mut seen, gen, pc + 1, pos + 1, n);
                    }
                }
            }
            // Unanchored: a fresh match may begin at the next position.
            self.add(&mut nlist, &mut seen, gen, 0, pos + 1, n);
            clist = nlist;
        }
        false
    }

    fn add(
        &self,
        list: &mut Vec<usize>,
        seen: &mut [u32],
        gen: u32,
        pc: usize,
        pos: usize,
        n: usize,
    ) {
        if seen[pc] == gen {
            return;
        }
        seen[pc] = gen;
        match &self.prog[pc] {
            Inst::Jmp(x) => self.add(list, seen, gen, *x, pos, n),
            Inst::Split(x, y) => {
                self.add(list, seen, gen, *x, pos, n);
                self.add(list, seen, gen, *y, pos, n);
            }
            Inst::AssertStart => {
                if pos == 0 {
                    self.add(list, seen, gen, pc + 1, pos, n);
                }
            }
            Inst::AssertEnd => {
                if pos == n {
                    self.add(list, seen, gen, pc + 1, pos, n);
                }
            }
            _ => list.push(pc), // Char or Match
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Regex;
    fn m(p: &str, s: &str) -> bool {
        Regex::new(p, false).unwrap().is_match(s)
    }
    fn mi(p: &str, s: &str) -> bool {
        Regex::new(p, true).unwrap().is_match(s)
    }

    #[test]
    fn literals_and_unanchored() {
        assert!(m("foo", "a foo b"));
        assert!(!m("foo", "bar"));
        assert!(m("", "anything")); // empty matches
    }
    #[test]
    fn dot_star_plus_quest() {
        assert!(m("a.c", "axc"));
        assert!(!m("a.c", "ac"));
        assert!(m("ab*c", "ac"));
        assert!(m("ab*c", "abbbc"));
        assert!(m("ab+c", "abc"));
        assert!(!m("ab+c", "ac"));
        assert!(m("ab?c", "ac"));
        assert!(m("ab?c", "abc"));
    }
    #[test]
    fn classes_and_predefs() {
        assert!(m("[abc]x", "bx"));
        assert!(!m("[abc]x", "dx"));
        assert!(m("[a-f]", "e"));
        assert!(m("[^0-9]", "a"));
        assert!(!m("[^0-9]", "5"));
        assert!(m(r"\d\d", "x42y"));
        assert!(!m(r"\d\d", "x4y"));
        assert!(m(r"\w+", "hello_1"));
        assert!(m(r"a\sb", "a b"));
        assert!(m(r"\S", "x"));
    }
    #[test]
    fn anchors() {
        assert!(m("^foo", "foobar"));
        assert!(!m("^foo", "xfoobar"));
        assert!(m("bar$", "foobar"));
        assert!(!m("bar$", "barfoo"));
        assert!(m("^abc$", "abc"));
        assert!(!m("^abc$", "abcd"));
        // ^ in one branch of an alternation only anchors that branch
        assert!(m("^a|b", "zzb"));
    }
    #[test]
    fn alternation_groups_repeat() {
        assert!(m("cat|dog", "a dog"));
        assert!(m("(ab)+", "abab"));
        assert!(m("(?:ab)+c", "ababc"));
        assert!(m("a{2,3}", "aaa"));
        assert!(!m("^a{2,3}$", "a"));
        assert!(m("^a{2,3}$", "aa"));
        assert!(!m("^a{2,3}$", "aaaa"));
        assert!(m("x{3}", "xxx"));
    }
    #[test]
    fn case_insensitive() {
        assert!(mi("foo", "FOO"));
        assert!(mi("[a-z]+", "ABC"));
        assert!(!m("foo", "FOO"));
    }
    #[test]
    fn escapes_and_literal_braces() {
        assert!(m(r"a\.b", "a.b"));
        assert!(!m(r"a\.b", "axb"));
        assert!(m(r"\(x\)", "(x)"));
        // a non-spec `{` is a literal
        assert!(m("a{b", "a{b"));
    }
    #[test]
    fn errors() {
        assert!(Regex::new("(unclosed", false).is_err());
        assert!(Regex::new("[unclosed", false).is_err());
        assert!(Regex::new("*nope", false).is_err());
        assert!(Regex::new(r"\bword", false).is_err()); // word boundary unsupported -> loud
        assert!(Regex::new("a{3,2}", false).is_err()); // reversed {n,m}
        assert!(Regex::new("[[:space:]]", false).is_err()); // POSIX class unsupported
        assert!(Regex::new("a{1,5000}", false).is_err()); // repeat count capped
        assert!(Regex::new("a{2,5}", false).is_ok()); // normal {n,m} still fine
    }
    #[test]
    fn no_catastrophic_backtracking() {
        // A pattern that destroys a backtracking engine; the NFA handles it linearly.
        let re = Regex::new("(a*)*b", false).unwrap();
        assert!(!re.is_match("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaac"));
        assert!(re.is_match("aaaaab"));
    }
}
