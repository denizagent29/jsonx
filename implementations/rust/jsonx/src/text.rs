use crate::*;
use std::collections::{HashMap, HashSet};

pub const MAX_DEPTH: usize = 256;
pub type Id = usize;
pub const NO_POS: usize = usize::MAX;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind { Null, Bool(bool), Int(i128), Float(f64), F32(f32), Str(String), Bytes(Vec<u8>), Arr(Vec<Id>), Obj(Vec<(String, Id)>), Ref }
#[derive(Debug, Clone, PartialEq)]
pub enum Key { None, Name(String), Index(usize) }
#[derive(Debug, Clone)]
pub enum Seg { Key(String), Index(usize), Parent, Prev, Next, Desc(String) }
#[derive(Debug, Clone, PartialEq)]
pub enum Start { Cur, Global, Ctx, Var, Name }
#[derive(Debug, Clone)]
pub struct Expr { pub start: Start, pub name: String, pub segs: Vec<Seg>, pub pos: usize }
#[derive(Debug, Clone)]
pub struct Label { pub name: String, pub global: bool, pub expr: Option<Expr>, pub pos: usize, pub owner: Id, pub target: Option<Id> }
#[derive(Debug, Clone)]
pub struct Node { pub kind: Kind, pub pos: usize, pub parent: Option<Id>, pub key: Key, pub labels: Vec<usize>, pub expr: Option<Expr>, pub target: Option<Id> }

/// A parsed, resolved document. Nodes live in an arena and are addressed by `Id`.
#[derive(Debug, Clone)]
pub struct Doc { pub nodes: Vec<Node>, pub root: Id, pub src: Vec<char>, pub labels: Vec<Label> }

impl Doc {
    pub fn empty() -> Doc { Doc { nodes: vec![], root: 0, src: vec![], labels: vec![] } }
    fn new(src: Vec<char>) -> Doc { Doc { nodes: vec![], root: 0, src, labels: vec![] } }
    /// Append a detached node (use [`DocBuilder`] for a convenient API).
    pub fn add(&mut self, kind: Kind, pos: usize) -> Id {
        self.nodes.push(Node { kind, pos, parent: None, key: Key::None, labels: vec![], expr: None, target: None });
        self.nodes.len() - 1
    }
    pub fn node(&self, id: Id) -> &Node { &self.nodes[id] }
    /// Follow a reference to its target (identity for non-references).
    pub fn deref(&self, id: Id) -> Id { if self.nodes[id].kind == Kind::Ref { self.nodes[id].target.unwrap_or(id) } else { id } }
    pub fn get(&self, id: Id, k: &str) -> Option<Id> {
        match &self.nodes[self.deref(id)].kind { Kind::Obj(m) => m.iter().find(|(n, _)| n == k).map(|(_, c)| self.deref(*c)), _ => None }
    }
    pub fn at(&self, id: Id, i: usize) -> Option<Id> {
        match &self.nodes[self.deref(id)].kind { Kind::Arr(a) => a.get(i).map(|c| self.deref(*c)), _ => None }
    }
    pub fn path(&self, id: Id) -> String {
        let mut parts = vec![]; let mut n = id;
        while let Some(p) = self.nodes[n].parent {
            parts.push(match &self.nodes[n].key { Key::Name(s) => s.replace('~', "~0").replace('/', "~1"), Key::Index(i) => i.to_string(), Key::None => String::new() });
            n = p;
        }
        parts.iter().rev().map(|s| format!("/{}", s)).collect()
    }
    pub fn dump(&self) -> String { let mut s = String::new(); self.dump_node(self.root, &mut s); s }
    fn dump_node(&self, id: Id, out: &mut String) {
        use std::fmt::Write;
        match &self.nodes[id].kind {
            Kind::Null => out.push_str("null"),
            Kind::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Kind::Int(i) => { let _ = write!(out, "{}", i); }
            Kind::Float(f) => { let _ = write!(out, "f64(0x{:016x})", f.to_bits()); }
            Kind::F32(f) => { let _ = write!(out, "f32(0x{:08x})", f.to_bits()); }
            Kind::Str(s) => out.push_str(&jstr(s)),
            Kind::Bytes(b) => { let _ = write!(out, "bytes(\"{}\")", b64(b)); }
            Kind::Ref => { let t = self.nodes[id].target.unwrap(); out.push_str(&format!("ref({})", jstr(&self.path(t)))); }
            Kind::Arr(a) => { out.push('['); for (i, c) in a.iter().enumerate() { if i > 0 { out.push(','); } self.dump_node(*c, out); } out.push(']'); }
            Kind::Obj(m) => { out.push('{'); for (i, (k, c)) in m.iter().enumerate() { if i > 0 { out.push(','); } out.push_str(&jstr(k)); out.push(':'); self.dump_node(*c, out); } out.push('}'); }
        }
    }
}
pub(crate) fn jstr(s: &str) -> String {
    let mut o = String::from("\"");
    for ch in s.chars() { let c = ch as u32; match ch { '"' => o.push_str("\\\""), '\\' => o.push_str("\\\\"), _ if c < 0x20 || c == 0x7f => o.push_str(&format!("\\u{:04x}", c)), _ => o.push(ch) } }
    o.push('"'); o
}
pub(crate) fn b64(b: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        o.push(A[(n >> 18) as usize & 63] as char); o.push(A[(n >> 12) as usize & 63] as char);
        o.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' }); o.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    o
}
pub(crate) fn unb64(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.len() % 4 != 0 { return None; }
    let val = |c: u8| -> Option<u32> { match c { b'A'..=b'Z' => Some((c - b'A') as u32), b'a'..=b'z' => Some((c - b'a') as u32 + 26), b'0'..=b'9' => Some((c - b'0') as u32 + 52), b'+' => Some(62), b'/' => Some(63), _ => None } };
    let mut out = vec![];
    for (ci, c) in b.chunks(4).enumerate() {
        let last = (ci + 1) * 4 == b.len();
        let pad = if last { c.iter().rev().take_while(|&&x| x == b'=').count() } else { 0 };
        if pad > 2 { return None; }
        let mut n = 0u32;
        for (i, &x) in c.iter().enumerate() { n <<= 6; if i >= 4 - pad { continue; } n |= val(x)?; }
        out.push((n >> 16) as u8); if pad < 2 { out.push((n >> 8) as u8); } if pad < 1 { out.push(n as u8); }
    }
    Some(out)
}

fn is_id_start(c: char) -> bool {
    let o = c as u32;
    if o < 0x80 { return c == '_' || c.is_ascii_alphabetic(); }
    !((0x80..=0xA0).contains(&o) || o == 0x1680 || (0x2000..=0x200F).contains(&o) || (0x2028..=0x202F).contains(&o) || (0x205F..=0x206F).contains(&o) || o == 0x3000 || o == 0xFEFF || o == 0xFFFE || o == 0xFFFF)
}
fn is_id_char(c: char) -> bool { is_id_start(c) || c.is_ascii_digit() }

struct P { s: Vec<char>, i: usize, doc: Doc }
impl P {
    fn n(&self) -> usize { self.s.len() }
    fn ch(&self, i: usize) -> Option<char> { self.s.get(i).copied() }
    fn cur(&self) -> Option<char> { self.ch(self.i) }
    fn skip(&mut self) -> Res<()> {
        while let Some(c) = self.cur() {
            if matches!(c, ' ' | '\t' | '\r' | '\n') { self.i += 1; }
            else if c == '/' && self.ch(self.i + 1) == Some('/') { self.i += 2; while let Some(d) = self.cur() { if d == '\r' || d == '\n' { break; } self.i += 1; } }
            else if c == '/' && self.ch(self.i + 1) == Some('*') {
                let mut j = self.i + 2; let mut found = false;
                while j + 1 < self.n() { if self.s[j] == '*' && self.s[j + 1] == '/' { found = true; break; } j += 1; }
                if !found { return errp("E_SYNTAX", "unterminated block comment", self.i); }
                self.i = j + 2;
            } else { break; }
        }
        Ok(())
    }
    fn ident(&mut self) -> String { let st = self.i; while self.cur().map_or(false, is_id_char) { self.i += 1; } self.s[st..self.i].iter().collect() }
    fn ascii_ident(&self, at: usize) -> Option<String> {
        let c = self.ch(at)?; if !(c == '_' || c.is_ascii_alphabetic()) { return None; }
        let mut j = at + 1; while self.ch(j).map_or(false, |c| c == '_' || c.is_ascii_alphanumeric()) { j += 1; }
        Some(self.s[at..j].iter().collect())
    }
    fn document(&mut self) -> Res<Id> {
        if self.cur() == Some('\u{feff}') { self.i = 1; }
        self.skip()?;
        if self.i >= self.n() { return errp("E_UNEXPECTED_EOF", "empty document: expected a value", self.i); }
        let v = self.value(0)?; self.skip()?;
        if self.i < self.n() { return errp("E_SYNTAX", "unexpected content after the root value", self.i); }
        Ok(v)
    }
    fn value(&mut self, depth: usize) -> Res<Id> {
        self.skip()?; let i = self.i;
        let c = match self.cur() { Some(c) => c, None => return errp("E_UNEXPECTED_EOF", "unexpected end of input: expected a value", i) };
        match c {
            '{' => return self.obj(depth), '[' => return self.arr(depth),
            '"' => { let s = self.string()?; return Ok(self.doc.add(Kind::Str(s), i)); }
            '@' => return self.reference(),
            '-' | '0'..='9' => return self.number(),
            '+' | '.' => return errp("E_INVALID_NUMBER", format!("numbers cannot start with '{}'", c), i),
            _ => {}
        }
        if is_id_start(c) {
            let w = self.ident();
            return match w.as_str() { "true" => Ok(self.doc.add(Kind::Bool(true), i)), "false" => Ok(self.doc.add(Kind::Bool(false), i)), "null" => Ok(self.doc.add(Kind::Null, i)),
                _ => errp("E_SYNTAX", format!("unexpected identifier '{}': expected a value", w), i) };
        }
        if c == '$' { return errp("E_SYNTAX", "unexpected '$': contextual variables are only valid inside references", i); }
        errp("E_SYNTAX", format!("unexpected character {:?}", c), i)
    }
    fn number(&mut self) -> Res<Id> {
        let i = self.i; let n = self.n(); let mut j = i;
        if self.s[j] == '-' { j += 1; }
        if j >= n || !self.s[j].is_ascii_digit() { return errp("E_INVALID_NUMBER", "'-' must be followed by a digit", i); }
        if self.s[j] == '0' { j += 1; } else { while j < n && self.s[j].is_ascii_digit() { j += 1; } }
        let mut is_f = false;
        if j + 1 < n && self.s[j] == '.' && self.s[j + 1].is_ascii_digit() { j += 1; while j < n && self.s[j].is_ascii_digit() { j += 1; } is_f = true; }
        if j < n && (self.s[j] == 'e' || self.s[j] == 'E') {
            let mut k = j + 1; if k < n && (self.s[k] == '+' || self.s[k] == '-') { k += 1; }
            if k < n && self.s[k].is_ascii_digit() { while k < n && self.s[k].is_ascii_digit() { k += 1; } j = k; is_f = true; }
        }
        if j < n { let c = self.s[j]; if c == '.' || c.is_ascii_digit() || is_id_char(c) { return errp("E_INVALID_NUMBER", "malformed number", i); } }
        self.i = j; let txt: String = self.s[i..j].iter().collect();
        if !is_f {
            return match txt.parse::<i128>() { Ok(v) if v >= -(1i128 << 63) && v < (1i128 << 64) => Ok(self.doc.add(Kind::Int(v), i)),
                _ => errp("E_NUMBER_OVERFLOW", format!("integer {} does not fit in [-2^63, 2^64-1]", txt), i) };
        }
        let f: f64 = txt.parse().unwrap();
        if f.is_infinite() { return errp("E_NUMBER_OVERFLOW", format!("float {} overflows IEEE-754 binary64", txt), i); }
        Ok(self.doc.add(Kind::Float(f), i))
    }
    fn hex4(&self, i: usize) -> Res<u32> {
        let h: String = self.s[i.min(self.n())..(i + 4).min(self.n())].iter().collect();
        if h.chars().count() != 4 || !h.chars().all(|c| c.is_ascii_hexdigit()) { return errp("E_INVALID_ESCAPE", "\\u needs exactly 4 hex digits", i.wrapping_sub(2)); }
        Ok(u32::from_str_radix(&h, 16).unwrap())
    }
    fn string(&mut self) -> Res<String> {
        let n = self.n(); let open = self.i; let mut i = open + 1; let mut out = String::new();
        loop {
            if i >= n { return errp("E_UNEXPECTED_EOF", "unterminated string", open); }
            let c = self.s[i];
            if c == '"' { self.i = i + 1; return Ok(out); }
            if c == '\\' {
                i += 1; if i >= n { return errp("E_UNEXPECTED_EOF", "unterminated escape", i); }
                let e = self.s[i];
                match e {
                    '"' | '\\' | '/' => { out.push(e); i += 1; } 'b' => { out.push('\u{8}'); i += 1; } 'f' => { out.push('\u{c}'); i += 1; }
                    'n' => { out.push('\n'); i += 1; } 'r' => { out.push('\r'); i += 1; } 't' => { out.push('\t'); i += 1; }
                    'u' => {
                        if self.ch(i + 1) == Some('{') {
                            let mut j = i + 2; while j < n && self.s[j] != '}' { j += 1; }
                            let h: String = if j < n { self.s[i + 2..j].iter().collect() } else { String::new() };
                            if !(1..=6).contains(&h.len()) || !h.chars().all(|c| c.is_ascii_hexdigit()) { return errp("E_INVALID_ESCAPE", "malformed \\u{...} escape", i - 1); }
                            let cp = u32::from_str_radix(&h, 16).unwrap();
                            match char::from_u32(cp) { Some(ch) => out.push(ch), None => return errp("E_INVALID_ESCAPE", "not a Unicode scalar value", i - 1) }
                            i = j + 1;
                        } else {
                            let mut cp = self.hex4(i + 1)?; i += 5;
                            if (0xD800..=0xDBFF).contains(&cp) {
                                if !(self.ch(i) == Some('\\') && self.ch(i + 1) == Some('u')) { return errp("E_INVALID_ESCAPE", "lone high surrogate", i - 6); }
                                let lo = self.hex4(i + 2)?;
                                if !(0xDC00..=0xDFFF).contains(&lo) { return errp("E_INVALID_ESCAPE", "high surrogate not followed by low surrogate", i - 6); }
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00); i += 6;
                            } else if (0xDC00..=0xDFFF).contains(&cp) { return errp("E_INVALID_ESCAPE", "lone low surrogate", i - 6); }
                            out.push(char::from_u32(cp).unwrap());
                        }
                    }
                    _ => return errp("E_INVALID_ESCAPE", "unknown escape", i - 1),
                }
                continue;
            }
            if (c as u32) < 0x20 { return errp("E_SYNTAX", "unescaped control character in string", i); }
            out.push(c); i += 1;
        }
    }
    fn obj(&mut self, depth: usize) -> Res<Id> {
        if depth >= MAX_DEPTH { return errp("E_DEPTH_LIMIT", "nesting too deep", self.i); }
        let id = self.doc.add(Kind::Obj(vec![]), self.i); self.i += 1;
        let mut seen: HashSet<String> = HashSet::new(); let mut members: Vec<(String, Id)> = vec![];
        loop {
            self.skip()?;
            let c = match self.cur() { Some(c) => c, None => return errp("E_UNEXPECTED_EOF", "unterminated object", self.i) };
            if c == '}' { self.i += 1; break; }
            if c == '<' { let l = self.label(id)?; let li = self.doc.labels.len(); self.doc.labels.push(l); self.doc.nodes[id].labels.push(li); continue; }
            let kpos = self.i;
            let key = if c == '"' { self.string()? } else if is_id_start(c) { self.ident() } else { return errp("E_SYNTAX", "expected an object key", self.i); };
            self.skip()?;
            if self.cur() != Some(':') { return errp("E_SYNTAX", "expected ':' after object key", self.i); }
            self.i += 1;
            let v = self.value(depth + 1)?;
            if !seen.insert(key.clone()) { return errp("E_DUPLICATE_KEY", "duplicate key", kpos); }
            self.doc.nodes[v].parent = Some(id); self.doc.nodes[v].key = Key::Name(key.clone()); members.push((key, v));
            self.skip()?;
            match self.cur() { None => return errp("E_UNEXPECTED_EOF", "unterminated object", self.i), Some(',') => self.i += 1, Some('}') => { self.i += 1; break; }
                _ => return errp("E_SYNTAX", "expected ',' or '}' after value", self.i) }
        }
        self.doc.nodes[id].kind = Kind::Obj(members); Ok(id)
    }
    fn arr(&mut self, depth: usize) -> Res<Id> {
        if depth >= MAX_DEPTH { return errp("E_DEPTH_LIMIT", "nesting too deep", self.i); }
        let id = self.doc.add(Kind::Arr(vec![]), self.i); self.i += 1; let mut items = vec![];
        loop {
            self.skip()?;
            match self.cur() { None => return errp("E_UNEXPECTED_EOF", "unterminated array", self.i), Some(']') => { self.i += 1; break; } _ => {} }
            let v = self.value(depth + 1)?;
            self.doc.nodes[v].parent = Some(id); self.doc.nodes[v].key = Key::Index(items.len()); items.push(v);
            self.skip()?;
            match self.cur() { None => return errp("E_UNEXPECTED_EOF", "unterminated array", self.i), Some(',') => self.i += 1, Some(']') => { self.i += 1; break; }
                _ => return errp("E_SYNTAX", "expected ',' or ']' after element", self.i) }
        }
        self.doc.nodes[id].kind = Kind::Arr(items); Ok(id)
    }
    fn label(&mut self, owner: Id) -> Res<Label> {
        let pos = self.i; self.i += 1; self.skip()?; let npos = self.i; let mut var = false;
        if self.cur() == Some('$') { var = true; self.i += 1; }
        let mut name = match self.ascii_ident(self.i) { Some(n) => n, None => return errp("E_INVALID_LABEL_NAME", "expected a label name after '<'", npos) };
        self.i += name.len();
        if self.cur().map_or(false, is_id_char) { return errp("E_INVALID_LABEL_NAME", "label names are ASCII", npos); }
        let global;
        if var { global = false; name = format!("${}", name); if name == "$prev" || name == "$next" { return errp("E_RESERVED_LABEL", format!("'{}' is a built-in contextual variable", name), pos); } }
        else if name.starts_with('_') { return errp("E_INVALID_LABEL_NAME", "label names must start with a letter", npos); }
        else { global = name.chars().next().unwrap().is_ascii_uppercase(); }
        self.skip()?; let mut expr = None;
        if self.cur() == Some('=') {
            self.i += 1; self.skip()?; let at = self.i; if self.cur() == Some('@') { self.i += 1; }
            expr = Some(self.path(at)?); self.skip()?;
        }
        if self.cur() != Some('>') { return errp("E_SYNTAX", "expected '>' to close the label declaration", self.i); }
        self.i += 1;
        Ok(Label { name, global, expr, pos, owner, target: None })
    }
    fn seg_start(&self) -> bool { match self.cur() { None => false, Some(c) => c == '!' || c == '"' || c == '$' || is_id_char(c) } }
    fn segment(&mut self) -> Res<Seg> {
        let i = self.i; let c = self.cur();
        match c {
            Some('!') => { self.i += 1; Ok(Seg::Parent) }
            Some('"') => Ok(Seg::Key(self.string()?)),
            Some('$') => {
                let nm = match self.ascii_ident(i + 1) { Some(n) => n, None => return errp("E_INVALID_POINTER", "expected a name after '$'", self.i) };
                self.i = i + 1 + nm.len();
                Ok(match nm.as_str() { "prev" => Seg::Prev, "next" => Seg::Next, _ => Seg::Desc(nm) })
            }
            Some(d) if d.is_ascii_digit() => {
                let mut j = i; while self.ch(j).map_or(false, |c| c.is_ascii_digit()) { j += 1; }
                if self.ch(j).map_or(false, is_id_start) { return errp("E_INVALID_POINTER", "malformed array index", self.i); }
                let t: String = self.s[i..j].iter().collect();
                if t.len() > 1 && t.starts_with('0') { return errp("E_INVALID_POINTER", "array indexes have no leading zeros", self.i); }
                self.i = j; Ok(Seg::Index(t.parse().unwrap_or(usize::MAX)))
            }
            Some(d) if is_id_start(d) => Ok(Seg::Key(self.ident())),
            _ => errp("E_INVALID_POINTER", "expected a field name, index, '$name' or '!' in pointer", self.i),
        }
    }
    fn path(&mut self, at: usize) -> Res<Expr> {
        let mut segs = vec![]; let start; let mut name = String::new();
        match self.cur() {
            Some('!') => {
                self.i += 1;
                if !self.cur().map_or(false, |c| is_id_start(c) || c == '"') { return errp("E_INVALID_POINTER", "'@!' must be followed by a field name", self.i); }
                start = Start::Cur; segs.push(self.segment()?);
            }
            Some('.') => {
                if self.ch(self.i + 1) == Some('.') {
                    self.i += 2;
                    match self.ascii_ident(self.i) { Some(n) => { self.i += n.len(); name = n; } None => return errp("E_INVALID_POINTER", "'@..' must be followed by a global label name", self.i) }
                    start = Start::Global;
                } else { self.i += 1; start = Start::Cur; if self.seg_start() { segs.push(self.segment()?); } }
            }
            Some('$') => match self.segment()? { Seg::Prev => { start = Start::Ctx; name = "prev".into(); } Seg::Next => { start = Start::Ctx; name = "next".into(); }
                Seg::Desc(n) => { start = Start::Var; name = format!("${}", n); } _ => unreachable!() },
            Some(c) if is_id_start(c) => { start = Start::Name; name = self.ident(); }
            _ => return errp("E_INVALID_POINTER", "expected a label, field name, '.', '!' or '$' after '@'", self.i),
        }
        while self.cur() == Some('.') { self.i += 1; segs.push(self.segment()?); }
        Ok(Expr { start, name, segs, pos: at })
    }
    fn reference(&mut self) -> Res<Id> {
        let at = self.i; self.i += 1; let id = self.doc.add(Kind::Ref, at); let e = self.path(at)?; self.doc.nodes[id].expr = Some(e); Ok(id)
    }
}

struct R { doc: Doc, globals: HashMap<String, usize>, by_name: HashMap<String, Vec<usize>>, active: HashSet<(u8, usize)> }
impl R {
    fn run(&mut self) -> Res<()> {
        let mut seen_obj: HashMap<(usize, String), usize> = HashMap::new();
        let mut order = vec![]; self.collect_order(self.doc.root, &mut order);
        for &li in &order {
            let (name, global, owner, pos) = { let l = &self.doc.labels[li]; (l.name.clone(), l.global, l.owner, l.pos) };
            if global { if self.globals.contains_key(&name) { return errp("E_DUPLICATE_LABEL", format!("global label '{}' is already defined", name), pos); } self.globals.insert(name.clone(), li); }
            else { if seen_obj.insert((owner, name.clone()), li).is_some() { return errp("E_DUPLICATE_LABEL", format!("label '{}' is declared twice in the same object", name), pos); } }
            self.by_name.entry(name).or_default().push(li);
        }
        // labels were pushed in document order of declaration (parse order), which equals Python's collect order for diagnostics
        for &li in &order { self.label_target(li)?; }
        for id in 0..self.doc.nodes.len() { if self.doc.nodes[id].kind == Kind::Ref { self.target(id)?; } }
        Ok(())
    }
    fn collect_order(&self, n: Id, out: &mut Vec<usize>) {
        match &self.doc.nodes[n].kind {
            Kind::Obj(m) => { out.extend(self.doc.nodes[n].labels.iter().copied()); for (_, c) in m { self.collect_order(*c, out); } }
            Kind::Arr(a) => { for c in a { self.collect_order(*c, out); } }
            _ => {}
        }
    }
    fn target(&mut self, r: Id) -> Res<Id> {
        if let Some(t) = self.doc.nodes[r].target { return Ok(t); }
        let pos = self.doc.nodes[r].pos;
        if !self.active.insert((0, r)) { return errp("E_CYCLIC_REFERENCE", "reference depends on itself", pos); }
        let ex = self.doc.nodes[r].expr.clone().unwrap();
        let mut t = self.eval(&ex, r, false, pos)?;
        while self.doc.nodes[t].kind == Kind::Ref { t = self.target(t)?; }
        self.active.remove(&(0, r)); self.doc.nodes[r].target = Some(t); Ok(t)
    }
    fn label_target(&mut self, li: usize) -> Res<Id> {
        if let Some(t) = self.doc.labels[li].target { return Ok(t); }
        let (owner, pos, name) = { let l = &self.doc.labels[li]; (l.owner, l.pos, l.name.clone()) };
        let ex = match self.doc.labels[li].expr.clone() { None => { self.doc.labels[li].target = Some(owner); return Ok(owner); } Some(e) => e };
        if !self.active.insert((1, li)) { return errp("E_CYCLIC_REFERENCE", format!("label '{}' is defined in terms of itself", name), pos); }
        let mut t = self.eval(&ex, owner, true, pos)?;
        while self.doc.nodes[t].kind == Kind::Ref { t = self.target(t)?; }
        self.active.remove(&(1, li)); self.doc.labels[li].target = Some(t); Ok(t)
    }
    fn deref(&mut self, mut n: Id) -> Res<Id> { while self.doc.nodes[n].kind == Kind::Ref { n = self.target(n)?; } Ok(n) }
    fn elem(&self, loc: Id) -> Option<Id> {
        let mut n = Some(loc);
        while let Some(c) = n { if let Some(p) = self.doc.nodes[c].parent { if matches!(self.doc.nodes[p].kind, Kind::Arr(_)) { return Some(c); } } n = self.doc.nodes[c].parent; }
        None
    }
    fn find_label(&self, name: &str, loc: Id) -> Option<usize> {
        let mut n = Some(loc);
        while let Some(c) = n {
            if matches!(self.doc.nodes[c].kind, Kind::Obj(_)) { for &li in &self.doc.nodes[c].labels { if self.doc.labels[li].name == name && !self.doc.labels[li].global { return Some(li); } } }
            n = self.doc.nodes[c].parent;
        }
        self.globals.get(name).copied()
    }
    fn desc(&self, n: Id) -> String { let p = self.doc.path(n); if p.is_empty() { "the root".into() } else { format!("'{}'", p) } }
    fn search(&self, n: Id, name: &str, out: &mut Vec<usize>) {
        match &self.doc.nodes[n].kind {
            Kind::Obj(m) => { for &li in &self.doc.nodes[n].labels { let l = &self.doc.labels[li].name; if l == name || *l == format!("${}", name) { out.push(li); } } for (_, c) in m { self.search(*c, name, out); } }
            Kind::Arr(a) => { for c in a { self.search(*c, name, out); } }
            _ => {}
        }
    }
    fn eval(&mut self, ex: &Expr, loc: Id, is_lab: bool, epos: usize) -> Res<Id> {
        let mut segs = ex.segs.clone();
        let mut cur: Id;
        match ex.start {
            Start::Cur => {
                if is_lab { cur = loc; } else {
                    let mut n = self.doc.nodes[loc].parent;
                    while let Some(c) = n { if matches!(self.doc.nodes[c].kind, Kind::Obj(_)) { break; } n = self.doc.nodes[c].parent; }
                    match n { Some(c) => cur = c, None => return errp("E_INVALID_CURRENT", "there is no enclosing object", epos) }
                }
            }
            Start::Global => match self.globals.get(&ex.name).copied() { None => return errp("E_UNDEFINED_LABEL", format!("global label '{}' is not defined", ex.name), epos), Some(li) => cur = self.label_target(li)? },
            Start::Ctx => {
                match self.elem(loc) {
                    None => return errp(if ex.name == "prev" { "E_INVALID_PREV" } else { "E_INVALID_NEXT" }, format!("'${}' used where there is no enclosing array element", ex.name), epos),
                    Some(e) => cur = e,
                }
                segs.insert(0, if ex.name == "prev" { Seg::Prev } else { Seg::Next });
            }
            Start::Name | Start::Var => {
                let nm = &ex.name; let lb = self.find_label(nm, loc);
                let rk = if ex.start == Start::Name { match &self.doc.nodes[self.doc.root].kind { Kind::Obj(m) => m.iter().find(|(k, _)| k == nm).map(|(_, c)| *c), _ => None } } else { None };
                if lb.is_some() && rk.is_some() { return errp("E_AMBIGUOUS_NAME", format!("'{}' names both a label and a field of the root object", nm), epos); }
                if let Some(li) = lb { cur = self.label_target(li)?; }
                else if let Some(c) = rk { cur = c; }
                else {
                    if self.by_name.get(nm).map_or(false, |v| !v.is_empty()) { return errp("E_LABEL_OUT_OF_SCOPE", format!("label '{}' is not visible from here", nm), epos); }
                    return errp("E_UNDEFINED_LABEL", format!("no label or root field named '{}'", nm), epos);
                }
            }
        }
        for sg in segs {
            match sg {
                Seg::Key(k) => {
                    cur = self.deref(cur)?;
                    match &self.doc.nodes[cur].kind {
                        Kind::Obj(m) => match m.iter().find(|(n, _)| *n == k) { Some((_, c)) => cur = *c, None => return errp("E_POINTER_NO_FIELD", format!("{} has no field '{}'", self.desc(cur), k), epos) },
                        _ => return errp("E_POINTER_TYPE", format!("cannot select field '{}' of a non-object", k), epos),
                    }
                }
                Seg::Index(i) => {
                    cur = self.deref(cur)?;
                    match &self.doc.nodes[cur].kind {
                        Kind::Arr(a) => { if i >= a.len() { return errp("E_INVALID_INDEX", format!("index {} is out of range (length {})", i, a.len()), epos); } cur = a[i]; }
                        _ => return errp("E_POINTER_TYPE", "cannot index a non-array", epos),
                    }
                }
                Seg::Parent => match self.doc.nodes[cur].parent { Some(p) => cur = p, None => return errp("E_INVALID_PARENT", format!("{} has no parent", self.desc(cur)), epos) },
                Seg::Prev | Seg::Next => {
                    let prev = matches!(sg, Seg::Prev); let code = if prev { "E_INVALID_PREV" } else { "E_INVALID_NEXT" };
                    let p = match self.doc.nodes[cur].parent { Some(p) if matches!(self.doc.nodes[p].kind, Kind::Arr(_)) => p, _ => return errp(code, "needs an array element", epos) };
                    let idx = match self.doc.nodes[cur].key { Key::Index(i) => i as isize, _ => 0 };
                    let t = idx + if prev { -1 } else { 1 };
                    let a = match &self.doc.nodes[p].kind { Kind::Arr(a) => a, _ => unreachable!() };
                    if t < 0 { return errp(code, "'$prev' used on the first element: there is no previous element", epos); }
                    if t as usize >= a.len() { return errp(code, "'$next' used on the last element: there is no next element", epos); }
                    cur = a[t as usize];
                }
                Seg::Desc(name) => {
                    let mut found = vec![]; self.search(cur, &name, &mut found);
                    if found.is_empty() { return errp("E_UNDEFINED_LABEL", format!("no label '{}' is declared inside {}", name, self.desc(cur)), epos); }
                    if found.len() > 1 { return errp("E_AMBIGUOUS_LABEL", format!("label '{}' is declared more than once inside {}", name, self.desc(cur)), epos); }
                    cur = self.label_target(found[0])?;
                }
            }
        }
        Ok(cur)
    }
}

/// Parse JSONX text, validate it and resolve all labels and pointers.
pub fn parse(text: &str) -> Res<Doc> {
    let chars: Vec<char> = text.chars().collect();
    let fix = |mut e: Error| { if e.line.is_none() { if let Some(p) = e.pos { let (l, c) = line_col(&chars, p); e.line = Some(l); e.col = Some(c); } } e };
    let mut p = P { s: chars.clone(), i: 0, doc: Doc::new(chars.clone()) };
    let root = p.document().map_err(&fix)?;
    p.doc.root = root;
    let mut r = R { doc: p.doc, globals: HashMap::new(), by_name: HashMap::new(), active: HashSet::new() };
    r.run().map_err(&fix)?;
    Ok(r.doc)
}
