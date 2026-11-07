#!/usr/bin/env python3
"""Generates the shared golden test vectors from the Python reference implementation.
The inputs below are the source of truth; the .dump/.bjsonx/.err files are derived and committed."""
import os, sys, shutil, struct, zlib
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "implementations", "python"))
import jsonx
from jsonx import JsonxError

V = os.path.join(HERE, "vectors")
shutil.rmtree(V, ignore_errors=True)
for d in ("text/valid", "text/errors", "typed", "typed/errors", "binary/malformed", "compat", "schema", "schema/errors", "schema/ok"):
    os.makedirs(os.path.join(V, d))

def w(path, data):
    with open(os.path.join(V, path), "wb" if isinstance(data, bytes) else "w", **({} if isinstance(data, bytes) else {"encoding": "utf-8", "newline": ""})) as f:
        f.write(data)

VALID = {
"scalars": '{a:null,b:true,c:false,i:0,n:-12,big:18446744073709551615,min:-9223372036854775808,f:1.5,e1:3e45,e2:1.5e-10,e3:-2.3E+8,z:-0.0,s:"h\u00e9llo \\u00e9 \\u{1F600} \\ud83d\\ude00 \\n\\t\\"\\\\\\/"}',
"comments_idents": '''/* header */ {
  // line comment
  plain: 1, "quoted key": 2, _under: 3, имя: "Дениз", 日本語: 4, true: 5, null: 6, "": 7, a1_b2: [1, 2, /* inline */ 3,],
}''',
"refs_basic": '''{ users: [ { id: 23, name: "Deniz" }, { id: 35, name: "Adil" } ],
  user_related_to: @users.1, user_id: @users.0.id }''',
"refs_current": '''{ id: 23, name: "Deniz", user_id: @!id, inner: { x: 1, y: @!x, up: @.!.id }, via: @.inner.x }''',
"refs_parent_array": '''{ items: [ { id: 1, other: @.!.1.id }, { id: 2, me: @.!.0 } ], last: @items.1 }''',
"labels_local": '''{ <user> id: 23, name: "Deniz", related_id: @user.id, nested: { again: @user.name } }''',
"labels_global": '''{ <Users> users: [ { id: 23, name: "Deniz" } ], selected_user: @Users.users.0, deep: { v: @Users.users.0.name } }''',
"labels_descent": '''{ <Users> users: [ { <user> id: 23 } ], selected: @..Users.$user.id, other: @users.0.id }''',
"labels_variable": '''{ users: [ { id: 23 }, { <$previous=$prev> id: 35, previous_user_id: @$previous.id } ] }''',
"labels_alias_expr": '''{ users: [ { id: 1 }, { id: 2 } ], <First=@users.0> pick: @First.id, <$second=users.1> also: @$second.id }''',
"labels_shadowing": '''{ a: { <x> v: 1, r: @x.v }, b: { <x> v: 2, r: @x.v, c: { r: @x.v } } }''',
"context_next_prev": '''{ users: [ { id: 23, next_user_id: @$next.id }, { id: 35, previous_user_id: @$prev.id, n: @$next.id }, { id: 40, p: @.$prev.id } ] }''',
"context_nested": '''{ values: [ { current: 10, meta: { prev: @$next.current } }, { current: 11 } ] }''',
"cross_object": '''{ users: [ { id: 23, gender: "male" } ], lessons: { attendees: [ { user: @users.0 } ] } }''',
"ref_chain": '''{ a: 1, b: @a, c: @b, d: { e: @c } }''',
"ref_through_ref": '''{ users: [ { id: 1, tag: "x" } ], att: { user: @users.0 }, tag: @att.user.tag }''',
"structure_cycle": '''{ <Root> node: { <n> self: @n, up: @.!, root: @Root.node } }''',
"root_array": '''[ { id: 1 }, { id: 2, p: @$prev, n: @.!.0.id }, 3, "x", [ ] ]''',
"empty_containers": '''{ o: {}, a: [], n: [[], {}, [[]]] }''',
"root_scalar": '''  42 ''',
"root_string": '"just a string"',
"numbers_edge": '''[0, -0, 1, 9223372036854775807, 9223372036854775808, 2147483647, 2147483648, -2147483648, -2147483649, 0.0, 1e0, 1E2, 5e-324, 1.7976931348623157e308, 0.1]''',
"unicode_keys_dupe_values": '''[{k:"v",k2:"v"},{k:"v",k2:"v"}]''',
"deep_nesting": "[" * 200 + "1" + "]" * 200,
"many_refs": "{ items: [" + ",".join("{id:%d,next:@$next}" % i for i in range(5)) + ", {id:5}], last:@items.5, ids:[" + ",".join("@items.%d.id" % i for i in range(6)) + "] }",
}

ERRORS = {
"syntax_missing_comma": '{ a: 1\n  b: 2 }',
"syntax_unterminated_string": '{ a: "abc\n}',
"syntax_unterminated_comment": '{ a: 1 } /* oops',
"syntax_trailing_content": '{ a: 1 } 2',
"syntax_empty": '  // nothing\n',
"syntax_bad_escape": '{ a: "x\\qy" }',
"syntax_lone_surrogate": '{ a: "\\ud800" }',
"syntax_control_char": '{ a: "x\ty" }'.replace("\t", "\x01"),
"syntax_duplicate_key": '{ a: 1,\n  a: 2 }',
"syntax_bare_variable": '{ a: 1, b: { c: $prev } }',
"syntax_unquoted_value": '{ a: hello }',
"syntax_depth": "[" * 300 + "]" * 300,
"syntax_key_expected": '{ 1: 2 }',
"syntax_crlf_line": '{\r\n  a: 1,\r\n  b: @\r\n}',
"number_leading_zero": '{ a: 01 }',
"number_trailing_dot": '{ a: 1. }',
"number_leading_dot": '{ a: .5 }',
"number_plus": '{ a: +1 }',
"number_bad_exp": '{ a: 1e }',
"number_minus_only": '{ a: - }',
"number_int_overflow": '{ a: 18446744073709551616 }',
"number_int_underflow": '{ a: -9223372036854775809 }',
"number_float_overflow": '{ a: 1e999 }',
"label_duplicate_local": '{ <a> <a> x: 1 }',
"label_duplicate_global": '{ x: { <Foo> y: 1 }, z: { <Foo> w: 2 } }',
"label_undefined": '{ x: @nothing.id }',
"label_out_of_scope": '{ users: [ { <mylable> id: 23 } ], user_id: @mylable.id }',
"label_ambiguous_name": '{ <users> users: [ 1 ], r: @users }',
"label_ambiguous_descent": '{ <Users> u: [ { <x> a: 1 }, { <x> a: 2 } ], r: @..Users.$x.a }',
"label_reserved": '{ <$prev=$next> a: 1 }',
"label_bad_name": '{ <_x> a: 1 }',
"label_unclosed": '{ <x a: 1 }',
"label_global_undefined": '{ r: @..Nope.a }',
"pointer_no_field": '{ a: { b: 1 }, r: @a.c }',
"pointer_wrong_type_key": '{ a: [1], r: @a.b }',
"pointer_wrong_type_index": '{ a: { b: 1 }, r: @a.0 }',
"pointer_index_range": '{ a: [1, 2], r: @a.5 }',
"pointer_parent_root": '{ r: @.!.x }',
"pointer_no_current": '[ @.x ]',
"pointer_cycle": '{ a: @b, b: @a }',
"pointer_self_cycle": '{ a: @a.b }',
"pointer_malformed_empty": '{ a: 1, r: @ }',
"pointer_trailing_dot": '{ a: 1, r: @a. }',
"pointer_leading_zero_index": '{ a: [1,2], r: @a.01 }',
"pointer_bang_alone": '{ a: 1, r: @! }',
"context_prev_first": '{ users: [ { id: 1, p: @$prev.id } ] }',
"context_next_last": '{ users: [ { id: 1 }, { id: 2, n: @$next.id } ] }',
"context_prev_no_array": '{ a: 1, b: { c: @$prev } }',
"context_dotprev_nested": '{ values: [ { current: 10, meta: { p: @.$prev } }, { current: 11 } ] }',
"context_next_root": '{ n: @.$next }',
"context_label_prev_root": '{ users: [ { id: 23 } ], <$previous=$prev> previous_user_id: @$previous.id }',
}

for name, text in VALID.items():
    w(f"text/valid/{name}.jsonx", text)
    doc = jsonx.parse(text)
    w(f"text/valid/{name}.dump", doc.dump())
    w(f"text/valid/{name}.bjsonx", jsonx.encode(doc))
    w(f"text/valid/{name}.crc.bjsonx", jsonx.encode(doc, checksum=True))
for name, text in ERRORS.items():
    w(f"text/errors/{name}.jsonx", text)
    try:
        jsonx.parse(text); print("UNEXPECTED SUCCESS", name); sys.exit(1)
    except JsonxError as e:
        w(f"text/errors/{name}.err", f"{e.code} {e.line}:{e.col}\n")
        print(f"  {name:36} {e.code} {e.line}:{e.col}")

# ---- typed
EX = os.path.join(HERE, "..", "examples")
sc = jsonx.compile_file(os.path.join(EX, "schemas", "lesson.jxs"))
w("schema/lesson.jxs", open(os.path.join(EX, "schemas", "lesson.jxs")).read())
w("schema/common.jxs", open(os.path.join(EX, "schemas", "common.jxs")).read())
w("schema/lesson.jxsb", sc.to_bytes())
V1 = open(os.path.join(EX, "schemas", "lesson.jxs")).read().replace("version 2", "version 1")
import re
V1 = re.sub(r"\n\s*email:.*", "", V1); V1 = re.sub(r"\n\s*nick:.*", "", V1)
open(os.path.join(V, "schema", "lesson_v1.jxs"), "w").write(V1)
sc1 = jsonx.compile_schema(V1, "lesson_v1.jxs", lambda p, f: (open(os.path.join(EX, "schemas", p)).read(), p))
w("schema/lesson_v1.jxsb", sc1.to_bytes())
w("schema/ids.txt", f"lesson_v2_id={sc.id:016x}\nlesson_v1_id={sc1.id:016x}\nlesson_v2_fingerprint={sc.fingerprint():016x}\n")

TYPED = {
"lesson": open(os.path.join(EX, "text", "lesson.jsonx")).read(),
"lesson_minimal": '{ id: 1, users: [], attendees: [], blob: "", grid: [] }',
"lesson_union_label": '{ id: 2, users: [], attendees: [], shape: { label: "hello" }, blob: "AAEC/w==", grid: [[32767,-32768]] }',
"lesson_many_users": "{ id: 3, users: [" + ",".join('{id:%d,name:"user%d",active:%s,score:%d.5,tags:["t%d"]}' % (i, i, "true" if i % 2 else "false", i, i) for i in range(20)) + "], attendees: [" + ",".join("{user:@users.%d,uid:@users.%d.id,seat:%d,note:%s}" % (i, i, i, '"n%d"' % i) for i in range(0, 20, 3)) + "], blob: \"\", grid: [] }",
}
manifest = []
for name, text in TYPED.items():
    w(f"typed/{name}.jsonx", text)
    doc = jsonx.parse(text)
    b = jsonx.encode(doc, sc)
    w(f"typed/{name}.bjsonx", b)
    w(f"typed/{name}.dump", jsonx.decode(b, sc).dump())
    w(f"typed/{name}.crc.bjsonx", jsonx.encode(doc, sc, checksum=True))
    manifest.append(name)
w("typed/manifest.txt", "\n".join(manifest) + "\n")

BASE = '{ id: 1, users: [ %s ], attendees: [ %s ], blob: "", grid: [] }'
U = 'id: 1, name: "n", active: true, score: 1, tags: []'
TERR = {
"missing_field": BASE % ('{ id: 1, active: true, score: 1, tags: [] }', ''),
"unknown_field": BASE % ('{ %s, bogus: 1 }' % U, ''),
"type_mismatch": BASE % ('{ id: "x", name: "n", active: true, score: 1, tags: [] }', ''),
"range_u8": BASE % ('{ %s, age: 300 }' % U, ''),
"range_u64_negative": BASE % ('{ id: -1, name: "n", active: true, score: 1, tags: [] }', ''),
"constraint_len": BASE % ('{ id: 1, name: "", active: true, score: 1, tags: [] }', ''),
"constraint_range": BASE % ('{ %s, age: 151 }' % U, ''),
"enum_bad_member": BASE % ('{ %s, role: "root" }' % U, ''),
"null_not_nullable": BASE % ('{ id: 1, name: null, active: true, score: 1, tags: [] }', ''),
"nullable_missing": BASE % ('{ %s }' % U, '{ user: @users.0, uid: @users.0.id, note: 1 }'),
"ref_wrong_type": BASE % ('{ %s }' % U, '{ user: @users.0.id, uid: @users.0.id, seat: null, note: 1 }'),
"ref_inline_value": BASE % ('{ %s }' % U, '{ user: { %s }, uid: @users.0.id, seat: null, note: 1 }' % U),
"ref_any_to_typed": BASE % ('{ %s }' % U, '{ user: @users.0, uid: @users.0.id, seat: null, note: @users.0 }'),
"union_two_keys": '{ id: 1, users: [], attendees: [], shape: { circle: { radius: 1 }, label: "x" }, blob: "", grid: [] }',
"union_unknown_variant": '{ id: 1, users: [], attendees: [], shape: { square: 1 }, blob: "", grid: [] }',
"bytes_bad_base64": '{ id: 1, users: [], attendees: [], blob: "!!!", grid: [] }',
"f32_overflow": BASE % ('{ id: 1, name: "n", active: true, score: 1e39, tags: [] }', ''),
}
for name, text in TERR.items():
    w(f"typed/errors/{name}.jsonx", text)
    try:
        jsonx.encode(jsonx.parse(text), sc); print("UNEXPECTED SUCCESS", name); sys.exit(1)
    except JsonxError as e:
        w(f"typed/errors/{name}.err", f"{e.code} {e.line}:{e.col}\n"); print(f"  typed {name:26} {e.code} {e.line}:{e.col}")

# ---- compat: v1 data read by v2 schema and v2 data read by v1 schema
d1 = jsonx.parse('{ id: 9, users: [ { id: 1, name: "a", active: true, score: 2, tags: ["x"] } ], attendees: [], blob: "", grid: [] }')
b1 = jsonx.encode(d1, sc1)
w("compat/v1_data.bjsonx", b1)
w("compat/v1_data_read_by_v2.dump", jsonx.decode(b1, sc).dump())
d2 = jsonx.parse('{ id: 9, users: [ { id: 1, name: "a", active: true, score: 2, tags: ["x"], email: "e@x", nick: "nn" }, { id: 2, name: "b", active: false, score: 3, tags: [] } ], attendees: [], blob: "", grid: [] }')
b2 = jsonx.encode(d2, sc)
w("compat/v2_data.bjsonx", b2)
w("compat/v2_data_read_by_v1.dump", jsonx.decode(b2, sc1).dump())
w("compat/v2_data_read_by_v2.dump", jsonx.decode(b2, sc).dump())
# v2 writer targeting version 1 layout is byte-identical in structure to a v1 writer, with version 2 header stamp avoided:
b21 = jsonx.encode(jsonx.parse('{ id: 9, users: [ { id: 1, name: "a", active: true, score: 2, tags: ["x"] } ], attendees: [], blob: "", grid: [] }'), sc, version=1)
w("compat/v2_schema_v1_layout.bjsonx", b21)

# ---- malformed binary
dyn = jsonx.encode(jsonx.parse('{ a: "hi", b: [1, 2.5, "x"], c: { d: null } }'))
typed = jsonx.encode(jsonx.parse(TYPED["lesson"]), sc)
def fixcrc(b): return b
def patch(b, off, data): b = bytearray(b); b[off:off + len(data)] = data; return bytes(b)
def u32(v): return struct.pack("<I", v)
MAL = {}
MAL["dyn_truncated_header"] = (dyn[:20], "E_BIN_TRUNCATED")
MAL["dyn_bad_magic"] = (patch(dyn, 0, b"XXX"), "E_BIN_MAGIC")
MAL["dyn_bad_version"] = (patch(dyn, 3, b"\x09"), "E_BIN_VERSION")
MAL["dyn_reserved_flags"] = (patch(dyn, 4, b"\x02\x00"), "E_BIN_INVALID")
MAL["dyn_length_too_big"] = (patch(dyn, 24, u32(len(dyn) + 10)), "E_BIN_TRUNCATED")
MAL["dyn_trailing_bytes"] = (dyn + b"\0\0", "E_BIN_INVALID")
MAL["dyn_truncated_body"] = (patch(dyn[:-6], 24, u32(len(dyn) - 6)), "E_BIN_BOUNDS")
MAL["dyn_root_in_header"] = (patch(dyn, 20, u32(4)), "E_BIN_BOUNDS")
MAL["dyn_root_oob"] = (patch(dyn, 20, u32(len(dyn))), "E_BIN_BOUNDS")
MAL["dyn_typed_needs_schema"] = (typed, "E_BIN_SCHEMA_MISMATCH")
MAL["dyn_bad_tag"] = (patch(dyn, 28, b"\x7f"), "E_BIN_INVALID")
MAL["dyn_nonzero_const_payload"] = (patch(dyn, 28, b"\x01") if False else patch(jsonx.encode(jsonx.parse("true")), 29, b"\x01\0\0\0"), "E_BIN_INVALID")
MAL["dyn_offset_backward"] = (patch(dyn, 29, u32(28)), "E_BIN_INVALID")
MAL["dyn_offset_oob"] = (patch(dyn, 29, u32(0x7fffffff)), "E_BIN_BOUNDS")
big = bytearray(dyn); 
MAL["dyn_huge_count"] = (patch(dyn, 33, b"\xff\xff\xff\xff\x0f"), "E_BIN_BOUNDS")
MAL["dyn_overlong_varint"] = (patch(dyn, 33, b"\x80\x00"), "E_BIN_INVALID")
MAL["dyn_ref_chain"] = (None, "E_BIN_INVALID_REF")
# ref chain: array [ref->slot1, ref->slot2(ref->slot0 value)] built by hand
def mk_dyn(body_slots, extra=b""):
    n = len(body_slots)
    blob = bytearray(28) + bytes([8]) + u32(33) + bytes([n]) + b"".join(body_slots) + extra
    struct.pack_into("<3sBHHQIII", blob, 0, b"BJX", 1, 0, 0, 0, 0, 28, len(blob))
    return bytes(blob)
s_int = lambda v: bytes([3]) + struct.pack("<i", v)
s_ref = lambda loc: bytes([10]) + u32(loc)
MAL["dyn_ref_chain"] = (mk_dyn([s_int(1), s_ref(44), s_ref(34)]), "E_BIN_INVALID_REF")
MAL["dyn_ref_to_nowhere"] = (mk_dyn([s_ref(500)]), "E_BIN_BOUNDS")
MAL["dyn_ref_not_a_slot"] = (mk_dyn([s_int(1), s_ref(36)]), "E_BIN_INVALID_REF")
MAL["dyn_bad_utf8"] = (patch(dyn, len(dyn) - 3, b"\xff\xfe") if False else None, "E_BIN_UTF8")
u8 = jsonx.encode(jsonx.parse('"ab"')); MAL["dyn_bad_utf8"] = (patch(u8, len(u8) - 2, b"\xc3\x28"), "E_BIN_UTF8")
MAL["dyn_checksum"] = (patch(jsonx.encode(jsonx.parse('[1,2,3]'), checksum=True), 40, b"\x55"), "E_BIN_CHECKSUM")
MAL["typed_bad_schema_id"] = (patch(typed, 8, b"\x01\x02\x03\x04\x05\x06\x07\x08"), "E_BIN_SCHEMA_MISMATCH")
MAL["typed_version_zero"] = (patch(typed, 16, u32(0)), "E_BIN_INVALID")
MAL["typed_truncated"] = (patch(typed[:200], 24, u32(200)), "E_BIN_BOUNDS")
MAL["typed_bad_bool"] = (None, "E_BIN_INVALID")
tmin = jsonx.encode(jsonx.parse(TYPED["lesson_minimal"]), sc)
MAL["typed_root_backward_child"] = (patch(tmin, 28 + 8, u32(28)), "E_BIN_INVALID")
for name, (data, code) in MAL.items():
    if data is None: continue
    w(f"binary/malformed/{name}.bin", data); w(f"binary/malformed/{name}.err", code + "\n")
# typed bad bool: find 'active' of first user via decoding offsets
r = jsonx.Reader(typed, sc)
uv = jsonx.view(typed, sc)['users'][0]
w("binary/malformed/typed_bad_bool.bin", patch(typed, uv.loc + sc.layout(sc.by_name['User'], 2)[0][2].off, b"\x07")); w("binary/malformed/typed_bad_bool.err", "E_BIN_INVALID\n")

# ---- schema compiler vectors: valid schemas (-> canonical bytes) and invalid ones (-> code line:col)
SOK = {
"all_types": """namespace t.all
version 3
enum E : u16 { a = 1  b  c = 300 }
enum Auto { x  y }
union U { s = 1: string  n = 2: Inner  arr = 3: array<u8> }
schema Inner { v: i32  f: f32  g: f64  b: bytes  any_: any }
schema Rec {
    id: u64 @doc("the id")  name: string = "x\\"q" @len(0, 10)
    opt: optional<u8> @range(0, _)  nul: nullable<E> = null  e: E = "b"
    list: array<array<i16>>  inner: Inner  inner_ref: ref<Inner>  u: U
    flag: bool = true  big: u64 = 18446744073709551615  neg: i64 = -5  fl: f64 = 2.5  fi: f32 = 3
    empty: array<string> = []  late: string = "z" @since(2)  late2: optional<u32> @since(3) @deprecated
}
root Rec""",
"minimal": "schema A { x: u8 }",
"enum_declared_later": "schema A { x: E = \"b\" }\nenum E { a  b }",
"comments_semicolons": "// c\nschema A { x: u8; y: optional<string>, /* c */ z: bool = false }\nroot A;",
}
SERR = {
"unexpected_char": "schema A { x: u8 } #",
"top_level_token": "42",
"missing_brace": "schema A { x: u8",
"unknown_type": "schema A { x: Foo }",
"duplicate_type": "schema A { x: u8 }\nschema A { y: u8 }",
"duplicate_field": "schema A { x: u8  x: u16 }",
"reserved_name": "schema string { x: u8 }",
"optional_nested": "schema A { x: array<optional<u8>> }",
"ref_ref": "schema B { v: u8 }\nschema A { x: ref<ref<B>> }",
"since_decreasing": "version 3\nschema A { a: u8  b: optional<u8> @since(3)  c: optional<u8> @since(2) }",
"since_beyond_version": "schema A { a: u8  b: optional<u8> @since(2) }",
"since_required_field": "version 2\nschema A { a: u8  b: u8 @since(2) }",
"first_field_late": "version 2\nschema A { a: optional<u8> @since(2) }",
"empty_schema": "schema A { }",
"enum_duplicate": "enum E { a  a }",
"enum_overflow": "enum E : u8 { a = 300 }",
"enum_bad_base": "enum E : i8 { a }",
"union_any_variant": "union U { a = 1: any }",
"union_bad_tag": "union U { a = 0: u8 }",
"union_dup_tag": "union U { a = 1: u8  b = 1: u16 }",
"default_wrong_type": "schema A { x: u8 = \"s\" }",
"default_out_of_range": "schema A { x: u8 = 256 }",
"default_on_optional": "schema A { x: optional<u8> = 1 }",
"default_bad_enum": "enum E { a }\nschema A { x: E = \"zz\" }",
"range_on_string": "schema A { x: string @range(0, 1) }",
"len_on_u8": "schema A { x: u8 @len(0, 1) }",
"root_unknown": "schema A { x: u8 }\nroot B",
"root_not_schema": "enum E { a }\nroot E",
"bad_version": "version 0\nschema A { x: u8 }",
"import_missing": "import \"nope.jxs\"\nschema A { x: u8 }",
}
for name, text in SOK.items():
    w(f"schema/ok/{name}.jxs", text)
    w(f"schema/ok/{name}.jxsb", jsonx.compile_schema(text, name + ".jxs").to_bytes())
for name, text in SERR.items():
    w(f"schema/errors/{name}.jxs", text)
    try:
        jsonx.compile_schema(text, name + ".jxs", lambda p, f: (open(os.path.join(V, "schema", "ok", p)).read(), p)); print("UNEXPECTED SUCCESS", name); sys.exit(1)
    except JsonxError as e:
        w(f"schema/errors/{name}.err", f"{e.code} {e.line}:{e.col}\n"); print(f"  schema {name:24} {e.code} {e.line}:{e.col}")

# ---- shop schema (typed structs in Go/Rust): golden blobs at version 2 and version 1 layouts
os.makedirs(os.path.join(V, "shop"), exist_ok=True)
shop = jsonx.compile_file(os.path.join(EX, "schemas", "shop.jxs"))
w("shop/shop.jxs", open(os.path.join(EX, "schemas", "shop.jxs")).read()); w("shop/shop.jxsb", shop.to_bytes())
RECEIPT = """{ id: 7,
  customers: [ { id: 1, name: "Deniz", vip: true, email: "d@example.org" }, { id: 2, name: "Adil", vip: false } ],
  items: [ { sku: "A-1", qty: 3, price: 9.5, tags: ["x", "y"], weight: 1.25 }, { sku: "B", qty: 1, price: 0.1, tags: [] } ],
  status: "paid", payment: { card: { last4: "4242", brand: "visa" } }, note: null, raw: "AAH/", grid: [[1, -2], []], nick: "dz" }"""
RECEIPT_V1 = """{ id: 7, customers: [ { id: 1, name: "Deniz", vip: true } ], items: [], status: "draft", note: "n", raw: "", grid: [] }"""
w("shop/receipt.jsonx", RECEIPT); w("shop/receipt.bjsonx", jsonx.encode(jsonx.parse(RECEIPT), shop, root="Receipt"))
w("shop/receipt_v1.jsonx", RECEIPT_V1); w("shop/receipt_v1.bjsonx", jsonx.encode(jsonx.parse(RECEIPT_V1), shop, root="Receipt", version=1))
w("shop/receipt.dump", jsonx.decode(jsonx.encode(jsonx.parse(RECEIPT), shop, root="Receipt"), shop).dump())
w("shop/receipt_v1_as_v2.dump", jsonx.decode(jsonx.encode(jsonx.parse(RECEIPT_V1), shop, root="Receipt", version=1), shop).dump())
print("vectors:", sum(len(f) for _, _, f in os.walk(V)), "files")
