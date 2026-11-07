//! cargo run --release -p jsonx-derive --example derive_bench
use jsonx_derive::Bjsonx;
use std::time::Instant;

#[derive(Bjsonx, Debug, Clone)] #[bjsonx(schema = "../../../examples/schemas/shop.jxs")] enum Status { Draft, Paid, Shipped }
#[derive(Bjsonx, Debug, Clone)] #[bjsonx(schema = "../../../examples/schemas/shop.jxs")] struct Card { last4: String, brand: String }
#[derive(Bjsonx, Debug, Clone)] #[bjsonx(schema = "../../../examples/schemas/shop.jxs")] enum Payment { Card(Card), Iban(String), Cash(u32) }
#[derive(Bjsonx, Debug, Clone)] #[bjsonx(schema = "../../../examples/schemas/shop.jxs")] struct Item { sku: String, qty: u32, price: f64, tags: Vec<String>, weight: Option<f32> }
#[derive(Bjsonx, Debug, Clone)] #[bjsonx(schema = "../../../examples/schemas/shop.jxs")] struct Customer { id: u64, name: String, vip: bool, email: Option<String> }
#[derive(Bjsonx, Debug, Clone)] #[bjsonx(schema = "../../../examples/schemas/shop.jxs")]
struct Receipt { id: u64, customers: Vec<Customer>, items: Vec<Item>, status: Status, payment: Option<Payment>, note: Option<String>, raw: Vec<u8>, grid: Vec<Vec<i16>>, nick: String }

fn t<F: FnMut()>(mut f: F) -> f64 {
    f(); let mut s = vec![];
    for _ in 0..5 { let mut n = 1u32; loop { let a = Instant::now(); for _ in 0..n { f(); } let e = a.elapsed().as_secs_f64(); if e >= 0.1 || n >= 1 << 14 { s.push(e / n as f64); break; } n *= 2; } }
    s.sort_by(|a, b| a.partial_cmp(b).unwrap()); s[2]
}
fn main() {
    let r = Receipt { id: 1, customers: (0..1000).map(|i| Customer { id: i, name: format!("user_{:06}", i), vip: i % 3 == 0, email: if i % 2 == 0 { Some(format!("u{}@x.org", i)) } else { None } }).collect(),
        items: (0..10000).map(|i| Item { sku: format!("SKU-{}", i), qty: (i % 9) as u32, price: i as f64 * 0.01, tags: vec![format!("t{}", i % 17), "x".repeat((i % 9 + 1) as usize)], weight: if i % 5 == 0 { Some(1.5) } else { None } }).collect(),
        status: Status::Paid, payment: Some(Payment::Iban("DE0000".into())), note: None, raw: vec![7; 64], grid: vec![vec![1, 2, 3]; 10], nick: "n".into() };
    let blob = r.to_blob().unwrap(); let sc = Receipt::schema();
    println!("| operation (Rust --release; 10,000 items + 1,000 customers; blob {} bytes) | ms |\n|---|---:|", blob.len());
    let row = |n: &str, s: f64| println!("| {} | {:.3} |", n, s * 1e3);
    row("derive: to_blob (direct struct -> bytes)", t(|| { std::hint::black_box(r.to_blob().unwrap()); }));
    row("derive: from_blob (bytes -> owned struct)", t(|| { std::hint::black_box(Receipt::from_blob(&blob).unwrap()); }));
    row("generic: to_doc + jsonx::encode (the old path through a Doc)", t(|| { let d = r.to_doc().unwrap(); std::hint::black_box(jsonx::encode(&d, Some(sc), &jsonx::EncodeOptions { root: Some("Receipt".into()), ..Default::default() }).unwrap()); }));
    row("generic: jsonx::decode (bytes -> validated Doc)", t(|| { std::hint::black_box(jsonx::decode(&blob, Some(sc), &Default::default()).unwrap()); }));
    row("derive view: items[5000].sku (zero-copy)", t(|| { let o = Receipt::open(&blob).unwrap(); let v = Receipt::view(&o).unwrap(); if let jsonx::Val::Struct(i) = v.items().unwrap().get(5000).unwrap() { std::hint::black_box(Item::__jx_view(i).unwrap().sku().unwrap()); } }));
}
