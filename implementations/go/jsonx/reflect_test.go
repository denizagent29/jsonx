package jsonx

import (
	"bytes"
	"os"
	"testing"
)

// Types exactly as produced by `jsonxc gen go examples/schemas/shop.jxs`.
type Status string
type Payment struct {
	Card *Card
	Iban *string
	Cash *uint32
}
type Card struct {
	Last4 string `jx:"last4"`
	Brand string `jx:"brand"`
}
type Item struct {
	Sku    string   `jx:"sku"`
	Qty    uint32   `jx:"qty"`
	Price  float64  `jx:"price"`
	Tags   []string `jx:"tags"`
	Weight *float32 `jx:"weight"`
}
type Customer struct {
	Id    uint64  `jx:"id"`
	Name  string  `jx:"name"`
	Vip   bool    `jx:"vip"`
	Email *string `jx:"email"`
}
type Receipt struct {
	Id        uint64      `jx:"id"`
	Customers []*Customer `jx:"customers"`
	Items     []*Item     `jx:"items"`
	Status    Status      `jx:"status"`
	Payment   *Payment    `jx:"payment"`
	Note      *string     `jx:"note"`
	Raw       []byte      `jx:"raw"`
	Grid      [][]int16   `jx:"grid"`
	Nick      string      `jx:"nick"`
}
type Order struct {
	Id        uint64      `jx:"id"`
	Customers []*Customer `jx:"customers"`
	Buyer     *Customer   `jx:"buyer"`
	Price     float64     `jx:"price"`
	PriceRef  float64     `jx:"price_ref"`
	Status    Status      `jx:"status"`
}

func sp(s string) *string { return &s }

func TestTypedMapping(t *testing.T) {
	sc, err := LoadSchema([]byte(rd(t, "shop/shop.jxsb")))
	if err != nil {
		t.Fatal(err)
	}
	golden := []byte(rd(t, "shop/receipt.bjsonx"))
	// golden blob (written by the Python reference) -> Go struct
	var r Receipt
	if err := Unmarshal(golden, sc, &r); err != nil {
		t.Fatal(err)
	}
	f32 := float32(1.25)
	want := Receipt{Id: 7, Customers: []*Customer{{Id: 1, Name: "Deniz", Vip: true, Email: sp("d@example.org")}, {Id: 2, Name: "Adil"}},
		Items:  []*Item{{Sku: "A-1", Qty: 3, Price: 9.5, Tags: []string{"x", "y"}, Weight: &f32}, {Sku: "B", Qty: 1, Price: 0.1, Tags: []string{}}},
		Status: "paid", Payment: &Payment{Card: &Card{Last4: "4242", Brand: "visa"}}, Raw: []byte{0, 1, 255}, Grid: [][]int16{{1, -2}, {}}, Nick: "dz"}
	if !deepEq(r, want) {
		t.Fatalf("unmarshal mismatch:\n got %+v\nwant %+v", r, want)
	}
	// Go struct -> blob is byte-identical to the Python golden blob (and to the text path)
	out, err := Marshal(&want, sc, EncodeOptions{})
	if err != nil || !bytes.Equal(out, golden) {
		t.Fatalf("marshal differs from golden (err=%v, %d vs %d bytes)", err, len(out), len(golden))
	}
	// older layout: golden v1 blob read by the v2 struct takes defaults for newer fields
	var old Receipt
	if err := Unmarshal([]byte(rd(t, "shop/receipt_v1.bjsonx")), sc, &old); err != nil {
		t.Fatal(err)
	}
	if old.Nick != "n" || old.Customers[0].Email != nil || old.Note == nil || *old.Note != "n" || old.Status != "draft" {
		t.Fatalf("v1 data: %+v", old)
	}
	v1, err := Marshal(&old, sc, EncodeOptions{Version: 1})
	if err != nil || !bytes.Equal(v1, []byte(rd(t, "shop/receipt_v1.bjsonx"))) {
		t.Fatalf("v1 marshal differs from golden (err=%v)", err)
	}
	// union variants, enum, constraint and error paths
	for _, p := range []*Payment{{Iban: sp("DE00")}, {Cash: func() *uint32 { v := uint32(5); return &v }()}} {
		x := want
		x.Payment = p
		b, err := Marshal(&x, sc, EncodeOptions{})
		if err != nil {
			t.Fatal(err)
		}
		var back Receipt
		if err := Unmarshal(b, sc, &back); err != nil || !deepEq(back, x) {
			t.Fatalf("union roundtrip failed: %v", err)
		}
	}
	bad := want
	bad.Customers = []*Customer{{Id: 1, Name: ""}}
	if _, err := Marshal(&bad, sc, EncodeOptions{}); err == nil || err.(*Error).Code != "E_CONSTRAINT" {
		t.Fatalf("expected E_CONSTRAINT, got %v", err)
	}
	two := want
	two.Payment = &Payment{Iban: sp("x"), Card: &Card{Last4: "1234"}}
	if _, err := Marshal(&two, sc, EncodeOptions{}); err == nil || err.(*Error).Code != "E_TYPE_MISMATCH" {
		t.Fatalf("expected E_TYPE_MISMATCH for two variants, got %v", err)
	}
	var wrong Customer
	if err := Unmarshal(golden, sc, &wrong); err == nil || err.(*Error).Code != "E_TYPE_MISMATCH" {
		t.Fatalf("expected E_TYPE_MISMATCH for wrong destination type, got %v", err)
	}
	// references: the target is copied when reading; marshalling refs is refused
	d, _ := Parse(`{ id: 1, customers: [ { id: 1, name: "A", vip: false }, { id: 2, name: "B", vip: true } ], buyer: @customers.1, price: 2.5, price_ref: @price, status: "shipped" }`)
	ob, err := Encode(d, sc, EncodeOptions{Root: "Order"})
	if err != nil {
		t.Fatal(err)
	}
	var o Order
	if err := Unmarshal(ob, sc, &o); err != nil || o.Buyer == nil || o.Buyer.Name != "B" || !o.Buyer.Vip || o.PriceRef != 2.5 || o.Status != "shipped" {
		t.Fatalf("order: %+v err=%v", o, err)
	}
	if _, err := Marshal(&o, sc, EncodeOptions{}); err == nil || err.(*Error).Code != "E_REF_TARGET" {
		t.Fatalf("expected E_REF_TARGET, got %v", err)
	}
}

func deepEq(a, b interface{}) bool {
	x, e1 := Marshal(a, mustShop(), EncodeOptions{})
	y, e2 := Marshal(b, mustShop(), EncodeOptions{})
	return e1 == nil && e2 == nil && bytes.Equal(x, y)
}

var shopCache *Schema

func mustShop() *Schema {
	if shopCache == nil {
		b, _ := os.ReadFile(V + "shop/shop.jxsb")
		shopCache, _ = LoadSchema(b)
	}
	return shopCache
}
