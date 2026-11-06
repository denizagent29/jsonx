use jsonx::*;
use std::time::Instant;
fn t<F: FnMut()>(mut f: F) -> f64 {
    f(); let mut s = vec![];
    for _ in 0..5 { let mut n = 1u32; loop { let a = Instant::now(); for _ in 0..n { f(); } let e = a.elapsed().as_secs_f64(); if e >= 0.08 || n >= 1 << 14 { s.push(e / n as f64); break; } n *= 2; } }
    s.sort_by(|a, b| a.partial_cmp(b).unwrap()); s[2]
}
fn main() {
    let d = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../benchmarks/data/");
    let xtext = std::fs::read_to_string(format!("{}users.jsonx", d)).expect("run benchmarks/gen_data.py first");
    let sc = Schema::from_bytes(&std::fs::read(format!("{}users.jxsb", d)).unwrap()).unwrap();
    let doc = parse(&xtext).unwrap(); let o = EncodeOptions::default(); let ro = ReadOptions::default();
    let bt = encode(&doc, Some(&sc), &o).unwrap(); let bd = encode(&doc, None, &o).unwrap();
    println!("| operation (Rust, --release) | ms | size (bytes) |\n|---|---:|---:|");
    let row = |n: &str, ms: f64, s: String| println!("| {} | {:.2} | {} |", n, ms * 1e3, s);
    row("JSONX parse (text, with resolver)", t(|| { std::hint::black_box(parse(&xtext).unwrap()); }), xtext.len().to_string());
    row("BJSONX dynamic encode", t(|| { std::hint::black_box(encode(&doc, None, &o).unwrap()); }), bd.len().to_string());
    row("BJSONX dynamic decode (to Doc)", t(|| { std::hint::black_box(decode(&bd, None, &ro).unwrap()); }), String::new());
    row("BJSONX typed encode", t(|| { std::hint::black_box(encode(&doc, Some(&sc), &o).unwrap()); }), bt.len().to_string());
    row("BJSONX typed decode (to Doc)", t(|| { std::hint::black_box(decode(&bt, Some(&sc), &ro).unwrap()); }), String::new());
    row("BJSONX typed view: users[5000].name (zero-copy)", t(|| {
        let b = Blob::open(&bt, Some(&sc), &ro).unwrap();
        if let Val::Struct(r) = b.root().unwrap() { if let Val::Array(a) = r.get("users").unwrap() { if let Val::Struct(u) = a.get(5000).unwrap() { std::hint::black_box(u.get("name").unwrap()); } } }
    }), String::new());
}
