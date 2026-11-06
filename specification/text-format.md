# JSONX Text Format (v0.1)

Media type `application/jsonx`, extension `.jsonx`, encoding UTF-8 (a leading U+FEFF is ignored).
The shared test vectors in `tests/vectors/text/` are normative examples of everything below.

## 1. Grammar (EBNF)

```ebnf
document   = ws value ws ;
value      = object | array | string | number | "true" | "false" | "null" | reference ;
object     = "{" ws { label_decl ws } [ member { ws "," ws { label_decl ws } member } [ ws "," ] ] ws { label_decl ws } "}" ;
member     = key ws ":" ws value ;
key        = identifier | string ;
array      = "[" ws [ value { ws "," ws value } [ ws "," ] ] ws "]" ;
string     = '"' { char | escape } '"' ;
escape     = "\" ( '"' | "\" | "/" | "b" | "f" | "n" | "r" | "t" | "u" hex4 | "u{" hex{1,6} "}" ) ;
number     = [ "-" ] ( "0" | digit19 { digit } ) [ "." digit { digit } ] [ ( "e" | "E" ) [ "+" | "-" ] digit { digit } ] ;
label_decl = "<" ws label_name [ ws "=" ws [ "@" ] pointer ] ws ">" ;
reference  = "@" pointer ;                       (* see references.md *)
ws         = { " " | "\t" | "\r" | "\n" | line_comment | block_comment } ;
line_comment  = "//" { any char except CR, LF } ;
block_comment = "/*" { any char } "*/" ;        (* not nestable *)
```

A comma after the last member/element is allowed. Members are separated by commas; label declarations are
*not* members and need no comma (`{ <a> x: 1, <b> y: 2 }`).

## 2. Values and types

| JSONX | value kind |
|---|---|
| `null`, `true`, `false` | null, bool |
| number without `.`/exponent | **integer** |
| number with `.` or exponent (`3e45`, `1.5e-10`, `-2.3E+8`) | **float** (IEEE-754 binary64) |
| string | Unicode string |
| `{...}` | object (insertion-ordered; duplicate keys are an error `E_DUPLICATE_KEY`) |
| `[...]` | array |
| `@...` | reference (resolved to a node; never a copy) |

### Numbers
* Integers must lie in **[-2^63, 2^64-1]**; otherwise `E_NUMBER_OVERFLOW`. `-0` is the integer 0.
* Floats are converted with round-to-nearest-even. A result of ±infinity is `E_NUMBER_OVERFLOW`; underflow to a
  subnormal or zero is accepted. `-0.0` is preserved. NaN/Infinity have no text syntax.
* No leading `+`, no leading zeros (`01`), no `.5`, no `5.`, no `1e`: `E_INVALID_NUMBER`. A number must not be
  followed by a digit, `.`, or an identifier character.

### Strings
Escapes as in JSON plus `\u{X..X}` (1-6 hex digits, a Unicode scalar value). `\uD83D\uDE00` surrogate pairs are
combined; a lone surrogate is `E_INVALID_ESCAPE`. Raw control characters (< U+0020) are rejected. Bytes input must be valid UTF-8 (`E_INVALID_UTF8`).

### Whitespace and comments
Whitespace is exactly space, tab, CR, LF. Unicode spaces outside strings are syntax errors. Comments are
`//…` and `/*…*/`.

## 3. Identifiers (unquoted keys)
`identifier = id_start { id_start | digit }` where `id_start` is an ASCII letter, `_`, or **any non-ASCII code point
except**: U+0080–U+00A0, U+1680, U+2000–U+200F, U+2028–U+202F, U+205F–U+206F, U+3000, U+FEFF, U+FFFE, U+FFFF.
This table-free rule is deliberate: it needs no Unicode database, so all implementations agree on every code point
regardless of their runtime's Unicode version. Anything else must be a quoted key. `true`, `false`, `null` are
valid keys. *Label names* and pointer start names used for labels are ASCII-only (see references.md).

## 4. Positions
`line` and `column` are 1-based; a column counts Unicode code points since the line start (a BOM counts as one).
`\n`, `\r\n` and lone `\r` each end one line.

## 5. Limits
Maximum nesting depth 256 (`E_DEPTH_LIMIT`). Implementations are not dependent on any JSON parser.

## 6. Canonical dump (testing)
All implementations can render a document in a canonical text form used by the shared vectors:
ints decimal, floats `f64(0x<16 hex digits of the IEEE bits>)`, f32 `f32(0x<8 hex>)`, strings JSON-escaped (only
`"`, `\`, <0x20 and 0x7F escaped, as `\u00xx`), bytes `bytes("<base64>")`, objects/arrays without whitespace,
references `ref("<JSON-pointer path of the target>")` (`~` → `~0`, `/` → `~1`; root is `""`).
