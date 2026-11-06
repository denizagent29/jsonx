//! jsonxc: JSONX schema compiler, converter, validator and binary inspector (Rust implementation).
use jsonx::*;
use std::io::{Read, Write};
use std::path::Path;

const EXIT_USAGE: i32 = 1; const EXIT_INVALID: i32 = 2; const EXIT_BINARY: i32 = 3;
const HELP: &str = "jsonxc - JSONX schema compiler, converter, validator and inspector

USAGE: jsonxc <command> [options]
  compile  <schema.jxs> [-o out.jxsb] [--describe]     compile to canonical binary schema (or print a JSON description)
  gen      <schema.jxs> --lang ts|go|rust|python [-o f]  generate type definitions + embedded schema
  encode   <input|-> [-s schema] [-r root] [--version N] [--crc] [-o out]   JSONX text -> BJSONX
  decode   <input|-> [-s schema] [--format dump|json] [-o out]              BJSONX -> canonical dump / JSON
  validate <input|-> [-s schema] [-r root]             validate text (and encodability against a schema)
  inspect  <input|-> [-s schema] [--hex]                header and structure of a .bjsonx file
  compat   <old schema> <new schema>                    check that NEW is a compatible evolution of OLD
  help | --version
Exit codes: 0 ok, 1 usage/IO, 2 invalid text/schema/data, 3 malformed binary.";

struct Args { pos: Vec<String>, opts: std::collections::HashMap<String, String>, flags: std::collections::HashSet<String> }
fn parse_args(a: &[String]) -> Result<Args, String> {
    let mut r = Args { pos: vec![], opts: Default::default(), flags: Default::default() }; let mut i = 0;
    while i < a.len() {
        let x = &a[i];
        match x.as_str() {
            "-o" | "--out" => { r.opts.insert("out".into(), a.get(i + 1).ok_or("-o needs a value")?.clone()); i += 1; }
            "-s" | "--schema" => { r.opts.insert("schema".into(), a.get(i + 1).ok_or("-s needs a value")?.clone()); i += 1; }
            "-r" | "--root" => { r.opts.insert("root".into(), a.get(i + 1).ok_or("-r needs a value")?.clone()); i += 1; }
            "--version" => { r.opts.insert("version".into(), a.get(i + 1).ok_or("--version needs a value")?.clone()); i += 1; }
            "--format" => { r.opts.insert("format".into(), a.get(i + 1).ok_or("--format needs a value")?.clone()); i += 1; }
            "--lang" => { r.opts.insert("lang".into(), a.get(i + 1).ok_or("--lang needs a value")?.clone()); i += 1; }
            "--crc" | "--hex" | "--describe" => { r.flags.insert(x.trim_start_matches('-').to_string()); }
            s if s.starts_with('-') && s != "-" => return Err(format!("unknown option {}", s)),
            _ => r.pos.push(x.clone()),
        }
        i += 1;
    }
    Ok(r)
}
fn read_in(p: &str) -> std::io::Result<Vec<u8>> { if p == "-" { let mut v = vec![]; std::io::stdin().read_to_end(&mut v)?; Ok(v) } else { std::fs::read(p) } }
fn write_out(p: Option<&String>, data: &[u8]) -> std::io::Result<()> { match p { Some(f) if f != "-" => std::fs::write(f, data), _ => std::io::stdout().write_all(data) } }
fn load_schema(p: &str) -> Res<Schema> { let d = std::fs::read(p).map_err(|e| Error { code: "E_SCHEMA_IMPORT".into(), message: format!("{}: {}", p, e), pos: None, line: None, col: None })?; if d.starts_with(b"BJXS") { Schema::from_bytes(&d) } else { compile_file(Path::new(p)) } }
fn arr(b: &[u8]) -> String { b.chunks(16).map(|c| format!("    {}", c.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))).collect::<Vec<_>>().join(",\n") }
fn camel(s: &str) -> String { s.split('_').map(|p| { let mut c = p.chars(); match c.next() { Some(f) => f.to_uppercase().collect::<String>() + c.as_str(), None => String::new() } }).collect() }
fn jq(s: &str) -> String { let mut o = String::from("\""); for c in s.chars() { match c { '"' => o.push_str("\\\""), '\\' => o.push_str("\\\\"), '\n' => o.push_str("\\n"), c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)), c => o.push(c) } } o.push('"'); o }

fn tsname(sc: &Schema, t: &Ty) -> String { match t { Ty::Bool => "boolean".into(), Ty::U64 | Ty::I64 => "bigint | number".into(), Ty::Str => "string".into(), Ty::Bytes => "Uint8Array".into(), Ty::Any => "unknown".into(),
    Ty::Struct(i) | Ty::Enum(i) | Ty::Union(i) => sc.types[*i].name().into(), Ty::Array(e) => format!("Array<{}>", tsname(sc, e)), Ty::Ref(e) => tsname(sc, e), _ => "number".into() } }
fn goname(sc: &Schema, t: &Ty) -> String { match t { Ty::Bool => "bool".into(), Ty::U8 => "uint8".into(), Ty::U16 => "uint16".into(), Ty::U32 => "uint32".into(), Ty::U64 => "uint64".into(), Ty::I8 => "int8".into(), Ty::I16 => "int16".into(), Ty::I32 => "int32".into(), Ty::I64 => "int64".into(),
    Ty::F32 => "float32".into(), Ty::F64 => "float64".into(), Ty::Str => "string".into(), Ty::Bytes => "[]byte".into(), Ty::Any => "interface{}".into(), Ty::Struct(i) => format!("*{}", sc.types[*i].name()), Ty::Enum(i) | Ty::Union(i) => sc.types[*i].name().into(),
    Ty::Array(e) => format!("[]{}", goname(sc, e)), Ty::Ref(e) => goname(sc, e) } }
fn rsname(sc: &Schema, t: &Ty) -> String { match t { Ty::Bool => "bool".into(), Ty::U8 => "u8".into(), Ty::U16 => "u16".into(), Ty::U32 => "u32".into(), Ty::U64 => "u64".into(), Ty::I8 => "i8".into(), Ty::I16 => "i16".into(), Ty::I32 => "i32".into(), Ty::I64 => "i64".into(),
    Ty::F32 => "f32".into(), Ty::F64 => "f64".into(), Ty::Str => "String".into(), Ty::Bytes => "Vec<u8>".into(), Ty::Any => "jsonx::Doc".into(), Ty::Struct(i) | Ty::Enum(i) | Ty::Union(i) => sc.types[*i].name().into(),
    Ty::Array(e) => format!("Vec<{}>", rsname(sc, e)), Ty::Ref(e) => format!("jsonx::Id /* ref<{}> */", rsname(sc, e)) } }
fn pyname(sc: &Schema, t: &Ty) -> String { match t { Ty::Bool => "bool".into(), Ty::F32 | Ty::F64 => "float".into(), Ty::Str => "str".into(), Ty::Bytes => "bytes".into(), Ty::Any => "Any".into(), Ty::Struct(i) | Ty::Enum(i) | Ty::Union(i) => format!("'{}'", sc.types[*i].name()),
    Ty::Array(e) => format!("List[{}]", pyname(sc, e)), Ty::Ref(e) => pyname(sc, e), _ => "int".into() } }

fn gen(sc: &Schema, lang: &str) -> Result<String, String> {
    let blob = sc.to_bytes(); let ns = if sc.namespace.is_empty() { "schema" } else { &sc.namespace }; let mut o: Vec<String> = vec![];
    match lang {
        "ts" => {
            o.push(format!("// Generated by jsonxc from namespace {} (schema version {}). Do not edit.", ns, sc.version));
            o.push(format!("export const SCHEMA_VERSION = {};\nexport const SCHEMA_BJXS = new Uint8Array([\n{}\n]);\n", sc.version, arr(&blob)));
            for t in &sc.types { match t {
                TypeDef::Enum { name, values, .. } => o.push(format!("export type {} = {};", name, values.iter().map(|(n, _)| jq(n)).collect::<Vec<_>>().join(" | "))),
                TypeDef::Union { name, variants } => o.push(format!("export type {} = {};", name, variants.iter().map(|(n, _, ty)| format!("{{ {}: {} }}", n, tsname(sc, ty))).collect::<Vec<_>>().join(" | "))),
                TypeDef::Struct { name, fields } => { o.push(format!("export interface {} {{", name));
                    for f in fields { let q = if f.optional { "?" } else { "" }; let n = if f.nullable || f.optional { " | null" } else { "" }; o.push(format!("  {}{}: {}{};{}", f.name, q, tsname(sc, &f.ty), n, if f.deprecated { "  // deprecated" } else { "" })); }
                    o.push("}".into()); } } }
            o.push("\n// Decode with: import { loadSchema, decode, view } from 'jsonx'; const sc = loadSchema(SCHEMA_BJXS);".into());
        }
        "go" => {
            o.push(format!("// Code generated by jsonxc from namespace {} (schema version {}). DO NOT EDIT.\npackage {}\n", ns, sc.version, ns.split('.').last().unwrap()));
            o.push(format!("const SchemaVersion = {}\n\nvar SchemaBJXS = []byte{{\n{},\n}}\n", sc.version, arr(&blob)));
            for t in &sc.types { match t {
                TypeDef::Enum { name, values, .. } => { o.push(format!("type {} string\n\nconst (", name)); for (n, _) in values { o.push(format!("\t{}{} {} = {}", name, camel(n), name, jq(n))); } o.push(")\n".into()); }
                TypeDef::Union { name, variants } => { o.push(format!("// {} holds exactly one variant; the others are nil.\ntype {} struct {{", name, name)); for (n, _, ty) in variants { o.push(format!("\t{} *{}", camel(n), goname(sc, ty).trim_start_matches('*'))); } o.push("}\n".into()); }
                TypeDef::Struct { name, fields } => { o.push(format!("type {} struct {{", name));
                    for f in fields { let mut ty = goname(sc, &f.ty); if (f.optional || f.nullable) && !(ty.starts_with('*') || ty.starts_with("[]") || ty.starts_with("interface")) { ty = format!("*{}", ty); } o.push(format!("\t{} {} `jx:\"{}\"`", camel(&f.name), ty, f.name)); }
                    o.push("}\n".into()); } } }
        }
        "rust" => {
            o.push(format!("// Generated by jsonxc from namespace {} (schema version {}). Do not edit.\n#![allow(dead_code)]\n", ns, sc.version));
            o.push(format!("pub const SCHEMA_VERSION: usize = {};\npub const SCHEMA_BJXS: &[u8] = &[\n{},\n];\n\n/// Load the embedded schema once and reuse it for every request.\npub fn schema() -> jsonx::Schema {{ jsonx::Schema::from_bytes(SCHEMA_BJXS).expect(\"embedded schema is valid\") }}\n", sc.version, arr(&blob)));
            for t in &sc.types { match t {
                TypeDef::Enum { name, values, .. } => o.push(format!("#[derive(Debug, Clone, PartialEq)]\npub enum {} {{\n{}}}\n", name, values.iter().map(|(n, _)| format!("    {},\n", camel(n))).collect::<String>())),
                TypeDef::Union { name, variants } => o.push(format!("#[derive(Debug, Clone)]\npub enum {} {{\n{}}}\n", name, variants.iter().map(|(n, _, ty)| format!("    {}({}),\n", camel(n), rsname(sc, ty))).collect::<String>())),
                TypeDef::Struct { name, fields } => { o.push(format!("#[derive(Debug, Clone)]\npub struct {} {{", name));
                    for f in fields { let ty = rsname(sc, &f.ty); let ty = if f.optional || f.nullable { format!("Option<{}>", ty) } else { ty }; o.push(format!("    pub {}: {},", f.name, ty)); } o.push("}\n".into()); } } }
        }
        "python" => {
            o.push(format!("# Generated by jsonxc from namespace {} (schema version {}). Do not edit.\nfrom __future__ import annotations\nfrom dataclasses import dataclass\nfrom typing import Any, List, Optional\n", ns, sc.version));
            o.push(format!("SCHEMA_VERSION = {}\nSCHEMA_BJXS = bytes([\n{}\n])\n", sc.version, arr(&blob)));
            for t in &sc.types { match t {
                TypeDef::Enum { name, values, .. } => o.push(format!("{} = str  # one of: {}\n", name, values.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", "))),
                TypeDef::Union { name, variants } => o.push(format!("{} = dict  # exactly one key of: {}\n", name, variants.iter().map(|(n, _, _)| n.as_str()).collect::<Vec<_>>().join(", "))),
                TypeDef::Struct { name, fields } => { o.push(format!("@dataclass\nclass {}:", name));
                    for f in fields { let ty = pyname(sc, &f.ty); let opt = f.optional || f.nullable; o.push(format!("    {}: {}{}", f.name, if opt { format!("Optional[{}]", ty) } else { ty }, if opt { " = None" } else { "" })); }
                    o.push(String::new()); } } }
        }
        _ => return Err(format!("unknown language {}", lang)),
    }
    Ok(o.join("\n") + "\n")
}

fn describe(sc: &Schema) -> String {
    fn td(sc: &Schema, t: &Ty) -> String { match t { Ty::Struct(i) | Ty::Enum(i) | Ty::Union(i) => format!("{{\"kind\":{},\"name\":{}}}", jq(&format!("{:?}", t).split('(').next().unwrap().to_lowercase()), jq(sc.types[*i].name())),
        Ty::Array(e) => format!("{{\"kind\":\"array\",\"of\":{}}}", td(sc, e)), Ty::Ref(e) => format!("{{\"kind\":\"ref\",\"of\":{}}}", td(sc, e)), o => format!("{{\"kind\":{}}}", jq(&format!("{:?}", o).to_lowercase())) } }
    let mut ts = vec![];
    for t in &sc.types { ts.push(match t {
        TypeDef::Struct { name, fields } => format!("{{\"kind\":\"struct\",\"name\":{},\"fields\":[{}]}}", jq(name), fields.iter().map(|f| format!("{{\"name\":{},\"type\":{},\"optional\":{},\"nullable\":{},\"since\":{},\"deprecated\":{},\"attrs\":[{}]}}", jq(&f.name), td(sc, &f.ty), f.optional, f.nullable, f.since, f.deprecated, f.attrs.iter().map(|(n, a)| format!("[{},{}]", jq(n), jq(a))).collect::<Vec<_>>().join(","))).collect::<Vec<_>>().join(",")),
        TypeDef::Enum { name, base, values } => format!("{{\"kind\":\"enum\",\"name\":{},\"base\":{},\"values\":{{{}}}}}", jq(name), base * 8, values.iter().map(|(n, v)| format!("{}:{}", jq(n), v)).collect::<Vec<_>>().join(",")),
        TypeDef::Union { name, variants } => format!("{{\"kind\":\"union\",\"name\":{},\"variants\":[{}]}}", jq(name), variants.iter().map(|(n, tag, ty)| format!("{{\"name\":{},\"tag\":{},\"type\":{}}}", jq(n), tag, td(sc, ty))).collect::<Vec<_>>().join(",")) }); }
    format!("{{\"namespace\":{},\"version\":{},\"root\":{},\"id\":\"{:016x}\",\"fingerprint\":\"{:016x}\",\"types\":[{}]}}\n", jq(&sc.namespace), sc.version, sc.root.map_or("null".into(), |r| jq(sc.types[r].name())), sc.id(), sc.fingerprint(), ts.join(","))
}

fn compat(old: &Schema, new: &Schema) -> Vec<String> {
    let mut p = vec![];
    for t in &old.types {
        let n = match new.types.iter().find(|x| x.name() == t.name()) { Some(n) => n, None => { p.push(format!("type {} was removed", t.name())); continue; } };
        match (t, n) {
            (TypeDef::Struct { name, fields: of }, TypeDef::Struct { fields: nf, .. }) => {
                for (i, f) in of.iter().enumerate() {
                    let g = match nf.get(i) { Some(g) if g.name == f.name => g, _ => { p.push(format!("{}.{}: field removed or reordered (use @deprecated instead)", name, f.name)); continue; } };
                    if old.type_name(&f.ty) != new.type_name(&g.ty) { p.push(format!("{}.{}: type changed {} -> {}", name, f.name, old.type_name(&f.ty), new.type_name(&g.ty))); }
                    if (f.optional, f.nullable) != (g.optional, g.nullable) { p.push(format!("{}.{}: optional/nullable changed (layout change)", name, f.name)); }
                }
                for g in nf.iter().skip(of.len()) { if g.since <= old.version { p.push(format!("{}.{}: new field must have @since({}) or later", name, g.name, old.version + 1)); } }
            }
            (TypeDef::Enum { name, base, values }, TypeDef::Enum { base: nb, values: nv, .. }) => {
                for (n, v) in values { if !nv.iter().any(|(a, b)| a == n && b == v) { p.push(format!("enum {}: member {}={} changed or removed", name, n, v)); } }
                if nb != base { p.push(format!("enum {}: storage size changed", name)); }
            }
            (TypeDef::Union { name, variants }, TypeDef::Union { variants: nv, .. }) => { for (n, tag, _) in variants { if !nv.iter().any(|(a, b, _)| a == n && b == tag) { p.push(format!("union {}: variant {}={} changed or removed", name, n, tag)); } } }
            _ => p.push(format!("{}: kind changed", t.name())),
        }
    }
    if new.version < old.version { p.push("schema version decreased".into()); }
    p
}

fn json_tree(d: &Doc, id: Id, o: &mut String) {
    match &d.nodes[id].kind {
        Kind::Null => o.push_str("null"), Kind::Bool(b) => o.push_str(if *b { "true" } else { "false" }), Kind::Int(i) => o.push_str(&i.to_string()),
        Kind::Float(f) => o.push_str(&format!("{:?}", f)), Kind::F32(f) => o.push_str(&format!("{:?}", f)), Kind::Str(s) => o.push_str(&jq(s)),
        Kind::Bytes(b) => { o.push('"'); o.push_str(&{ let mut s = String::new(); const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"; for c in b.chunks(3) { let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32; s.push(A[(n >> 18) as usize & 63] as char); s.push(A[(n >> 12) as usize & 63] as char); s.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' }); s.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' }); } s }); o.push('"'); }
        Kind::Ref => o.push_str(&format!("{{\"$ref\":{}}}", jq(&d.path(d.nodes[id].target.unwrap())))),
        Kind::Arr(a) => { o.push('['); for (i, c) in a.iter().enumerate() { if i > 0 { o.push(','); } json_tree(d, *c, o); } o.push(']'); }
        Kind::Obj(m) => { o.push('{'); for (i, (k, c)) in m.iter().enumerate() { if i > 0 { o.push(','); } o.push_str(&jq(k)); o.push(':'); json_tree(d, *c, o); } o.push('}'); }
    }
}

fn inspect(data: &[u8], sc: Option<&Schema>, hex: bool) -> String {
    let n = data.len(); let mut o = vec![];
    if n < 28 { return "too short".into(); }
    let u32_ = |p: usize| u32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]);
    let flags = u16::from_le_bytes([data[4], data[5]]); let mut sid = [0u8; 8]; sid.copy_from_slice(&data[8..16]); let sid = u64::from_le_bytes(sid);
    o.push(format!("BJSONX v{}  {} bytes", data[3], n)); o.push(format!("  flags      0x{:04x}{}", flags, if flags & 1 != 0 { " (crc32 trailer)" } else { "" }));
    o.push(format!("  schema id  {:016x}{}", sid, if sid == 0 { " (dynamic)" } else { "" })); o.push(format!("  schema ver {}", u32_(16))); o.push(format!("  root       @{}", u32_(20))); o.push(format!("  length     {}", u32_(24)));
    match decode(data, sc, &ReadOptions::default()) { Ok(d) => { o.push("  decoded    OK".into()); o.push(format!("  value      {}", d.dump())); } Err(e) => o.push(format!("  decode     FAILED: {}", e)) }
    if hex { for (i, row) in data.chunks(16).enumerate() { o.push(format!("  {:08x}  {:<48} {}", i * 16, row.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "), row.iter().map(|&b| if (32..127).contains(&b) { b as char } else { '.' }).collect::<String>())); } }
    o.join("\n")
}

fn run(argv: Vec<String>) -> Result<i32, Error> {
    let cmd = argv.first().map(|s| s.as_str()).unwrap_or("help");
    if cmd == "help" || cmd == "-h" || cmd == "--help" { println!("{}", HELP); return Ok(0); }
    if cmd == "--version" { println!("jsonxc {}", env!("CARGO_PKG_VERSION")); return Ok(0); }
    let a = match parse_args(&argv[1..]) { Ok(a) => a, Err(e) => { eprintln!("jsonxc: {}", e); return Ok(EXIT_USAGE); } };
    let io = |e: std::io::Error| Error { code: "E_IO".into(), message: e.to_string(), pos: None, line: None, col: None };
    let need = |n: usize| -> Option<i32> { if a.pos.len() < n { eprintln!("jsonxc {}: missing argument\n\n{}", cmd, HELP); Some(EXIT_USAGE) } else { None } };
    let schema = match a.opts.get("schema") { Some(p) => Some(load_schema(p)?), None => None };
    match cmd {
        "compile" => { if let Some(c) = need(1) { return Ok(c); } let sc = compile_file(Path::new(&a.pos[0]))?;
            if a.flags.contains("describe") { write_out(a.opts.get("out"), describe(&sc).as_bytes()).map_err(io)?; }
            else { let out = a.opts.get("out").cloned().unwrap_or_else(|| Path::new(&a.pos[0]).with_extension("jxsb").to_string_lossy().to_string()); std::fs::write(&out, sc.to_bytes()).map_err(io)?; eprintln!("compiled {} types, schema id {:016x}, version {}", sc.types.len(), sc.id(), sc.version); } }
        "gen" => { if let Some(c) = need(1) { return Ok(c); } let sc = compile_file(Path::new(&a.pos[0]))?; let lang = a.opts.get("lang").map(|s| s.as_str()).unwrap_or("");
            match gen(&sc, lang) { Ok(s) => write_out(a.opts.get("out"), s.as_bytes()).map_err(io)?, Err(e) => { eprintln!("jsonxc: {}", e); return Ok(EXIT_USAGE); } } }
        "encode" => { if let Some(c) = need(1) { return Ok(c); } let text = String::from_utf8(read_in(&a.pos[0]).map_err(io)?).map_err(|_| Error { code: "E_INVALID_UTF8".into(), message: "input is not valid UTF-8".into(), pos: None, line: None, col: None })?;
            let doc = parse(&text)?; let o = EncodeOptions { root: a.opts.get("root").cloned(), version: a.opts.get("version").and_then(|v| v.parse().ok()), checksum: a.flags.contains("crc") };
            write_out(a.opts.get("out"), &encode(&doc, schema.as_ref(), &o)?).map_err(io)?; }
        "decode" => { if let Some(c) = need(1) { return Ok(c); } let data = read_in(&a.pos[0]).map_err(io)?; let doc = decode(&data, schema.as_ref(), &ReadOptions::default())?;
            let out = if a.opts.get("format").map(|s| s.as_str()) == Some("json") { let mut s = String::new(); json_tree(&doc, doc.root, &mut s); s + "\n" } else { doc.dump() + "\n" }; write_out(a.opts.get("out"), out.as_bytes()).map_err(io)?; }
        "validate" => { if let Some(c) = need(1) { return Ok(c); } let text = String::from_utf8_lossy(&read_in(&a.pos[0]).map_err(io)?).into_owned(); let doc = parse(&text)?;
            if let Some(sc) = &schema { encode(&doc, Some(sc), &EncodeOptions { root: a.opts.get("root").cloned(), ..Default::default() })?; } eprintln!("ok"); }
        "inspect" => { if let Some(c) = need(1) { return Ok(c); } println!("{}", inspect(&read_in(&a.pos[0]).map_err(io)?, schema.as_ref(), a.flags.contains("hex"))); }
        "compat" => { if let Some(c) = need(2) { return Ok(c); } let probs = compat(&load_schema(&a.pos[0])?, &load_schema(&a.pos[1])?); for p in &probs { println!("incompatible: {}", p); } return Ok(if probs.is_empty() { 0 } else { EXIT_INVALID }); }
        _ => { eprintln!("jsonxc: unknown command '{}'\n\n{}", cmd, HELP); return Ok(EXIT_USAGE); }
    }
    Ok(0)
}
fn main() {
    let code = match run(std::env::args().skip(1).collect()) {
        Ok(c) => c,
        Err(e) if e.code == "E_IO" => { eprintln!("jsonxc: {}", e.message); EXIT_USAGE }
        Err(e) => { eprintln!("{}", e); if e.code.starts_with("E_BIN_") { EXIT_BINARY } else { EXIT_INVALID } }
    };
    std::process::exit(code);
}
