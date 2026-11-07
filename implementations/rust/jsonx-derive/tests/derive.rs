use jsonx_derive::Bjsonx;

#[derive(Bjsonx, Debug, Clone, PartialEq)]
#[bjsonx(schema = "../../../examples/schemas/shop.jxs")]
enum Status { Draft, Paid, Shipped }

#[derive(Bjsonx, Debug, Clone, PartialEq)]
#[bjsonx(schema = "../../../examples/schemas/shop.jxs")]
struct Card { last4: String, brand: String }

#[derive(Bjsonx, Debug, Clone, PartialEq)]
#[bjsonx(schema = "../../../examples/schemas/shop.jxs")]
enum Payment { Card(Card), Iban(String), Cash(u32) }

#[derive(Bjsonx, Debug, Clone, PartialEq)]
#[bjsonx(schema = "../../../examples/schemas/shop.jxs")]
struct Item { sku: String, qty: u32, price: f64, tags: Vec<String>, weight: Option<f32> }

#[derive(Bjsonx, Debug, Clone, PartialEq)]
#[bjsonx(schema = "../../../examples/schemas/shop.jxs")]
struct Customer { id: u64, name: String, vip: bool, email: Option<String> }

#[derive(Bjsonx, Debug, Clone, PartialEq)]
#[bjsonx(schema = "../../../examples/schemas/shop.jxs")]
struct Receipt { id: u64, customers: Vec<Customer>, items: Vec<Item>, status: Status, payment: Option<Payment>, note: Option<String>, raw: Vec<u8>, grid: Vec<Vec<i16>>, nick: String }

#[derive(Bjsonx, Debug, Clone, PartialEq)]
#[bjsonx(schema = "../../../examples/schemas/shop.jxs")]
struct Order { id: u64, customers: Vec<Customer>, buyer: Customer, price: f64, price_ref: f64, status: Status }

fn sample() -> Receipt {
    Receipt { id: 7, customers: vec![Customer { id: 1, name: "Deniz".into(), vip: true, email: Some("d@example.org".into()) }, Customer { id: 2, name: "Adil".into(), vip: false, email: None }],
        items: vec![Item { sku: "A-1".into(), qty: 3, price: 9.5, tags: vec!["x".into(), "y".into()], weight: Some(1.25) }, Item { sku: "B".into(), qty: 1, price: 0.1, tags: vec![], weight: None }],
        status: Status::Paid, payment: Some(Payment::Card(Card { last4: "4242".into(), brand: "visa".into() })), note: None, raw: vec![0, 1, 255], grid: vec![vec![1, -2], vec![]], nick: "dz".into() }
}
const TEXT: &str = r#"{ id: 7,
  customers: [ { id: 1, name: "Deniz", vip: true, email: "d@example.org" }, { id: 2, name: "Adil", vip: false } ],
  items: [ { sku: "A-1", qty: 3, price: 9.5, tags: ["x", "y"], weight: 1.25 }, { sku: "B", qty: 1, price: 0.1, tags: [] } ],
  status: "paid", payment: { card: { last4: "4242", brand: "visa" } }, note: null, raw: "AAH/", grid: [[1, -2], []], nick: "dz" }"#;

#[test]
fn encode_is_byte_identical_to_the_text_path() {
    let via_text = jsonx::encode(&jsonx::parse(TEXT).unwrap(), Some(Receipt::schema()), &Default::default()).unwrap();
    assert_eq!(sample().to_blob().unwrap(), via_text);
    let crc = sample().to_blob_with(None, true).unwrap();
    assert_eq!(crc, jsonx::encode(&jsonx::parse(TEXT).unwrap(), Some(Receipt::schema()), &jsonx::EncodeOptions { checksum: true, ..Default::default() }).unwrap());
}
#[test]
fn roundtrip_owned() {
    let r = sample(); let back = Receipt::from_blob(&r.to_blob().unwrap()).unwrap(); assert_eq!(back, r);
    let r2 = Receipt { payment: Some(Payment::Iban("DE00".into())), note: Some("hi".into()), ..sample() }; assert_eq!(Receipt::from_blob(&r2.to_blob().unwrap()).unwrap(), r2);
    let r3 = Receipt { payment: Some(Payment::Cash(5)), ..sample() }; assert_eq!(Receipt::from_blob(&r3.to_blob().unwrap()).unwrap(), r3);
}
#[test]
fn zero_copy_view() {
    let blob = sample().to_blob().unwrap(); let open = Receipt::open(&blob).unwrap(); let v = Receipt::view(&open).unwrap();
    assert_eq!(v.id().unwrap(), 7); assert_eq!(v.nick().unwrap(), "dz"); assert_eq!(v.raw().unwrap(), &[0u8, 1, 255][..]);
    let cs = v.customers().unwrap(); assert_eq!(cs.len(), 2);
    if let jsonx::Val::Struct(c) = cs.get(1).unwrap() { let cv = Customer::__jx_view(c).unwrap(); assert_eq!(cv.name().unwrap(), "Adil"); assert_eq!(cv.email().unwrap(), None); assert!(!cv.vip().unwrap()); } else { panic!() }
    assert!(matches!(v.note().unwrap(), None)); assert!(v.payment().unwrap().is_some());
    // the returned &str points into the blob itself (no copy)
    let s: &str = v.nick().unwrap(); let (lo, hi) = (blob.as_ptr() as usize, blob.as_ptr() as usize + blob.len()); assert!((lo..hi).contains(&(s.as_ptr() as usize)));
}
#[test]
fn references_decode_and_view() {
    let text = r#"{ id: 1, customers: [ { id: 1, name: "A", vip: false }, { id: 2, name: "B", vip: true } ], buyer: @customers.1, price: 2.5, price_ref: @price, status: "shipped" }"#;
    let o = jsonx::EncodeOptions { root: Some("Order".into()), ..Default::default() };
    let blob = jsonx::encode(&jsonx::parse(text).unwrap(), Some(Order::schema()), &o).unwrap();
    // owned decode follows the references (copies the targets)
    let order = Order::from_blob(&blob).unwrap();
    assert_eq!(order.buyer, Customer { id: 2, name: "B".into(), vip: true, email: None }); assert_eq!(order.price_ref, 2.5); assert_eq!(order.status, Status::Shipped);
    // views navigate through the pointer without copying
    let open = Order::open(&blob).unwrap(); let v = Order::view(&open).unwrap();
    assert_eq!(v.buyer().unwrap().name().unwrap(), "B"); assert_eq!(v.price_ref().unwrap(), 2.5);
    // a struct with ref fields cannot be encoded from plain values
    assert_eq!(order.to_blob().unwrap_err().code, "E_REF_TARGET");
    // a ref<f64> pointing at an integer node is rejected by the encoder with a type error
    let bad = text.replace("price_ref: @price", "price_ref: @id");
    assert_eq!(jsonx::encode(&jsonx::parse(&bad).unwrap(), Some(Order::schema()), &o).unwrap_err().code, "E_REF_TARGET");
}
#[test]
fn old_and_new_versions() {
    let r = sample(); let v1 = r.to_blob_with(Some(1), false).unwrap();
    let back = Receipt::from_blob(&v1).unwrap();           // fields added in v2 take their defaults
    assert_eq!(back.nick, "n"); assert_eq!(back.customers[0].email, None); assert_eq!(back.customers[0].name, "Deniz");
}
#[test]
fn malformed_input_is_an_error_not_a_panic() {
    let blob = sample().to_blob().unwrap();
    for i in 0..blob.len() { let mut m = blob.clone(); m[i] ^= 0xA5; let _ = Receipt::from_blob(&m); }
    for n in 0..blob.len() { let _ = Receipt::from_blob(&blob[..n]); }
    assert_eq!(Receipt::from_blob(b"nope").unwrap_err().code, "E_BIN_TRUNCATED");
}

struct Lcg(u64);
impl Lcg { fn n(&mut self, m: u64) -> u64 { self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); (self.0 >> 33) % m } fn s(&mut self) -> String { let l = self.n(6); (0..l).map(|_| ['a', 'é', 'Z', '日', '0', '"'][self.n(6) as usize]).collect() } }
fn random_receipt(r: &mut Lcg) -> Receipt {
    let customers = (0..r.n(4)).map(|_| Customer { id: r.n(1 << 40), name: format!("n{}", r.s()), vip: r.n(2) == 0, email: if r.n(2) == 0 { Some(r.s()) } else { None } }).collect();
    let items = (0..r.n(5)).map(|_| Item { sku: r.s(), qty: r.n(100) as u32, price: r.n(100000) as f64 / 7.0, tags: (0..r.n(3)).map(|_| r.s()).collect(), weight: if r.n(2) == 0 { Some(r.n(1000) as f32 / 3.0) } else { None } }).collect();
    let payment = match r.n(4) { 0 => None, 1 => Some(Payment::Cash(r.n(1 << 31) as u32)), 2 => Some(Payment::Iban(r.s())), _ => Some(Payment::Card(Card { last4: "1234".into(), brand: r.s() })) };
    Receipt { id: r.n(u64::MAX), customers, items, status: [Status::Draft, Status::Paid, Status::Shipped][r.n(3) as usize].clone(), payment, note: if r.n(2) == 0 { Some(r.s()) } else { None },
        raw: (0..r.n(8)).map(|_| r.n(256) as u8).collect(), grid: (0..r.n(3)).map(|_| (0..r.n(4)).map(|_| r.n(65536) as i64 as i16).collect()).collect(), nick: r.s() }
}
#[test]
fn direct_encoder_equals_doc_encoder_on_random_values() {
    let mut r = Lcg(2024);
    for i in 0..500 {
        let v = random_receipt(&mut r);
        for ver in [1usize, 2] {
            let direct = v.to_blob_with(Some(ver), i % 2 == 0).unwrap();
            let doc = v.to_doc_with(Some(ver)).unwrap();
            let via_doc = jsonx::encode(&doc, Some(Receipt::schema()), &jsonx::EncodeOptions { root: Some("Receipt".into()), version: Some(ver), checksum: i % 2 == 0 }).unwrap();
            assert_eq!(direct, via_doc, "case {} version {}", i, ver);
        }
        assert_eq!(Receipt::from_blob(&v.to_blob().unwrap()).unwrap(), v);
    }
}
#[test]
fn direct_encoder_enforces_constraints_like_the_text_encoder() {
    let bad_name = Receipt { customers: vec![Customer { id: 1, name: "".into(), vip: false, email: None }], ..sample() };                     // @len(1, 64)
    assert_eq!(bad_name.to_blob().unwrap_err().code, "E_CONSTRAINT");
    let bad_card = Receipt { payment: Some(Payment::Card(Card { last4: "123".into(), brand: "x".into() })), ..sample() };                       // @len(4, 4) inside a union payload
    assert_eq!(bad_card.to_blob().unwrap_err().code, "E_CONSTRAINT");
    let long = Receipt { customers: vec![Customer { id: 1, name: "x".repeat(65), vip: false, email: None }], ..sample() };
    assert_eq!(long.to_blob().unwrap_err().code, "E_CONSTRAINT");
    assert_eq!(sample().to_blob_with(Some(9), false).unwrap_err().code, "E_SCHEMA_INVALID");
}

#[test]
fn matches_the_python_golden_blob() {
    // written by the Python reference implementation (tests/vectors/shop); Go's Marshal is checked against the same file
    let golden = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/vectors/shop/receipt.bjsonx")).unwrap();
    assert_eq!(sample().to_blob().unwrap(), golden);
    assert_eq!(Receipt::from_blob(&golden).unwrap(), sample());
    let v1 = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/vectors/shop/receipt_v1.bjsonx")).unwrap();
    let old = Receipt::from_blob(&v1).unwrap();
    assert_eq!((old.nick.as_str(), old.note.as_deref(), old.status.clone()), ("n", Some("n"), Status::Draft));
    assert_eq!(old.to_blob_with(Some(1), false).unwrap(), v1);
}
