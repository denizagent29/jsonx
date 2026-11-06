//! Schema language compiler (`.jxs` text -> [`Schema`]); mirrors the Python reference compiler, including error codes and positions.
use crate::*;
use std::collections::{HashMap, HashSet};

/// Resolves `import "path"`: `(path, importing_file) -> (source text, canonical file name)`.
pub type Loader<'a> = &'a dyn Fn(&str, &str) -> std::io::Result<(String, String)>;

#[derive(Clone, Debug, PartialEq)]
enum TK { Num, Id, Str, P, Eof }
#[derive(Clone, Debug)]
struct Tok { kind: TK, val: String, pos: usize }
fn serr<T>(code: &str, fname: &str, msg: &str, pos: usize, src: &[char]) -> Res<T> {
    let (l, c) = line_col(src, pos);
    Err(Error { code: code.into(), message: format!("{}: {}", fname, msg), pos: Some(pos), line: Some(l), col: Some(c) })
}
fn lex(src: &[char], fname: &str) -> Res<Vec<Tok>> {
    let mut out = vec![]; let n = src.len(); let mut i = 0;
    while i < n {
        let c = src[i];
        if c.is_whitespace() { i += 1; continue; }
        if c == '/' && i + 1 < n && src[i + 1] == '/' { while i < n && src[i] != '\n' { i += 1; } continue; }
        if c == '/' && i + 1 < n && src[i + 1] == '*' {
            let mut j = i + 2; let mut found = false;
            while j + 1 < n { if src[j] == '*' && src[j + 1] == '/' { found = true; break; } j += 1; }
            if found { i = j + 2; continue; }
        }
        let st = i;
        let digit = |k: usize| k < n && src[k].is_ascii_digit();
        if digit(i) || (c == '-' && digit(i + 1)) {
            if c == '-' { i += 1; } while digit(i) { i += 1; }
            if i + 1 < n && src[i] == '.' && digit(i + 1) { i += 1; while digit(i) { i += 1; } }
            if i < n && (src[i] == 'e' || src[i] == 'E') { let mut k = i + 1; if k < n && (src[k] == '+' || src[k] == '-') { k += 1; } if digit(k) { while digit(k) { k += 1; } i = k; } }
            out.push(Tok { kind: TK::Num, val: src[st..i].iter().collect(), pos: st });
        } else if c == '_' || c.is_ascii_alphabetic() {
            while i < n && (src[i] == '_' || src[i].is_ascii_alphanumeric()) { i += 1; }
            out.push(Tok { kind: TK::Id, val: src[st..i].iter().collect(), pos: st });
        } else if c == '"' {
            i += 1; let mut ok = false;
            while i < n { if src[i] == '"' { ok = true; i += 1; break; } if src[i] == '\n' { break; } if src[i] == '\\' && i + 1 < n { i += 2; } else { i += 1; } }
            if !ok { return serr("E_SCHEMA_SYNTAX", fname, &format!("unexpected character {:?}", c), st, src); }
            out.push(Tok { kind: TK::Str, val: src[st..i].iter().collect(), pos: st });
        } else if "{}<>():,;=@.[]".contains(c) { i += 1; out.push(Tok { kind: TK::P, val: c.to_string(), pos: st }); }
        else { return serr("E_SCHEMA_SYNTAX", fname, &format!("unexpected character {:?}", c), i, src); }
    }
    out.push(Tok { kind: TK::Eof, val: String::new(), pos: n });
    Ok(out)
}
fn unq(s: &str) -> String {
    let inner: Vec<char> = s.chars().collect(); let inner = &inner[1..inner.len() - 1]; let mut o = String::new(); let mut i = 0;
    while i < inner.len() {
        if inner[i] == '\\' && i + 1 < inner.len() {
            let e = inner[i + 1]; i += 2;
            match e { 'n' => o.push('\n'), 't' => o.push('\t'), 'r' => o.push('\r'), '"' => o.push('"'), '\'' => o.push('\''), '\\' => o.push('\\'), '0' => o.push('\0'),
                'x' | 'u' => { let w = if e == 'x' { 2 } else { 4 }; let h: String = inner[i..(i + w).min(inner.len())].iter().collect();
                    match u32::from_str_radix(&h, 16).ok().filter(|_| h.len() == w).and_then(char::from_u32) { Some(ch) => { o.push(ch); i += w; } None => { o.push('\\'); o.push(e); } } }
                _ => { o.push('\\'); o.push(e); } }
        } else { o.push(inner[i]); i += 1; }
    }
    o
}

#[derive(Clone, Debug)]
enum Lit { Int(i128), Float(f64), Str(String), Bool(bool), Null, Arr }
#[derive(Clone, Debug)]
enum Raw { Name(String, usize), Optional(Box<Raw>, usize), Nullable(Box<Raw>, usize), Array(Box<Raw>, usize), Ref(Box<Raw>, usize) }
struct RawField { name: String, ty: Raw, dflt: Option<Lit>, attrs: Vec<(String, String)>, pos: usize }
enum RawKind { Struct(Vec<RawField>), Enum(Option<String>, Vec<(String, u64, usize)>), Union(Vec<(String, u64, Raw, usize, usize)>) }
struct RawDecl { name: String, pos: usize, fname: String, src: std::rc::Rc<Vec<char>>, kind: RawKind }
struct Meta { namespace: String, version: usize, root: Option<(String, usize)>, src: std::rc::Rc<Vec<char>>, fname: String }

struct SP { src: std::rc::Rc<Vec<char>>, fname: String, t: Vec<Tok>, i: usize }
impl SP {
    fn err<T>(&self, msg: &str) -> Res<T> { self.err_at(msg, self.t[self.i].pos, "E_SCHEMA_SYNTAX") }
    fn err_at<T>(&self, msg: &str, pos: usize, code: &str) -> Res<T> { serr(code, &self.fname, msg, pos, &self.src) }
    fn peek(&self) -> &Tok { &self.t[self.i] }
    fn nxt(&mut self) -> Tok { let t = self.t[self.i].clone(); if self.i < self.t.len() - 1 { self.i += 1; } else { self.i += 0; } t }
    fn is_p(&self, v: &str) -> bool { let t = self.peek(); t.kind == TK::P && t.val == v }
    fn eat_p(&mut self, v: &str) -> bool { if self.is_p(v) { self.i += 1; true } else { false } }
    fn expect_p(&mut self, v: &str) -> Res<()> { if self.eat_p(v) { Ok(()) } else { self.err(&format!("expected '{}', found '{}'", v, self.peek().val)) } }
    fn ident(&mut self) -> Res<String> { let t = self.peek().clone(); if t.kind != TK::Id { return self.err(&format!("expected a name, found '{}'", t.val)); } self.i += 1; Ok(t.val) }
    fn type_(&mut self) -> Res<Raw> {
        let pos = self.peek().pos; let name = self.ident()?;
        if ["array", "ref", "optional", "nullable"].contains(&name.as_str()) {
            self.expect_p("<")?; let inner = Box::new(self.type_()?); self.expect_p(">")?;
            return Ok(match name.as_str() { "array" => Raw::Array(inner, pos), "ref" => Raw::Ref(inner, pos), "optional" => Raw::Optional(inner, pos), _ => Raw::Nullable(inner, pos) });
        }
        Ok(Raw::Name(name, pos))
    }
    fn literal(&mut self) -> Res<Lit> {
        let t = self.t[self.i].clone(); if self.i < self.t.len() - 1 { self.i += 1; }
        match t.kind {
            TK::Num => Ok(if t.val.trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) { Lit::Int(t.val.parse::<i128>().unwrap_or(i128::MAX)) } else { Lit::Float(t.val.parse().unwrap()) }),
            TK::Str => Ok(Lit::Str(unq(&t.val))),
            TK::Id if t.val == "true" || t.val == "false" => Ok(Lit::Bool(t.val == "true")),
            TK::Id if t.val == "null" => Ok(Lit::Null),
            TK::P if t.val == "[" => { self.expect_p("]")?; Ok(Lit::Arr) }
            _ => { self.i -= 1; self.err("expected a literal default value") }
        }
    }
    fn attrs(&mut self) -> Res<Vec<(String, String)>> {
        let mut out = vec![];
        while self.is_p("@") {
            self.nxt(); let name = self.ident()?; let mut args = String::new();
            if self.is_p("(") {
                let a = self.nxt(); let mut depth = 1; let mut last;
                loop {
                    last = self.nxt();
                    if last.kind == TK::Eof { return self.err_at("unterminated attribute arguments", a.pos, "E_SCHEMA_SYNTAX"); }
                    if last.kind == TK::P && last.val == "(" { depth += 1; } else if last.kind == TK::P && last.val == ")" { depth -= 1; if depth == 0 { break; } }
                }
                args = self.src[a.pos + 1..last.pos].iter().collect::<String>().trim().to_string();
            }
            out.push((name, args));
        }
        Ok(out)
    }
}

fn parse_file(text: &str, fname: &str, loader: Option<Loader>, stack: &[String]) -> Res<(Vec<RawDecl>, Meta)> {
    let src: std::rc::Rc<Vec<char>> = std::rc::Rc::new(text.chars().collect());
    let mut p = SP { src: src.clone(), fname: fname.to_string(), t: lex(&src, fname)?, i: 0 };
    let mut decls: Vec<RawDecl> = vec![];
    let mut meta = Meta { namespace: String::new(), version: 1, root: None, src: src.clone(), fname: fname.to_string() };
    while p.peek().kind != TK::Eof {
        let t = p.peek().clone();
        if t.kind != TK::Id { return p.err(&format!("unexpected '{}' at top level", t.val)); }
        match t.val.as_str() {
            "namespace" => { p.nxt(); let mut parts = vec![p.ident()?]; while p.eat_p(".") { parts.push(p.ident()?); } meta.namespace = parts.join("."); }
            "version" => { p.nxt(); let n = p.nxt(); if n.kind != TK::Num || !n.val.chars().all(|c| c.is_ascii_digit()) || n.val.parse::<usize>().unwrap_or(0) < 1 { return p.err_at("version must be a positive integer", n.pos, "E_SCHEMA_SYNTAX"); } meta.version = n.val.parse().unwrap(); }
            "root" => { p.nxt(); let pos = p.peek().pos; meta.root = Some((p.ident()?, pos)); }
            "import" => {
                p.nxt(); let s = p.nxt(); if s.kind != TK::Str { return p.err_at("import expects a quoted path", s.pos, "E_SCHEMA_SYNTAX"); }
                let loader = match loader { Some(l) => l, None => return p.err_at("imports need a loader (use the file-based compiler)", s.pos, "E_SCHEMA_IMPORT") };
                let path = unq(&s.val);
                let (itext, iname) = match loader(&path, fname) { Ok(x) => x, Err(e) => return p.err_at(&format!("cannot import '{}': {}", path, e), s.pos, "E_SCHEMA_IMPORT") };
                if stack.contains(&iname) { return p.err_at(&format!("import cycle through '{}'", iname), s.pos, "E_SCHEMA_IMPORT"); }
                let mut st = stack.to_vec(); st.push(iname.clone());
                let (sub, _) = parse_file(&itext, &iname, Some(loader), &st)?; decls.extend(sub);
            }
            "schema" | "struct" => {
                p.nxt(); let pos = p.peek().pos; let name = p.ident()?; p.expect_p("{")?; let mut fields = vec![];
                while !p.eat_p("}") {
                    let fpos = p.peek().pos; let fname_ = p.ident()?; p.expect_p(":")?; let ty = p.type_()?; let mut dflt = None;
                    if p.eat_p("=") { dflt = Some(p.literal()?); }
                    let attrs = p.attrs()?; if !p.eat_p(",") { p.eat_p(";"); }
                    fields.push(RawField { name: fname_, ty, dflt, attrs, pos: fpos });
                }
                decls.push(RawDecl { name, pos, fname: fname.to_string(), src: src.clone(), kind: RawKind::Struct(fields) });
            }
            "enum" => {
                p.nxt(); let pos = p.peek().pos; let name = p.ident()?; let mut base = None;
                if p.eat_p(":") { let b = p.ident()?; if !["u8", "u16", "u32"].contains(&b.as_str()) { return p.err("enum base must be u8, u16 or u32"); } base = Some(b); }
                p.expect_p("{")?; let mut nv = 0u64; let mut values = vec![];
                while !p.eat_p("}") {
                    let vpos = p.peek().pos; let vn = p.ident()?;
                    if p.eat_p("=") { let n = p.nxt(); if n.kind != TK::Num || !n.val.chars().all(|c| c.is_ascii_digit()) { return p.err_at("enum values are non-negative integers", n.pos, "E_SCHEMA_SYNTAX"); } nv = n.val.parse().unwrap_or(u64::MAX); }
                    values.push((vn, nv, vpos)); nv += 1; if !p.eat_p(",") { p.eat_p(";"); }
                }
                decls.push(RawDecl { name, pos, fname: fname.to_string(), src: src.clone(), kind: RawKind::Enum(base, values) });
            }
            "union" => {
                p.nxt(); let pos = p.peek().pos; let name = p.ident()?; p.expect_p("{")?; let mut variants = vec![];
                while !p.eat_p("}") {
                    let vpos = p.peek().pos; let vn = p.ident()?; p.expect_p("=")?; let n = p.nxt();
                    let tag = if n.kind == TK::Num && n.val.chars().all(|c| c.is_ascii_digit()) { n.val.parse::<u64>().unwrap_or(0) } else { 0 };
                    if tag < 1 || tag > 255 { return p.err_at("union tags are integers 1..255", n.pos, "E_SCHEMA_SYNTAX"); }
                    p.expect_p(":")?; let ty = p.type_()?; variants.push((vn, tag, ty, vpos, 0)); if !p.eat_p(",") { p.eat_p(";"); }
                }
                decls.push(RawDecl { name, pos, fname: fname.to_string(), src: src.clone(), kind: RawKind::Union(variants) });
            }
            other => return p.err(&format!("unknown declaration '{}'", other)),
        }
        p.eat_p(";");
    }
    Ok((decls, meta))
}

fn range_ok(t: &Ty, v: i128) -> bool { match t { Ty::U8 => (0..256).contains(&v), Ty::U16 => (0..65536).contains(&v), Ty::U32 => (0..(1i128 << 32)).contains(&v), Ty::U64 => (0..(1i128 << 64)).contains(&v),
    Ty::I8 => (-128..128).contains(&v), Ty::I16 => (-32768..32768).contains(&v), Ty::I32 => (-(1i128 << 31)..(1i128 << 31)).contains(&v), Ty::I64 => (-(1i128 << 63)..(1i128 << 63)).contains(&v), _ => false } }
fn is_int(t: &Ty) -> bool { matches!(t, Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64 | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64) }
fn prim(n: &str) -> Option<Ty> { Some(match n { "bool" => Ty::Bool, "u8" => Ty::U8, "u16" => Ty::U16, "u32" => Ty::U32, "u64" => Ty::U64, "i8" => Ty::I8, "i16" => Ty::I16, "i32" => Ty::I32, "i64" => Ty::I64, "f32" => Ty::F32, "f64" => Ty::F64, "string" => Ty::Str, "bytes" => Ty::Bytes, "any" => Ty::Any, _ => return None }) }

fn build(decls: Vec<RawDecl>, meta: Meta) -> Res<Schema> {
    let msrc = &meta.src; let mfname = &meta.fname;
    let mut by_name: HashMap<String, usize> = HashMap::new(); let mut kinds: Vec<&'static str> = vec![];
    for (i, d) in decls.iter().enumerate() {
        if by_name.contains_key(&d.name) { return serr("E_SCHEMA_DUPLICATE", &d.fname, &format!("duplicate definition of '{}'", d.name), d.pos, &d.src); }
        if prim(&d.name).is_some() || ["array", "ref", "optional", "nullable"].contains(&d.name.as_str()) { return serr("E_SCHEMA_INVALID", &d.fname, &format!("'{}' is a reserved type name", d.name), d.pos, &d.src); }
        by_name.insert(d.name.clone(), i);
        kinds.push(match d.kind { RawKind::Struct(_) => "struct", RawKind::Enum(..) => "enum", RawKind::Union(_) => "union" });
    }
    let res = |raw: &Raw, d: &RawDecl, top: bool| -> Res<(Ty, bool, bool)> {
        fn go(raw: &Raw, d: &RawDecl, top: bool, by_name: &HashMap<String, usize>, kinds: &[&'static str]) -> Res<(Ty, bool, bool)> {
            match raw {
                Raw::Name(n, pos) => { if let Some(t) = prim(n) { return Ok((t, false, false)); }
                    match by_name.get(n) { None => serr("E_SCHEMA_UNKNOWN_TYPE", &d.fname, &format!("unknown type '{}'", n), *pos, &d.src),
                        Some(&i) => Ok((match kinds[i] { "struct" => Ty::Struct(i), "enum" => Ty::Enum(i), _ => Ty::Union(i) }, false, false)) } }
                Raw::Optional(v, pos) | Raw::Nullable(v, pos) => {
                    if !top { return serr("E_SCHEMA_INVALID", &d.fname, "optional<>/nullable<> is only allowed as a field's outermost type", *pos, &d.src); }
                    let (t, _, _) = go(v, d, false, by_name, kinds)?; let is_opt = matches!(raw, Raw::Optional(..)); Ok((t, is_opt, !is_opt)) }
                Raw::Array(v, _) => { let (t, _, _) = go(v, d, false, by_name, kinds)?; Ok((Ty::Array(Box::new(t)), false, false)) }
                Raw::Ref(v, _) => { let (t, _, _) = go(v, d, false, by_name, kinds)?; Ok((Ty::Ref(Box::new(t)), false, false)) }
            }
        }
        go(raw, d, top, &by_name, &kinds)
    };
    let mut types: Vec<Option<TypeDef>> = (0..decls.len()).map(|_| None).collect();
    let mut order: Vec<usize> = (0..decls.len()).collect(); order.sort_by_key(|&i| kinds[i] == "struct");   // enums/unions first
    for di in order {
        let d = &decls[di];
        match &d.kind {
            RawKind::Enum(base, values) => {
                let (mut names, mut vals) = (HashSet::new(), HashSet::new()); let mut out = vec![];
                for (vn, vv, vpos) in values { if !names.insert(vn.clone()) || !vals.insert(*vv) { return serr("E_SCHEMA_DUPLICATE", &d.fname, &format!("duplicate enum member '{}' or value {}", vn, vv), *vpos, &d.src); } out.push((vn.clone(), *vv as u32)); }
                if values.is_empty() { return serr("E_SCHEMA_INVALID", &d.fname, "enum has no members", d.pos, &d.src); }
                let mx = values.iter().map(|v| v.1).max().unwrap();
                let b = match base.as_deref() { Some("u8") => 1, Some("u16") => 2, Some(_) => 4, None => if mx < 256 { 1 } else if mx < 65536 { 2 } else { 4 } };
                let lim: u64 = match b { 1 => 256, 2 => 65536, _ => 1 << 32 };
                if mx >= lim { return serr("E_SCHEMA_INVALID", &d.fname, &format!("enum value does not fit in u{}", b * 8), d.pos, &d.src); }
                types[di] = Some(TypeDef::Enum { name: d.name.clone(), base: b, values: out });
            }
            RawKind::Union(variants) => {
                let (mut names, mut tags) = (HashSet::new(), HashSet::new()); let mut out = vec![];
                for (vn, tag, raw, vpos, _) in variants {
                    let (t, _, _) = res(raw, d, false)?;
                    if !names.insert(vn.clone()) || !tags.insert(*tag) { return serr("E_SCHEMA_DUPLICATE", &d.fname, &format!("duplicate union variant '{}' or tag {}", vn, tag), *vpos, &d.src); }
                    if matches!(t, Ty::Union(_) | Ty::Any | Ty::Ref(_)) { return serr("E_SCHEMA_INVALID", &d.fname, "union variants cannot have type union, any or ref", *vpos, &d.src); }
                    out.push((vn.clone(), *tag as u8, t));
                }
                if variants.is_empty() { return serr("E_SCHEMA_INVALID", &d.fname, "union has no variants", d.pos, &d.src); }
                types[di] = Some(TypeDef::Union { name: d.name.clone(), variants: out });
            }
            RawKind::Struct(_) => {}
        }
    }
    // enum member lookup for defaults
    let enum_has = |types: &Vec<Option<TypeDef>>, i: usize, n: &str| -> bool { matches!(&types[i], Some(TypeDef::Enum { values, .. }) if values.iter().any(|(x, _)| x == n)) };
    for (di, d) in decls.iter().enumerate() {
        let raws = match &d.kind { RawKind::Struct(f) => f, _ => continue };
        let mut fields: Vec<Field> = vec![]; let mut seen: HashSet<String> = HashSet::new(); let mut last_since = 1usize;
        for rf in raws {
            let bad = |m: &str| -> Res<()> { serr("E_SCHEMA_INVALID", &d.fname, m, rf.pos, &d.src) };
            if !seen.insert(rf.name.clone()) { return serr("E_SCHEMA_DUPLICATE", &d.fname, &format!("duplicate field '{}'", rf.name), rf.pos, &d.src); }
            let (t, opt, nul) = res(&rf.ty, d, true)?;
            let mut f = Field { name: rf.name.clone(), ty: t.clone(), optional: opt, nullable: nul, deprecated: false, since: 1, default: None, attrs: vec![], cons: vec![] };
            for (an, aa) in &rf.attrs {
                if an == "since" { if aa.is_empty() || !aa.chars().all(|c| c.is_ascii_digit()) || aa.parse::<usize>().unwrap_or(0) < 1 { bad("@since needs a positive integer")?; } f.since = aa.parse().unwrap(); }
                else if an == "deprecated" { f.deprecated = true; f.attrs.push((an.clone(), aa.clone())); }
                else { f.attrs.push((an.clone(), aa.clone())); }
            }
            for (an, aa) in &f.attrs {
                if an == "range" || an == "len" {
                    let parts: Vec<&str> = aa.split(',').map(|x| x.trim()).collect();
                    if parts.len() != 2 { bad(&format!("@{} takes two arguments (use _ for unbounded)", an))?; }
                    for x in &parts { if *x != "_" && x.parse::<f64>().is_err() { bad(&format!("@{}: bad number '{}'", an, x))?; } }
                    let num = is_int(&t) || matches!(t, Ty::F32 | Ty::F64);
                    if an == "range" && !num { bad(&format!("{}.{}: @range needs a numeric type", d.name, rf.name))?; }
                    if an == "len" && !matches!(t, Ty::Str | Ty::Bytes | Ty::Array(_)) { bad(&format!("{}.{}: @len needs string, bytes or array", d.name, rf.name))?; }
                    let pv = |x: &str| if x == "_" { None } else { Some(x.parse::<f64>().unwrap()) };
                    f.cons.push((an.clone(), pv(parts[0]), pv(parts[1])));
                }
            }
            if let Ty::Ref(e) = &t { if matches!(**e, Ty::Ref(_)) { bad("ref<ref<..>> is not allowed")?; } }
            if let Some(lit) = &rf.dflt {
                if opt { bad("optional fields cannot have defaults")?; }
                f.default = Some(match (&t, lit) {
                    (_, Lit::Null) => { if !nul && t != Ty::Any { bad("null default needs a nullable<> field")?; } DefaultVal::Null }
                    (t, Lit::Int(v)) if is_int(t) => { if !range_ok(t, *v) { bad(&format!("default {} is not a valid integer of that type", v))?; } DefaultVal::Int(*v) }
                    (t, _) if is_int(t) => { bad("default is not a valid integer of that type")?; unreachable!() }
                    (Ty::F32 | Ty::F64, Lit::Int(v)) => DefaultVal::Float(*v as f64), (Ty::F32 | Ty::F64, Lit::Float(x)) => DefaultVal::Float(*x),
                    (Ty::F32 | Ty::F64, _) => { bad("default must be a number")?; unreachable!() }
                    (Ty::Bool, Lit::Bool(b)) => DefaultVal::Bool(*b), (Ty::Bool, _) => { bad("default must be true or false")?; unreachable!() }
                    (Ty::Str, Lit::Str(s)) => DefaultVal::Str(s.clone()), (Ty::Str, _) => { bad("default must be a string")?; unreachable!() }
                    (Ty::Enum(ei), Lit::Str(s)) if enum_has(&types, *ei, s) => DefaultVal::Str(s.clone()),
                    (Ty::Enum(_), _) => { bad("default must be one of the enum's member names (as a string)")?; unreachable!() }
                    (Ty::Array(_), Lit::Arr) => DefaultVal::Arr, (Ty::Array(_), _) => { bad("only [] is allowed as an array default")?; unreachable!() }
                    (Ty::Any, l) => match l { Lit::Int(v) => DefaultVal::Int(*v), Lit::Float(x) => DefaultVal::Float(*x), Lit::Str(s) => DefaultVal::Str(s.clone()), Lit::Bool(b) => DefaultVal::Bool(*b), Lit::Arr => DefaultVal::Arr, Lit::Null => DefaultVal::Null },
                    _ => { bad("fields of this type cannot have defaults")?; unreachable!() }
                });
            }
            fields.push(f);
        }
        for (rf, f) in raws.iter().zip(&fields) {
            let bad = |m: String| -> Res<()> { serr("E_SCHEMA_INVALID", &d.fname, &m, d.pos, &d.src) };
            let _ = rf;
            if f.since < last_since { bad(format!("field '{}': @since values must not decrease along the declaration order (fields are append-only)", f.name))?; }
            last_since = f.since;
            if f.since > meta.version { bad(format!("field '{}' has @since({}) but the schema version is {}", f.name, f.since, meta.version))?; }
            if f.since > 1 && !(f.optional || f.nullable || f.default.is_some()) { bad(format!("field '{}' added in version {} must be optional, nullable or have a default (old data lacks it)", f.name, f.since))?; }
        }
        if fields.is_empty() || fields[0].since != 1 { return serr("E_SCHEMA_INVALID", &d.fname, &format!("schema '{}' needs at least one field present since version 1", d.name), d.pos, &d.src); }
        types[di] = Some(TypeDef::Struct { name: d.name.clone(), fields });
    }
    let mut root = None;
    if let Some((rn, rpos)) = &meta.root {
        match by_name.get(rn) { Some(&i) if kinds[i] == "struct" => root = Some(i), _ => return serr("E_SCHEMA_UNKNOWN_TYPE", mfname, &format!("root '{}' is not a schema", rn), *rpos, msrc) }
    }
    Ok(Schema::new(meta.namespace.clone(), meta.version, root, types.into_iter().map(|t| t.unwrap()).collect()))
}

/// Compile schema source text. Pass a `loader` to support `import`.
pub fn compile_str(text: &str, filename: &str, loader: Option<Loader>) -> Res<Schema> {
    let (decls, meta) = parse_file(text, filename, loader, &[filename.to_string()])?;
    build(decls, meta)
}
/// Compile a schema file (imports are resolved relative to the importing file).
pub fn compile_file(path: &std::path::Path) -> Res<Schema> {
    let text = std::fs::read_to_string(path).map_err(|e| Error { code: "E_SCHEMA_IMPORT".into(), message: format!("{}: {}", path.display(), e), pos: None, line: None, col: None })?;
    let name = path.to_string_lossy().to_string();
    let loader = |p: &str, from: &str| -> std::io::Result<(String, String)> {
        let full = std::path::Path::new(from).parent().unwrap_or(std::path::Path::new(".")).join(p);
        let norm = full.to_string_lossy().to_string(); Ok((std::fs::read_to_string(&full)?, norm))
    };
    compile_str(&text, &name, Some(&loader))
}

/// Like [`compile_file`], also returning every file that was read (the schema and its imports) so build tools can track dependencies.
pub fn compile_file_deps(path: &std::path::Path) -> Res<(Schema, Vec<std::path::PathBuf>)> {
    let deps = std::cell::RefCell::new(vec![path.to_path_buf()]);
    let text = std::fs::read_to_string(path).map_err(|e| Error { code: "E_SCHEMA_IMPORT".into(), message: format!("{}: {}", path.display(), e), pos: None, line: None, col: None })?;
    let loader = |p: &str, from: &str| -> std::io::Result<(String, String)> {
        let full = std::path::Path::new(from).parent().unwrap_or(std::path::Path::new(".")).join(p);
        deps.borrow_mut().push(full.clone()); Ok((std::fs::read_to_string(&full)?, full.to_string_lossy().to_string()))
    };
    let sc = compile_str(&text, &path.to_string_lossy(), Some(&loader))?;
    Ok((sc, deps.into_inner()))
}
