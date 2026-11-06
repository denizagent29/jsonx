// JSONX reference-compatible JavaScript implementation (Node >= 18 and browsers; no dependencies).
export class JsonxError extends Error {
  constructor(code, message, o = {}) { super(message); this.code = code; this.pos = o.pos ?? null; this.line = o.line ?? null; this.col = o.col ?? null; }
  toString() { return `${this.code}${this.line != null ? ` at ${this.line}:${this.col}` : ""}: ${this.message}`; }
}
const E = (c, m, pos) => new JsonxError(c, m, { pos });
export function lineCol(src, pos) {
  let line = 1, ls = 0, cp = 0, i = 0; const n = src.length;
  // column counts code points; positions are UTF-16 indexes
  while (i < pos && i < n) {
    const c = src.charCodeAt(i);
    if (c === 10) { line++; ls = i + 1; }
    else if (c === 13) { if (src.charCodeAt(i + 1) === 10) i++; line++; ls = i + 1; }
    i++;
  }
  let col = 1; for (let j = ls; j < pos;) { const c = src.codePointAt(j); j += c > 0xffff ? 2 : 1; col++; }
  return [line, col];
}
const MAX_DEPTH = 256;
const isIdStart = (c) => c < 0x80 ? (c === 95 || (c >= 65 && c <= 90) || (c >= 97 && c <= 122)) : nonAsciiOk(c);
const isIdChar = (c) => isIdStart(c) || (c >= 48 && c <= 57);
function nonAsciiOk(o) { return !((o >= 0x80 && o <= 0xA0) || o === 0x1680 || (o >= 0x2000 && o <= 0x200F) || (o >= 0x2028 && o <= 0x202F) || (o >= 0x205F && o <= 0x206F) || o === 0x3000 || o === 0xFEFF || o === 0xFFFE || o === 0xFFFF); }
const isAsciiId = (ch) => /^[A-Za-z_][A-Za-z0-9_]*/;

export class Node {
  constructor(kind, value = null, pos = -1) { this.kind = kind; this.value = value; this.pos = pos; this.parent = null; this.key = null; this.items = null; this.labels = null; this.lmap = null; this.expr = null; this.target = null; }
  deref() { return this.kind === "ref" ? this.target : this; }
  path() { const s = []; for (let n = this; n.parent; n = n.parent) s.push(String(n.key).replaceAll("~", "~0").replaceAll("/", "~1")); return s.reverse().map((x) => "/" + x).join(""); }
  get(k) { const n = this.deref(); return (n.kind === "object" ? n.items.get(k) : n.items[k]).deref(); }
}
export class Document {
  constructor(root, src = "") { this.root = root; this.src = src; }
  dump() { return dump(this.root); }
  /** Plain JS objects; references to objects/arrays preserve identity. Integers become Number (BigInt beyond 2^53). */
  toJS() { return toJS(this.root, new Map()); }
}
function toJS(n, memo) {
  switch (n.kind) {
    case "ref": return toJS(n.target, memo);
    case "int": { const v = n.value; return (v >= -(2n ** 53n) && v <= 2n ** 53n) ? Number(v) : v; }
    case "object": { if (memo.has(n)) return memo.get(n); const o = {}; memo.set(n, o); for (const [k, v] of n.items) Object.defineProperty(o, k, { value: toJS(v, memo), enumerable: true, writable: true, configurable: true }); return o; }
    case "array": { if (memo.has(n)) return memo.get(n); const a = []; memo.set(n, a); for (const v of n.items) a.push(toJS(v, memo)); return a; }
    case "bytes": return n.value;
    default: return n.value;
  }
}

// ------------------------------------------------------------------ parser
class Parser {
  constructor(s) { this.s = s; this.n = s.length; this.i = 0; }
  err(code, msg, pos) { throw E(code, msg, pos ?? this.i); }
  cp(i = this.i) { return i < this.n ? this.s.codePointAt(i) : -1; }
  ch(i = this.i) { return i < this.n ? this.s[i] : ""; }
  skip() {
    const s = this.s, n = this.n; let i = this.i;
    while (i < n) {
      const c = s[i];
      if (c === " " || c === "\t" || c === "\r" || c === "\n") i++;
      else if (c === "/" && s[i + 1] === "/") { i += 2; while (i < n && s[i] !== "\r" && s[i] !== "\n") i++; }
      else if (c === "/" && s[i + 1] === "*") { const j = s.indexOf("*/", i + 2); if (j < 0) this.err("E_SYNTAX", "unterminated block comment", i); i = j + 2; }
      else break;
    }
    this.i = i;
  }
  ident() { const st = this.i; while (this.i < this.n) { const c = this.cp(); if (!isIdChar(c)) break; this.i += c > 0xffff ? 2 : 1; } return this.s.slice(st, this.i); }
  document() {
    if (this.s.startsWith("\ufeff")) this.i = 1;
    this.skip();
    if (this.i >= this.n) this.err("E_UNEXPECTED_EOF", "empty document: expected a value");
    const v = this.value(0); this.skip();
    if (this.i < this.n) this.err("E_SYNTAX", "unexpected content after the root value");
    return v;
  }
  value(depth) {
    this.skip(); const s = this.s, i = this.i;
    if (i >= this.n) this.err("E_UNEXPECTED_EOF", "unexpected end of input: expected a value");
    const c = s[i];
    if (c === "{") return this.obj(depth);
    if (c === "[") return this.arr(depth);
    if (c === '"') return new Node("string", this.string(), i);
    if (c === "@") return this.ref();
    if (c === "-" || (c >= "0" && c <= "9")) return this.number();
    if (c === "+" || c === ".") this.err("E_INVALID_NUMBER", `numbers cannot start with '${c}'`);
    if (isIdStart(this.cp())) {
      const w = this.ident();
      if (w === "true") return new Node("bool", true, i);
      if (w === "false") return new Node("bool", false, i);
      if (w === "null") return new Node("null", null, i);
      this.err("E_SYNTAX", `unexpected identifier '${w}': expected a value`, i);
    }
    if (c === "$") this.err("E_SYNTAX", "unexpected '$': contextual variables are only valid inside references");
    this.err("E_SYNTAX", `unexpected character ${JSON.stringify(c)}`);
  }
  number() {
    const s = this.s, i = this.i; const re = /-?(?:0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?/y; re.lastIndex = i;
    const m = re.exec(s); if (!m) this.err("E_INVALID_NUMBER", "'-' must be followed by a digit");
    const j = i + m[0].length;
    if (j < this.n) { const c = this.cp(j); if (c === 46 || (c >= 48 && c <= 57) || isIdChar(c)) this.err("E_INVALID_NUMBER", "malformed number", i); }
    this.i = j;
    if (m[1] === undefined && m[2] === undefined) {
      const v = BigInt(m[0]);
      if (v < -(1n << 63n) || v >= (1n << 64n)) this.err("E_NUMBER_OVERFLOW", `integer ${m[0]} does not fit in [-2^63, 2^64-1]`, i);
      return new Node("int", v, i);
    }
    const v = Number(m[0]);
    if (!Number.isFinite(v)) this.err("E_NUMBER_OVERFLOW", `float ${m[0]} overflows IEEE-754 binary64`, i);
    return new Node("float", v, i);
  }
  hex4(i) { const h = this.s.slice(i, i + 4); if (h.length !== 4 || !/^[0-9a-fA-F]{4}$/.test(h)) this.err("E_INVALID_ESCAPE", "\\u needs exactly 4 hex digits", i - 2); return parseInt(h, 16); }
  string() {
    const s = this.s, n = this.n; let i = this.i + 1; const out = []; let start = i;
    for (;;) {
      if (i >= n) this.err("E_UNEXPECTED_EOF", "unterminated string", this.i);
      const c = s[i];
      if (c === '"') { out.push(s.slice(start, i)); this.i = i + 1; return out.join(""); }
      if (c === "\\") {
        out.push(s.slice(start, i)); i++;
        if (i >= n) this.err("E_UNEXPECTED_EOF", "unterminated escape", i);
        const e = s[i]; const m = { '"': '"', "\\": "\\", "/": "/", b: "\b", f: "\f", n: "\n", r: "\r", t: "\t" };
        if (e in m) { out.push(m[e]); i++; }
        else if (e === "u") {
          if (s[i + 1] === "{") {
            const j = s.indexOf("}", i + 2); const h = j > 0 ? s.slice(i + 2, j) : "";
            if (!/^[0-9a-fA-F]{1,6}$/.test(h)) this.err("E_INVALID_ESCAPE", "malformed \\u{...} escape", i - 1);
            const cp = parseInt(h, 16);
            if (cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF)) this.err("E_INVALID_ESCAPE", "not a Unicode scalar value", i - 1);
            out.push(String.fromCodePoint(cp)); i = j + 1;
          } else {
            let cp = this.hex4(i + 1); i += 5;
            if (cp >= 0xD800 && cp <= 0xDBFF) {
              if (s.slice(i, i + 2) !== "\\u") this.err("E_INVALID_ESCAPE", "lone high surrogate", i - 6);
              const lo = this.hex4(i + 2);
              if (!(lo >= 0xDC00 && lo <= 0xDFFF)) this.err("E_INVALID_ESCAPE", "high surrogate not followed by low surrogate", i - 6);
              cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00); i += 6;
            } else if (cp >= 0xDC00 && cp <= 0xDFFF) this.err("E_INVALID_ESCAPE", "lone low surrogate", i - 6);
            out.push(String.fromCodePoint(cp));
          }
        } else this.err("E_INVALID_ESCAPE", `unknown escape '\\${e}'`, i - 1);
        start = i; continue;
      }
      if (c.charCodeAt(0) < 0x20) this.err("E_SYNTAX", "unescaped control character in string", i);
      i++;
    }
  }
  obj(depth) {
    if (depth >= MAX_DEPTH) this.err("E_DEPTH_LIMIT", `nesting deeper than ${MAX_DEPTH}`);
    const node = new Node("object", null, this.i); node.items = new Map(); node.labels = []; this.i++;
    const s = this.s;
    for (;;) {
      this.skip();
      if (this.i >= this.n) this.err("E_UNEXPECTED_EOF", "unterminated object");
      const c = s[this.i];
      if (c === "}") { this.i++; break; }
      if (c === "<") { node.labels.push(this.label()); continue; }
      const kpos = this.i; let key;
      if (c === '"') key = this.string(); else if (isIdStart(this.cp())) key = this.ident(); else this.err("E_SYNTAX", "expected an object key");
      this.skip();
      if (this.i >= this.n || s[this.i] !== ":") this.err("E_SYNTAX", "expected ':' after object key");
      this.i++;
      const v = this.value(depth + 1);
      if (node.items.has(key)) this.err("E_DUPLICATE_KEY", `duplicate key ${JSON.stringify(key)}`, kpos);
      node.items.set(key, v); v.parent = node; v.key = key;
      this.skip();
      if (this.i >= this.n) this.err("E_UNEXPECTED_EOF", "unterminated object");
      const d = s[this.i];
      if (d === ",") this.i++; else if (d === "}") { this.i++; break; } else this.err("E_SYNTAX", "expected ',' or '}' after value");
    }
    return node;
  }
  arr(depth) {
    if (depth >= MAX_DEPTH) this.err("E_DEPTH_LIMIT", `nesting deeper than ${MAX_DEPTH}`);
    const node = new Node("array", null, this.i); node.items = []; this.i++; const s = this.s;
    for (;;) {
      this.skip();
      if (this.i >= this.n) this.err("E_UNEXPECTED_EOF", "unterminated array");
      if (s[this.i] === "]") { this.i++; break; }
      const v = this.value(depth + 1); v.parent = node; v.key = node.items.length; node.items.push(v);
      this.skip();
      if (this.i >= this.n) this.err("E_UNEXPECTED_EOF", "unterminated array");
      const d = s[this.i];
      if (d === ",") this.i++; else if (d === "]") { this.i++; break; } else this.err("E_SYNTAX", "expected ',' or ']' after element");
    }
    return node;
  }
  label() {
    const pos = this.i, s = this.s; this.i++; this.skip(); const npos = this.i; let v = false;
    if (s[this.i] === "$") { v = true; this.i++; }
    const re = /[A-Za-z_][A-Za-z0-9_]*/y; re.lastIndex = this.i; const m = re.exec(s);
    if (!m) this.err("E_INVALID_LABEL_NAME", "expected a label name after '<'", npos);
    let name = m[0]; this.i += name.length;
    if (this.i < this.n && isIdChar(this.cp())) this.err("E_INVALID_LABEL_NAME", "label names are ASCII", npos);
    let kind;
    if (v) { kind = "var"; name = "$" + name; if (name === "$prev" || name === "$next") this.err("E_RESERVED_LABEL", `'${name}' is a built-in contextual variable`, pos); }
    else if (name[0] === "_") this.err("E_INVALID_LABEL_NAME", "label names must start with a letter", npos);
    else kind = name[0] >= "A" && name[0] <= "Z" ? "global" : "local";
    this.skip(); let expr = null;
    if (s[this.i] === "=") { this.i++; this.skip(); const at = this.i; if (s[this.i] === "@") this.i++; expr = this.path(at); this.skip(); }
    if (s[this.i] !== ">") this.err("E_SYNTAX", "expected '>' to close the label declaration");
    this.i++;
    return { name, kind, expr, pos, owner: null, target: null };
  }
  segStart() { if (this.i >= this.n) return false; const c = this.s[this.i]; return c === "!" || c === '"' || c === "$" || isIdChar(this.cp()); }
  segment() {
    const s = this.s, i = this.i, c = i < this.n ? s[i] : "";
    if (c === "!") { this.i++; return ["parent", null]; }
    if (c === '"') return ["key", this.string()];
    if (c === "$") {
      const re = /[A-Za-z_][A-Za-z0-9_]*/y; re.lastIndex = i + 1; const m = re.exec(s);
      if (!m) this.err("E_INVALID_POINTER", "expected a name after '$'");
      this.i = i + 1 + m[0].length; return m[0] === "prev" ? ["prev", null] : m[0] === "next" ? ["next", null] : ["desc", m[0]];
    }
    if (c >= "0" && c <= "9") {
      let j = i; while (j < this.n && s[j] >= "0" && s[j] <= "9") j++;
      if (j < this.n && isIdStart(this.cp(j))) this.err("E_INVALID_POINTER", "malformed array index");
      const t = s.slice(i, j); if (t.length > 1 && t[0] === "0") this.err("E_INVALID_POINTER", "array indexes have no leading zeros");
      this.i = j; return ["index", Number(t)];
    }
    if (c && isIdStart(this.cp())) return ["key", this.ident()];
    this.err("E_INVALID_POINTER", "expected a field name, index, '$name' or '!' in pointer");
  }
  path(at) {
    const s = this.s; const c = this.i < this.n ? s[this.i] : ""; const segs = []; let start, name = null;
    if (c === "!") {
      this.i++;
      if (!(this.i < this.n && (isIdStart(this.cp()) || s[this.i] === '"'))) this.err("E_INVALID_POINTER", "'@!' must be followed by a field name");
      start = "cur"; segs.push(this.segment());
    } else if (c === ".") {
      if (s[this.i + 1] === ".") {
        this.i += 2; const re = /[A-Za-z_][A-Za-z0-9_]*/y; re.lastIndex = this.i; const m = re.exec(s);
        if (!m) this.err("E_INVALID_POINTER", "'@..' must be followed by a global label name");
        this.i += m[0].length; start = "global"; name = m[0];
      } else { this.i++; start = "cur"; if (this.segStart()) segs.push(this.segment()); }
    } else if (c === "$") {
      const sg = this.segment();
      if (sg[0] === "prev" || sg[0] === "next") { start = "ctx"; name = sg[0]; } else { start = "var"; name = "$" + sg[1]; }
    } else if (c && isIdStart(this.cp())) { start = "name"; name = this.ident(); }
    else this.err("E_INVALID_POINTER", "expected a label, field name, '.', '!' or '$' after '@'");
    while (this.i < this.n && s[this.i] === ".") { this.i++; segs.push(this.segment()); }
    return { start, name, segs, text: s.slice(at, this.i), pos: at };
  }
  ref() { const at = this.i; this.i++; const n = new Node("ref", null, at); n.expr = this.path(at); return n; }
}

// ------------------------------------------------------------------ resolver
class Resolver {
  constructor(root, src) { this.root = root; this.src = src; this.globals = new Map(); this.byName = new Map(); this.refs = []; this.labels = []; this.active = new Set(); }
  err(code, msg, pos) { const [l, c] = lineCol(this.src, pos); throw new JsonxError(code, msg, { pos, line: l, col: c }); }
  where(pos) { const [l, c] = lineCol(this.src, pos); return `${l}:${c}`; }
  collect(n) {
    if (n.kind === "object") {
      const seen = new Map();
      for (const lb of n.labels) {
        lb.owner = n;
        if (lb.kind === "global") { if (this.globals.has(lb.name)) this.err("E_DUPLICATE_LABEL", `global label '${lb.name}' is already defined at ${this.where(this.globals.get(lb.name).pos)}`, lb.pos); this.globals.set(lb.name, lb); }
        else { if (seen.has(lb.name)) this.err("E_DUPLICATE_LABEL", `label '${lb.name}' is declared twice in the same object`, lb.pos); seen.set(lb.name, lb); }
        if (!this.byName.has(lb.name)) this.byName.set(lb.name, []); this.byName.get(lb.name).push(lb); this.labels.push(lb);
      }
      n.lmap = seen; for (const c of n.items.values()) this.collect(c);
    } else if (n.kind === "array") for (const c of n.items) this.collect(c);
    else if (n.kind === "ref") this.refs.push(n);
  }
  run() { this.collect(this.root); for (const l of this.labels) this.labelTarget(l); for (const r of this.refs) this.target(r); }
  target(r) {
    if (r.target) return r.target;
    if (this.active.has(r)) this.err("E_CYCLIC_REFERENCE", "reference depends on itself", r.pos);
    this.active.add(r); let t = this.eval(r.expr, r, false, r.pos); while (t.kind === "ref") t = this.target(t);
    this.active.delete(r); r.target = t; return t;
  }
  labelTarget(lb) {
    if (lb.target) return lb.target;
    if (!lb.expr) { lb.target = lb.owner; return lb.owner; }
    if (this.active.has(lb)) this.err("E_CYCLIC_REFERENCE", `label '${lb.name}' is defined in terms of itself`, lb.pos);
    this.active.add(lb); let t = this.eval(lb.expr, lb.owner, true, lb.pos); while (t.kind === "ref") t = this.target(t);
    this.active.delete(lb); lb.target = t; return t;
  }
  deref(n) { while (n.kind === "ref") n = this.target(n); return n; }
  curObj(loc, isLab) { if (isLab) return loc; let n = loc.parent; while (n && n.kind !== "object") n = n.parent; return n; }
  elem(loc) { for (let n = loc; n; n = n.parent) if (n.parent && n.parent.kind === "array") return n; return null; }
  findLabel(name, loc) { for (let n = loc; n; n = n.parent) if (n.kind === "object" && n.lmap && n.lmap.has(name)) return n.lmap.get(name); return this.globals.get(name) ?? null; }
  describe(n) { const p = n.path(); return p ? `'${p}'` : "the root"; }
  search(n, name, out) {
    if (n.kind === "object") { for (const lb of n.labels) if (lb.name === name || lb.name === "$" + name) out.push(lb); for (const c of n.items.values()) this.search(c, name, out); }
    else if (n.kind === "array") for (const c of n.items) this.search(c, name, out);
  }
  eval(ex, loc, isLab, epos) {
    const e = (code, msg) => this.err(code, msg, epos); const txt = "@" + ex.text.replace(/^@/, "");
    let cur; const segs = ex.segs.slice();
    if (ex.start === "cur") { cur = this.curObj(loc, isLab); if (!cur) e("E_INVALID_CURRENT", `'${txt}': there is no enclosing object`); }
    else if (ex.start === "global") { const lb = this.globals.get(ex.name); if (!lb) e("E_UNDEFINED_LABEL", `global label '${ex.name}' is not defined`); cur = this.labelTarget(lb); }
    else if (ex.start === "ctx") {
      cur = this.elem(loc);
      if (!cur) e(ex.name === "prev" ? "E_INVALID_PREV" : "E_INVALID_NEXT", `'$${ex.name}' used where there is no enclosing array element`);
      segs.unshift([ex.name, null]);
    } else {
      const nm = ex.name, lb = this.findLabel(nm, loc);
      const rk = ex.start === "name" && this.root.kind === "object" && this.root.items.has(nm);
      if (lb && rk) e("E_AMBIGUOUS_NAME", `'${nm}' names both a label and a field of the root object`);
      if (lb) cur = this.labelTarget(lb); else if (rk) cur = this.root.items.get(nm);
      else {
        const o = this.byName.get(nm);
        if (o && o.length) e("E_LABEL_OUT_OF_SCOPE", `label '${nm}' (declared at ${this.where(o[0].pos)}) is not visible from here`);
        e("E_UNDEFINED_LABEL", `no label or root field named '${nm}'`);
      }
    }
    for (const [kind, val] of segs) {
      if (kind === "key" || kind === "index") {
        cur = this.deref(cur);
        if (kind === "key") {
          if (cur.kind !== "object") e("E_POINTER_TYPE", `'${txt}': cannot select field '${val}' of a ${cur.kind}`);
          if (!cur.items.has(val)) e("E_POINTER_NO_FIELD", `'${txt}': ${this.describe(cur)} has no field '${val}'`);
          cur = cur.items.get(val);
        } else {
          if (cur.kind !== "array") e("E_POINTER_TYPE", `'${txt}': cannot index a ${cur.kind} with [${val}]`);
          if (val >= cur.items.length) e("E_INVALID_INDEX", `'${txt}': index ${val} is out of range (length ${cur.items.length})`);
          cur = cur.items[val];
        }
      } else if (kind === "parent") { if (!cur.parent) e("E_INVALID_PARENT", `'${txt}': ${this.describe(cur)} has no parent`); cur = cur.parent; }
      else if (kind === "prev" || kind === "next") {
        const p = cur.parent, code = kind === "prev" ? "E_INVALID_PREV" : "E_INVALID_NEXT";
        if (!p || p.kind !== "array") e(code, `'$${kind}' needs an array element`);
        const i = cur.key + (kind === "prev" ? -1 : 1);
        if (i < 0) e(code, "'$prev' used on the first element: there is no previous element");
        if (i >= p.items.length) e(code, "'$next' used on the last element: there is no next element");
        cur = p.items[i];
      } else {
        const found = []; this.search(cur, val, found);
        if (!found.length) e("E_UNDEFINED_LABEL", `no label '${val}' is declared inside ${this.describe(cur)}`);
        if (found.length > 1) e("E_AMBIGUOUS_LABEL", `label '${val}' is declared ${found.length} times inside ${this.describe(cur)}`);
        cur = this.labelTarget(found[0]);
      }
    }
    return cur;
  }
}
export function parse(text) {
  if (text instanceof Uint8Array) { try { text = new TextDecoder("utf-8", { fatal: true }).decode(text); } catch { throw new JsonxError("E_INVALID_UTF8", "input is not valid UTF-8"); } }
  try { const root = new Parser(text).document(); new Resolver(root, text).run(); return new Document(root, text); }
  catch (ex) { if (ex instanceof JsonxError && ex.line == null && ex.pos != null) [ex.line, ex.col] = lineCol(text, ex.pos); throw ex; }
}

// ------------------------------------------------------------------ canonical dump
const jstr = (s) => { let o = '"'; for (const ch of s) { const c = ch.codePointAt(0); o += ch === '"' ? '\\"' : ch === "\\" ? "\\\\" : (c < 0x20 || c === 0x7f) ? "\\u" + c.toString(16).padStart(4, "0") : ch; } return o + '"'; };
const dvF = new DataView(new ArrayBuffer(8));
const b64 = (u8) => typeof Buffer !== "undefined" ? Buffer.from(u8).toString("base64") : btoa(String.fromCharCode(...u8));
export function dump(n) {
  switch (n.kind) {
    case "null": return "null"; case "bool": return n.value ? "true" : "false"; case "int": return n.value.toString();
    case "float": dvF.setFloat64(0, n.value); return "f64(0x" + dvF.getBigUint64(0).toString(16).padStart(16, "0") + ")";
    case "f32": dvF.setFloat32(0, n.value); return "f32(0x" + dvF.getUint32(0).toString(16).padStart(8, "0") + ")";
    case "string": return jstr(n.value); case "bytes": return `bytes("${b64(n.value)}")`;
    case "ref": return "ref(" + jstr(n.target.path()) + ")";
    case "array": return "[" + n.items.map(dump).join(",") + "]";
    default: return "{" + [...n.items].map(([k, v]) => jstr(k) + ":" + dump(v)).join(",") + "}";
  }
}

// ------------------------------------------------------------------ schema (load canonical BJXS)
const INTS = { u8: [0, 8], u16: [0, 16], u32: [0, 32], u64: [0, 64], i8: [1, 8], i16: [1, 16], i32: [1, 32], i64: [1, 64] };
const SCALAR_SIZE = { bool: 1, u8: 1, i8: 1, u16: 2, i16: 2, u32: 4, i32: 4, f32: 4, u64: 8, i64: 8, f64: 8 };
const CODE_T = { 1: "bool", 2: "u8", 3: "u16", 4: "u32", 5: "u64", 6: "i8", 7: "i16", 8: "i32", 9: "i64", 10: "f32", 11: "f64", 12: "string", 13: "bytes", 14: "any" };
const T = (kind, idx, elem) => ({ kind, idx, elem });
const tk = (t) => t.kind === "struct" || t.kind === "enum" || t.kind === "union" ? `${t.kind}:${t.idx}` : t.kind === "array" || t.kind === "ref" ? `${t.kind}<${tk(t.elem)}>` : t.kind;
const ANY = T("any");
export function fnv1a64(bytes) { let h = 0xcbf29ce484222325n; for (const b of bytes) h = ((h ^ BigInt(b)) * 0x100000001b3n) & 0xffffffffffffffffn; return h; }
export class Schema {
  static fromBytes(data) {
    const d = data instanceof Uint8Array ? data : new Uint8Array(data); let p = 0; const dec = new TextDecoder();
    const bad = (m) => { throw new JsonxError("E_SCHEMA_BINARY", m); };
    const u8 = () => { if (p >= d.length) bad("truncated schema"); return d[p++]; };
    const vi = () => { let v = 0n, sh = 0n; for (let i = 0; i < 10; i++) { const b = u8(); v |= BigInt(b & 0x7f) << sh; sh += 7n; if (!(b & 0x80)) return v; } bad("varint too long"); };
    const vn = () => Number(vi());
    const st = () => { const n = vn(); if (p + n > d.length) bad("truncated string"); const s = dec.decode(d.subarray(p, p + n)); p += n; return s; };
    const td = (dp = 0) => { if (dp > 32) bad("type nesting too deep"); const c = u8(); if (CODE_T[c]) return T(CODE_T[c]); if (c === 20) return T("struct", vn()); if (c === 21) return T("enum", vn()); if (c === 22) return T("union", vn()); if (c === 23) return T("array", undefined, td(dp + 1)); if (c === 24) return T("ref", undefined, td(dp + 1)); bad("unknown type code " + c); };
    if (d.length < 5 || String.fromCharCode(...d.subarray(0, 4)) !== "BJXS" || d[4] !== 1) bad("not a BJXS v1 schema");
    p = 5; const s = new Schema(); s.namespace = st(); s.version = vn(); const r = vn(); s.root = r === 0 ? null : r - 1; s.types = []; s._lay = new Map();
    const n = vn(); if (n > 100000) bad("too many types");
    for (let i = 0; i < n; i++) {
      const k = u8(), name = st(); let t;
      if (k === 1) {
        t = { kind: "struct", name, fields: [], byName: new Map() };
        const nf = vn();
        for (let j = 0; j < nf; j++) {
          const f = { name: st(), type: td(), optional: false, nullable: false, since: 1, default: null, cons: {} }; const fl = u8();
          f.optional = !!(fl & 1); f.nullable = !!(fl & 2); f.since = vn();
          if (fl & 8) { const c = u8();
            if (c === 0) f.default = ["null", null]; else if (c === 1 || c === 2) f.default = ["bool", c === 2];
            else if (c === 3) { const z = vi(); f.default = ["int", (z >> 1n) ^ -(z & 1n)]; } else if (c === 4) f.default = ["int", vi()];
            else if (c === 5) { f.default = ["float", new DataView(d.buffer, d.byteOffset + p, 8).getFloat64(0, true)]; p += 8; }
            else if (c === 6) f.default = ["string", st()]; else if (c === 7) f.default = ["array", []]; else bad("bad default tag"); }
          const na = vn();
          for (let a = 0; a < na; a++) { const an = st(), aa = st(); if (an === "range" || an === "len") { const q = aa.split(",").map((x) => x.trim()); f.cons[an] = q.map((x) => x === "_" ? null : Number(x)); } }
          t.fields.push(f); t.byName.set(f.name, f);
        }
      } else if (k === 2) { const b = u8(); if (![1, 2, 4].includes(b)) bad("bad enum base"); t = { kind: "enum", name, base: b, values: [] }; const nv = vn(); for (let j = 0; j < nv; j++) t.values.push([st(), vn()]); }
      else if (k === 3) { t = { kind: "union", name, variants: [] }; const nv = vn(); for (let j = 0; j < nv; j++) { const vname = st(), tag = u8(); t.variants.push([vname, tag, td()]); } }
      else bad("bad type kind");
      s.types.push(t);
    }
    if (p !== d.length) bad("trailing bytes in schema");
    return s;
  }
  slotSize(t) { const k = t.kind; if (k in SCALAR_SIZE) return SCALAR_SIZE[k]; if (k === "enum") return this.types[t.idx].base; if (k === "string" || k === "bytes" || k === "array" || k === "struct" || k === "ref") return 4; return 5; }
  layout(idx, ver) {
    const key = idx * 65536 + ver; let r = this._lay.get(key); if (r) return r;
    const out = []; let off = 0, nopt = 0; const pres = [];
    for (const f of this.types[idx].fields) {
      if (f.since > ver) continue; let pb = null, bit = null;
      if (f.optional || f.nullable) { if (nopt % 8 === 0) { pres.push(off); off += 1; } pb = pres[nopt >> 3]; bit = nopt % 8; nopt++; }
      out.push({ field: f, off, pbyte: pb, bit }); off += this.slotSize(f.type);
    }
    r = { lay: out, size: off }; this._lay.set(key, r); return r;
  }
  get id() { if (this.root === null) return 0n; return fnv1a64(new TextEncoder().encode(this.namespace + "\0" + this.types[this.root].name)); }
  idFor(name) { return fnv1a64(new TextEncoder().encode(this.namespace + "\0" + name)); }
  structIndex(name) { const i = this.types.findIndex((t) => t.kind === "struct" && t.name === name); if (i < 0) throw new JsonxError("E_SCHEMA_UNKNOWN_TYPE", `no schema named '${name}'`); return i; }
  typeName(t) { const k = t.kind; return k === "struct" || k === "enum" || k === "union" ? this.types[t.idx].name : k === "array" || k === "ref" ? `${k}<${this.typeName(t.elem)}>` : k; }
}
export const loadSchema = (b) => Schema.fromBytes(b);

// ------------------------------------------------------------------ binary
const HDR = 28, MAGIC = [0x42, 0x4a, 0x58];
const TAG = { NULL: 0, FALSE: 1, TRUE: 2, I32: 3, I64: 4, U64: 5, F64: 6, STR: 7, ARR: 8, OBJ: 9, REF: 10, BYTES: 11 };
const vlen = (n) => n < 0x80 ? 1 : n < 0x4000 ? 2 : n < 0x200000 ? 3 : n < 0x10000000 ? 4 : 5;
const kd = (n) => n.kind === "int" ? "integer" : n.kind === "f32" ? "float" : n.kind;
const utf8 = new TextEncoder();
const crcTable = (() => { const t = new Uint32Array(256); for (let i = 0; i < 256; i++) { let c = i; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[i] = c >>> 0; } return t; })();
export const crc32 = (u8, end = u8.length) => { let c = 0xffffffff; for (let i = 0; i < end; i++) c = crcTable[(c ^ u8[i]) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };

class Enc {
  constructor(sc, ver, src) { this.sc = sc; this.ver = ver; this.src = src; this.cap = 256; this.u = new Uint8Array(this.cap); this.dv = new DataView(this.u.buffer); this.len = HDR; this.refs = []; this.locs = new Map(); this.keys = new Map(); }
  err(code, msg, node) { let line = null, col = null; if (node && node.pos >= 0 && this.src) [line, col] = lineCol(this.src, node.pos); throw new JsonxError(code, msg, { pos: node?.pos, line, col }); }
  mismatch(t, node) { this.err("E_TYPE_MISMATCH", `expected ${this.sc ? this.sc.typeName(t) : t.kind}, got ${kd(node)}`, node); }
  alloc(n) { const p = this.len; if (p + n >= 0xffffffff) throw new JsonxError("E_BIN_LIMIT", "document exceeds the 4 GiB offset limit"); this.len += n; if (this.len > this.cap) { while (this.cap < this.len) this.cap *= 2; const nu = new Uint8Array(this.cap); nu.set(this.u); this.u = nu; this.dv = new DataView(nu.buffer); } return p; }
  varint(p, v) { while (v >= 0x80) { this.u[p++] = (v & 0x7f) | 0x80; v = Math.floor(v / 128); } this.u[p++] = v; return p; }
  u32(p, v) { this.dv.setUint32(p, v, true); }
  reg(node, loc, t) { this.locs.set(node, [loc, t]); }
  putStr(b) { const p = this.alloc(vlen(b.length) + b.length); const q = this.varint(p, b.length); this.u.set(b, q); return p; }
  anySlot(node, pos, depth = 0) {
    if (depth > MAX_DEPTH) throw new JsonxError("E_DEPTH_LIMIT", "nesting too deep");
    this.reg(node, pos, ANY); const k = node.kind;
    if (k === "null") return;
    if (k === "bool") this.u[pos] = node.value ? TAG.TRUE : TAG.FALSE;
    else if (k === "int") {
      const v = node.value;
      if (v >= -(2n ** 31n) && v < 2n ** 31n) { this.u[pos] = TAG.I32; this.dv.setInt32(pos + 1, Number(v), true); }
      else if (v >= -(2n ** 63n) && v < 2n ** 63n) { const p = this.alloc(8); this.dv.setBigInt64(p, v, true); this.u[pos] = TAG.I64; this.u32(pos + 1, p); }
      else { const p = this.alloc(8); this.dv.setBigUint64(p, v, true); this.u[pos] = TAG.U64; this.u32(pos + 1, p); }
    } else if (k === "float" || k === "f32") { const p = this.alloc(8); this.dv.setFloat64(p, node.value, true); this.u[pos] = TAG.F64; this.u32(pos + 1, p); }
    else if (k === "string") { this.u[pos] = TAG.STR; this.u32(pos + 1, this.putStr(utf8.encode(node.value))); }
    else if (k === "bytes") { this.u[pos] = TAG.BYTES; this.u32(pos + 1, this.putStr(node.value)); }
    else if (k === "array") {
      const n = node.items.length, hl = vlen(n), blk = this.alloc(hl + 5 * n); this.varint(blk, n); this.u[pos] = TAG.ARR; this.u32(pos + 1, blk);
      node.items.forEach((c, i) => this.anySlot(c, blk + hl + 5 * i, depth + 1));
    } else if (k === "object") {
      const n = node.items.size, hl = vlen(n), blk = this.alloc(hl + 9 * n); this.varint(blk, n); this.u[pos] = TAG.OBJ; this.u32(pos + 1, blk); let i = 0;
      for (const [key, c] of node.items) {
        const e = blk + hl + 9 * i++; let ko = this.keys.get(key); if (ko === undefined) { ko = this.putStr(utf8.encode(key)); this.keys.set(key, ko); }
        this.u32(e, ko); this.anySlot(c, e + 4, depth + 1);
      }
    } else if (k === "ref") { this.u[pos] = TAG.REF; this.refs.push([pos + 1, node, ANY]); }
    else throw new JsonxError("E_TYPE_MISMATCH", `cannot encode ${k}`);
  }
  scalar(t, node, pos) {
    const k = t.kind;
    if (k === "bool") { if (node.kind !== "bool") this.mismatch(t, node); this.u[pos] = node.value ? 1 : 0; }
    else if (k in INTS) {
      if (node.kind !== "int") this.mismatch(t, node);
      const [sg, bits] = INTS[k], v = node.value; const B = BigInt(bits);
      const lo = sg ? -(1n << (B - 1n)) : 0n, hi = sg ? (1n << (B - 1n)) - 1n : (1n << B) - 1n;
      if (v < lo || v > hi) this.err("E_RANGE", `${v} does not fit in ${k} (${lo}..${hi})`, node);
      const dv = this.dv;
      ({ u8: () => dv.setUint8(pos, Number(v)), i8: () => dv.setInt8(pos, Number(v)), u16: () => dv.setUint16(pos, Number(v), true), i16: () => dv.setInt16(pos, Number(v), true), u32: () => dv.setUint32(pos, Number(v), true), i32: () => dv.setInt32(pos, Number(v), true), u64: () => dv.setBigUint64(pos, v, true), i64: () => dv.setBigInt64(pos, v, true) })[k]();
    } else if (k === "f32" || k === "f64") {
      if (node.kind !== "int" && node.kind !== "float" && node.kind !== "f32") this.mismatch(t, node);
      const v = Number(node.value);
      if (k === "f32") { const r = Math.fround(v); if (!Number.isFinite(r) && Number.isFinite(v)) this.err("E_RANGE", `${v} overflows f32`, node); this.dv.setFloat32(pos, v, true); } else this.dv.setFloat64(pos, v, true);
    } else if (k === "enum") {
      const en = this.sc.types[t.idx]; let v;
      if (node.kind === "string") { const m = en.values.find(([n]) => n === node.value); if (!m) this.err("E_TYPE_MISMATCH", `'${node.value}' is not a member of enum ${en.name}`, node); v = m[1]; }
      else if (node.kind === "int" && node.value >= 0n && node.value < 2n ** BigInt(8 * en.base)) v = Number(node.value); else this.mismatch(t, node);
      for (let i = 0; i < en.base; i++) this.u[pos + i] = Math.floor(v / 256 ** i) & 255;
    }
  }
  bytesOf(node) {
    if (node.kind === "bytes") return node.value; if (node.kind !== "string") this.mismatch(T("bytes"), node);
    if (!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(node.value)) this.err("E_TYPE_MISMATCH", "bytes fields take a base64 string", node);
    const out = typeof Buffer !== "undefined" ? new Uint8Array(Buffer.from(node.value, "base64")) : Uint8Array.from(atob(node.value), (c) => c.charCodeAt(0));
    if (b64(out) !== node.value) this.err("E_TYPE_MISMATCH", "bytes fields take a canonical base64 string", node);
    return out;
  }
  heapPut(t, node) {
    const k = t.kind;
    if (k === "string") { if (node.kind !== "string") this.mismatch(t, node); return this.putStr(utf8.encode(node.value)); }
    if (k === "bytes") return this.putStr(this.bytesOf(node));
    if (k === "array") return this.arrayBlock(t, node);
    if (k === "struct") { if (node.kind !== "object") this.mismatch(t, node); const { size } = this.sc.layout(t.idx, this.ver); const p = this.alloc(size); this.structBlock(t.idx, node, p); return p; }
    const p = this.alloc(this.sc.slotSize(t)); this.scalar(t, node, p); return p;
  }
  slot(t, node, pos) {
    const k = t.kind;
    if (k in SCALAR_SIZE || k === "enum") { this.scalar(t, node, pos); this.reg(node, pos, t); }
    else if (k === "string" || k === "bytes" || k === "array" || k === "struct") { const off = this.heapPut(t, node); this.u32(pos, off); if (k !== "struct") this.reg(node, pos, t); }
    else if (k === "union") {
      if (node.kind !== "object" || node.items.size !== 1) this.err("E_TYPE_MISMATCH", "union value must be an object with exactly one key naming a variant", node);
      const un = this.sc.types[t.idx]; const [[name, child]] = [...node.items]; const v = un.variants.find((x) => x[0] === name);
      if (!v) this.err("E_UNKNOWN_FIELD", `'${name}' is not a variant of union ${un.name}`, child);
      const off = this.heapPut(v[2], child); this.u[pos] = v[1]; this.u32(pos + 1, off); this.reg(node, pos, t);
    } else if (k === "any") this.anySlot(node, pos);
    else if (k === "ref") { if (node.kind !== "ref") this.err("E_TYPE_MISMATCH", `expected ${this.sc.typeName(t)}, got ${kd(node)}`, node); this.refs.push([pos, node, t.elem]); }
  }
  arrayBlock(t, node) {
    if (node.kind !== "array") this.mismatch(t, node);
    const elem = t.elem, items = node.items, n = items.length; let blk;
    if (elem.kind === "struct") {
      const { size } = this.sc.layout(elem.idx, this.ver), hl = vlen(n) + vlen(size); blk = this.alloc(hl + n * size); const p = this.varint(blk, n); this.varint(p, size);
      items.forEach((c, i) => { if (c.kind !== "object") this.mismatch(elem, c); this.structBlock(elem.idx, c, blk + hl + i * size); });
    } else {
      const es = this.sc.slotSize(elem), hl = vlen(n); blk = this.alloc(hl + n * es); this.varint(blk, n);
      items.forEach((c, i) => this.slot(elem, c, blk + hl + i * es));
    }
    return blk;
  }
  structBlock(idx, node, base) {
    const sc = this.sc, st = sc.types[idx]; this.reg(node, base, T("struct", idx));
    const { lay } = sc.layout(idx, this.ver); const known = new Set(lay.map((l) => l.field.name));
    for (const [k, c] of node.items) if (!known.has(k)) this.err("E_UNKNOWN_FIELD", `schema ${st.name} has no field '${k}'`, c);
    for (const fl of lay) {
      const f = fl.field; let child = node.items.get(f.name); const opt = f.optional || f.nullable;
      if (child === undefined) {
        if (f.default) child = mkDefault(f.default);
        else if (f.optional) continue;
        else if (f.nullable) this.err("E_MISSING_FIELD", `${st.name}.${f.name} is nullable but not optional: write null explicitly`, node);
        else this.err("E_MISSING_FIELD", `${st.name}.${f.name} is required`, node);
      }
      if (opt) { if (child.kind === "null") continue; this.u[base + fl.pbyte] |= 1 << fl.bit; }
      else if (child.kind === "null" && f.type.kind !== "any") this.err("E_TYPE_MISMATCH", `${st.name}.${f.name} is not nullable`, child);
      this.slot(f.type, child, base + fl.off);
      if (Object.keys(f.cons).length) this.constrain(st, f, child);
    }
  }
  constrain(st, f, node) {
    for (const [name, [lo, hi]] of Object.entries(f.cons)) {
      if (name === "range") { if (["int", "float", "f32"].includes(node.kind)) { const v = Number(node.value); if ((lo !== null && v < lo) || (hi !== null && v > hi)) this.err("E_CONSTRAINT", `${st.name}.${f.name} = ${v} violates @range`, node); } }
      else {
        let n = null;
        if (node.kind === "string") n = f.type.kind === "bytes" ? this.bytesOf(node).length : [...node.value].length; else if (node.kind === "bytes") n = node.value.length; else if (node.kind === "array") n = node.items.length;
        if (n !== null && ((lo !== null && n < lo) || (hi !== null && n > hi))) this.err("E_CONSTRAINT", `${st.name}.${f.name} has length ${n}, violating @len`, node);
      }
    }
  }
}

function mkDefault([k, v]) { const n = new Node(k, v); if (k === "array") n.items = []; return n; }

export function encode(doc, schema = null, opts = {}) {
  const node = doc instanceof Document ? doc.root : doc, src = doc instanceof Document ? doc.src : "";
  let e, rootOff, sid = 0n, ver = 0;
  if (!schema) { e = new Enc(null, 0, src); const p = e.alloc(5); e.anySlot(node, p); rootOff = p; }
  else {
    ver = opts.version ?? schema.version;
    if (ver < 1 || ver > schema.version) throw new JsonxError("E_SCHEMA_INVALID", `cannot encode version ${ver} with a schema of version ${schema.version}`);
    const ridx = opts.root ? schema.structIndex(opts.root) : schema.root;
    if (ridx === null) throw new JsonxError("E_SCHEMA_INVALID", "schema declares no root");
    e = new Enc(schema, ver, src); if (node.kind !== "object") e.mismatch(T("struct", ridx), node);
    rootOff = e.alloc(schema.layout(ridx, ver).size); e.structBlock(ridx, node, rootOff);
    sid = opts.root ? schema.idFor(opts.root) : schema.id;
  }
  for (const [pos, rn, exp] of e.refs) {
    const tg = e.locs.get(rn.target); if (!tg) e.err("E_REF_TARGET", "reference target is not addressable in the binary layout", rn);
    if (tk(tg[1]) !== tk(exp)) e.err("E_REF_TARGET", `reference of type ref<${tk(exp)}> points at a value of type ${tk(tg[1])}`, rn);
    e.u32(pos, tg[0]);
  }
  const crc = !!opts.checksum; if (crc) e.alloc(4);
  const out = e.u.slice(0, e.len), dv = new DataView(out.buffer);
  out.set(MAGIC, 0); out[3] = 1; dv.setUint16(4, crc ? 1 : 0, true); dv.setUint16(6, 0, true); dv.setBigUint64(8, sid, true); dv.setUint32(16, ver, true); dv.setUint32(20, rootOff, true); dv.setUint32(24, out.length, true);
  if (crc) dv.setUint32(out.length - 4, crc32(out, out.length - 4), true);
  return out;
}

// ---- reader
class Reader {
  constructor(data, schema = null, opts = {}) {
    const u = data instanceof Uint8Array ? data : new Uint8Array(data); this.u = u; this.dv = new DataView(u.buffer, u.byteOffset, u.byteLength); const n = this.n = u.length;
    const bad = (c, m) => { throw new JsonxError(c, m); };
    if (n < HDR) bad("E_BIN_TRUNCATED", "blob is shorter than the 28-byte header");
    if (u[0] !== 0x42 || u[1] !== 0x4a || u[2] !== 0x58) bad("E_BIN_MAGIC", "not a BJSONX blob (bad magic)");
    if (u[3] !== 1) bad("E_BIN_VERSION", `unsupported BJSONX format version ${u[3]}`);
    const flags = this.dv.getUint16(4, true), res = this.dv.getUint16(6, true), sid = this.dv.getBigUint64(8, true), sver = this.dv.getUint32(16, true), root = this.dv.getUint32(20, true), length = this.dv.getUint32(24, true);
    if ((flags & ~1) || res) bad("E_BIN_INVALID", "reserved header bits are set");
    if (length > n) bad("E_BIN_TRUNCATED", "header length exceeds the data");
    if (length < n) bad("E_BIN_INVALID", "trailing bytes after the blob");
    if (flags & 1) { if (n < HDR + 4) bad("E_BIN_TRUNCATED", "missing checksum"); if (opts.verifyCrc !== false && crc32(u, n - 4) !== this.dv.getUint32(n - 4, true)) bad("E_BIN_CHECKSUM", "CRC-32 mismatch"); this.n = n - 4; }
    this.sid = sid; this.sver = sver; this.root = root; this.sc = schema; this.dynamic = sid === 0n && sver === 0; this.ridx = null;
    if (!this.dynamic) {
      if (!schema) bad("E_BIN_SCHEMA_MISMATCH", "blob requires a schema; none supplied");
      if (sver < 1) bad("E_BIN_INVALID", "typed blob with schema version 0");
      this.ridx = schema.types.findIndex((t) => t.kind === "struct" && schema.idFor(t.name) === sid);
      if (this.ridx < 0) bad("E_BIN_SCHEMA_MISMATCH", "schema id does not match any schema in the supplied definition");
      if (opts.strictVersion && sver > schema.version) bad("E_BIN_SCHEMA_MISMATCH", "blob is newer than the reader's schema");
    }
    this.w = sver; this._f = new Map();
    if (root < HDR || root >= this.n) bad("E_BIN_BOUNDS", `root offset ${root} is outside the payload`);
  }
  need(p, k) { if (p < HDR || p + k > this.n) throw new JsonxError("E_BIN_BOUNDS", `access of ${k} bytes at offset ${p} is outside the payload`); }
  u8(p) { this.need(p, 1); return this.u[p]; }
  u32(p) { this.need(p, 4); return this.dv.getUint32(p, true); }
  varint(p) {
    let v = 0;
    for (let i = 0; i < 5; i++) {
      this.need(p + i, 1); const b = this.u[p + i]; v += (b & 0x7f) * 2 ** (7 * i);
      if (!(b & 0x80)) { if (i && b === 0) throw new JsonxError("E_BIN_INVALID", "non-minimal varint"); if (v > 0xffffffff) throw new JsonxError("E_BIN_INVALID", "varint exceeds u32"); return [v, p + i + 1]; }
    }
    throw new JsonxError("E_BIN_INVALID", "varint longer than 5 bytes");
  }
  fwd(off, ms, what) { if (off <= ms) throw new JsonxError("E_BIN_INVALID", `${what} offset ${off} does not point forward of its container at ${ms}`); return off; }
  hbytes(off) { const [n, p] = this.varint(off); this.need(p, n); return this.u.subarray(p, p + n); }
  hstr(off) { const b = this.hbytes(off); try { return new TextDecoder("utf-8", { fatal: true }).decode(b); } catch { throw new JsonxError("E_BIN_UTF8", `invalid UTF-8 in string at offset ${off}`); } }
  scalar(k, p) {
    if (k === "bool") { const v = this.u8(p); if (v > 1) throw new JsonxError("E_BIN_INVALID", `bool byte ${v}`); return v === 1; }
    this.need(p, SCALAR_SIZE[k]); const dv = this.dv;
    switch (k) { case "u8": return dv.getUint8(p); case "i8": return dv.getInt8(p); case "u16": return dv.getUint16(p, true); case "i16": return dv.getInt16(p, true); case "u32": return dv.getUint32(p, true); case "i32": return dv.getInt32(p, true); case "u64": return dv.getBigUint64(p, true); case "i64": return dv.getBigInt64(p, true); case "f32": return dv.getFloat32(p, true); default: return dv.getFloat64(p, true); }
  }
  enumRaw(t, p) { const en = this.sc.types[t.idx]; this.need(p, en.base); let v = 0; for (let i = en.base - 1; i >= 0; i--) v = v * 256 + this.u[p + i]; return [v, en]; }
  fields(idx) { let f = this._f.get(idx); if (!f) { const { lay, size } = this.sc.layout(idx, this.w); f = { by: new Map(lay.map((l) => [l.field.name, l])), lay, size }; this._f.set(idx, f); } return f; }
  slotGet(t, pos, ms) {
    const k = t.kind;
    if (k in SCALAR_SIZE) return num(this.scalar(k, pos));
    if (k === "enum") { const [v, en] = this.enumRaw(t, pos); const m = en.values.find(([, x]) => x === v); return m ? m[0] : v; }
    if (k === "any") return this.anyGet(pos, ms);
    if (k === "union") return new UnionView(this, t.idx, pos, ms);
    if (k === "ref") return this.valueAt(t.elem, this.u32(pos));
    return this.heapGet(t, this.fwd(this.u32(pos), ms, k), ms);
  }
  heapGet(t, off, ms) {
    const k = t.kind;
    if (k === "string") return this.hstr(off); if (k === "bytes") return this.hbytes(off); if (k === "array") return this.arrayView(t, off);
    if (k === "struct") return new StructView(this, t.idx, off);
    if (k in SCALAR_SIZE) return num(this.scalar(k, off));
    if (k === "enum") { const [v, en] = this.enumRaw(t, off); const m = en.values.find(([, x]) => x === v); return m ? m[0] : v; }
    throw new JsonxError("E_BIN_INVALID", `type ${k} cannot be stored by offset`);
  }
  valueAt(t, loc) { if (t.kind === "struct") return new StructView(this, t.idx, loc); this.need(loc, this.sc.slotSize(t)); return this.slotGet(t, loc, 0); }
  arrayView(t, off) {
    let [n, p] = this.varint(off); const elem = t.elem;
    if (elem.kind === "struct") { let stride; [stride, p] = this.varint(p); const ksz = this.fields(elem.idx).size; if (stride < ksz) throw new JsonxError("E_BIN_INVALID", "array stride smaller than struct size"); if (n) this.need(p, n * stride); return new ArrayView(this, elem, off, n, p, stride); }
    const es = this.sc.slotSize(elem); if (n) this.need(p, n * es); return new ArrayView(this, elem, off, n, p, es);
  }
  anyGet(pos, ms) {
    const tag = this.u8(pos), pl = this.u32(pos + 1);
    if (tag <= 2) { if (pl) throw new JsonxError("E_BIN_INVALID", "nonzero payload on constant tag"); return tag === 0 ? null : tag === 2; }
    if (tag === TAG.I32) return this.dv.getInt32(pos + 1, true);
    if (tag === TAG.REF) { this.need(pl, 5); if (this.u[pl] === TAG.REF) throw new JsonxError("E_BIN_INVALID_REF", "reference targets another reference"); return this.anyGet(pl, 0); }
    this.fwd(pl, ms, "value");
    if (tag === TAG.I64) return num(this.scalar("i64", pl)); if (tag === TAG.U64) return num(this.scalar("u64", pl)); if (tag === TAG.F64) return this.scalar("f64", pl);
    if (tag === TAG.STR) return this.hstr(pl); if (tag === TAG.BYTES) return this.hbytes(pl);
    if (tag === TAG.ARR) { const [n, p] = this.varint(pl); if (n) this.need(p, 5 * n); return new AnyArray(this, pl, n, p); }
    if (tag === TAG.OBJ) { const [n, p] = this.varint(pl); if (n) this.need(p, 9 * n); return new AnyObject(this, pl, n, p); }
    throw new JsonxError("E_BIN_INVALID", `unknown any-tag ${tag}`);
  }
}
const num = (v) => typeof v === "bigint" && v >= -(2n ** 53n) && v <= 2n ** 53n ? Number(v) : v;
export class StructView {
  constructor(r, idx, loc) { this.r = r; this.idx = idx; this.loc = loc; r.need(loc, r.fields(idx).size); }
  keys() { return this.r.fields(this.idx).lay.map((l) => l.field.name); }
  get(name) {
    const r = this.r, fl = r.fields(this.idx).by.get(name);
    if (!fl) { const f = r.sc.types[this.idx].byName.get(name); if (!f) throw new Error("no field " + name); return f.default ? (f.default[0] === "array" ? [] : num(f.default[1])) : null; }
    if (fl.pbyte !== null && !((r.u8(this.loc + fl.pbyte) >> fl.bit) & 1)) return null;
    return r.slotGet(fl.field.type, this.loc + fl.off, this.loc);
  }
}
export class ArrayView {
  constructor(r, elem, blk, n, start, stride) { this.r = r; this.elem = elem; this.blk = blk; this.length = n; this.start = start; this.stride = stride; }
  get(i) { if (i < 0 || i >= this.length) throw new RangeError("index"); const p = this.start + i * this.stride; return this.elem.kind === "struct" ? new StructView(this.r, this.elem.idx, p) : this.r.slotGet(this.elem, p, this.blk); }
  *[Symbol.iterator]() { for (let i = 0; i < this.length; i++) yield this.get(i); }
}
export class UnionView {
  constructor(r, idx, pos, ms) { this.r = r; this.idx = idx; this.pos = pos; this.ms = ms; r.need(pos, 5); this.tag = r.u[pos]; }
  get name() { const v = this.r.sc.types[this.idx].variants.find((x) => x[1] === this.tag); return v ? v[0] : null; }
  get value() { const v = this.r.sc.types[this.idx].variants.find((x) => x[1] === this.tag); return v ? this.r.heapGet(v[2], this.r.fwd(this.r.u32(this.pos + 1), this.ms, "union payload"), this.ms) : null; }
}
export class AnyArray {
  constructor(r, blk, n, start) { this.r = r; this.blk = blk; this.length = n; this.start = start; }
  get(i) { if (i < 0 || i >= this.length) throw new RangeError("index"); return this.r.anyGet(this.start + 5 * i, this.blk); }
  *[Symbol.iterator]() { for (let i = 0; i < this.length; i++) yield this.get(i); }
}
export class AnyObject {
  constructor(r, blk, n, start) { this.r = r; this.blk = blk; this.length = n; this.start = start; }
  keyAt(i) { return this.r.hstr(this.r.u32(this.start + 9 * i)); }
  keys() { return Array.from({ length: this.length }, (_, i) => this.keyAt(i)); }
  get(key) { for (let i = 0; i < this.length; i++) if (this.keyAt(i) === key) return this.r.anyGet(this.start + 9 * i + 4, this.blk); return undefined; }
}
/** Zero-copy view over `data` (Uint8Array/ArrayBuffer). Scalars are read on access. */
export function view(data, schema = null, opts = {}) { const r = new Reader(data, schema, opts); return r.dynamic ? r.anyGet(r.root, r.root) : new StructView(r, r.ridx, r.root); }

// ---- owned decoder (to Document, aliasing preserved)
class Dec {
  constructor(r) { this.r = r; this.memo = new Map(); this.pending = []; this.visited = 0; this.budget = Math.max(1 << 20, 256 * r.n); this.used = 0; }
  node(kind, value, parent, key, loc, t, depth) {
    if (depth > MAX_DEPTH) throw new JsonxError("E_BIN_LIMIT", `nesting deeper than ${MAX_DEPTH}`);
    if (++this.visited > this.r.n) throw new JsonxError("E_BIN_LIMIT", "decoded node count exceeds the blob size");
    const n = new Node(kind, value); n.parent = parent; n.key = key;
    if (loc !== null) { const k = loc + ":" + tk(t); if (this.memo.has(k)) throw new JsonxError("E_BIN_INVALID", `location ${loc} is used by two values`); this.memo.set(k, n); }
    return n;
  }
  charge(k) { this.used += k; if (this.used > this.budget) throw new JsonxError("E_BIN_LIMIT", "decoded string/bytes volume exceeds the expansion limit"); }
  run() {
    const r = this.r; const root = r.dynamic ? this.anyNode(r.root, r.root, null, null, 0) : this.structNode(r.ridx, r.root, null, null, 0);
    for (const [n, loc, t] of this.pending) { const tg = this.memo.get(loc + ":" + tk(t)); if (!tg) throw new JsonxError("E_BIN_INVALID_REF", `reference to offset ${loc} does not hit a value of type ${tk(t)}`); n.target = tg; }
    return root;
  }
  anyNode(pos, ms, parent, key, depth) {
    const r = this.r, tag = r.u8(pos), pl = r.u32(pos + 1);
    if (tag <= 2) { if (pl) throw new JsonxError("E_BIN_INVALID", "nonzero payload on constant tag"); return this.node(tag === 0 ? "null" : "bool", tag === 0 ? null : tag === 2, parent, key, pos, ANY, depth); }
    if (tag === TAG.I32) return this.node("int", BigInt(r.dv.getInt32(pos + 1, true)), parent, key, pos, ANY, depth);
    if (tag === TAG.REF) { const n = this.node("ref", null, parent, key, null, ANY, depth); this.pending.push([n, pl, ANY]); r.need(pl, 5); return n; }
    r.fwd(pl, ms, "value");
    if (tag === TAG.I64) return this.node("int", r.scalar("i64", pl), parent, key, pos, ANY, depth);
    if (tag === TAG.U64) return this.node("int", r.scalar("u64", pl), parent, key, pos, ANY, depth);
    if (tag === TAG.F64) return this.node("float", r.scalar("f64", pl), parent, key, pos, ANY, depth);
    if (tag === TAG.STR) { const s = r.hstr(pl); this.charge(s.length); return this.node("string", s, parent, key, pos, ANY, depth); }
    if (tag === TAG.BYTES) { const b = r.hbytes(pl).slice(); this.charge(b.length); return this.node("bytes", b, parent, key, pos, ANY, depth); }
    if (tag === TAG.ARR) {
      const [n, p] = r.varint(pl); if (n) r.need(p, 5 * n); const node = this.node("array", null, parent, key, pos, ANY, depth); node.items = [];
      for (let i = 0; i < n; i++) node.items.push(this.anyNode(p + 5 * i, pl, node, i, depth + 1)); return node;
    }
    if (tag === TAG.OBJ) {
      const [n, p] = r.varint(pl); if (n) r.need(p, 9 * n); const node = this.node("object", null, parent, key, pos, ANY, depth); node.items = new Map(); node.labels = [];
      for (let i = 0; i < n; i++) { const e = p + 9 * i, k = r.hstr(r.u32(e)); this.charge(k.length); if (node.items.has(k)) throw new JsonxError("E_BIN_INVALID", `duplicate object key ${k}`); node.items.set(k, this.anyNode(e + 4, pl, node, k, depth + 1)); }
      return node;
    }
    throw new JsonxError("E_BIN_INVALID", `unknown any-tag ${tag}`);
  }
  structNode(idx, base, parent, key, depth) {
    const r = this.r, { lay, size } = r.fields(idx), sc = r.sc; r.need(base, size);
    const node = this.node("object", null, parent, key, base, T("struct", idx), depth); node.items = new Map(); node.labels = []; const have = new Set();
    for (const fl of lay) {
      const f = fl.field; have.add(f.name);
      if (fl.pbyte !== null && !((r.u8(base + fl.pbyte) >> fl.bit) & 1)) { if (f.nullable) { const c = new Node("null"); c.parent = node; c.key = f.name; node.items.set(f.name, c); } continue; }
      node.items.set(f.name, this.slotNode(f.type, base + fl.off, base, node, f.name, depth + 1));
    }
    for (const f of sc.types[idx].fields) if (!have.has(f.name) && f.default) { const c = mkDefault(f.default); c.parent = node; c.key = f.name; node.items.set(f.name, c); }
    return node;
  }
  slotNode(t, pos, ms, parent, key, depth) {
    const r = this.r, k = t.kind;
    if (k === "bool") return this.node("bool", r.scalar("bool", pos), parent, key, pos, t, depth);
    if (k in INTS) return this.node("int", BigInt(r.scalar(k, pos)), parent, key, pos, t, depth);
    if (k === "f64") return this.node("float", r.scalar("f64", pos), parent, key, pos, t, depth);
    if (k === "f32") return this.node("f32", r.scalar("f32", pos), parent, key, pos, t, depth);
    if (k === "enum") return this.enumNode(t, pos, parent, key, depth);
    if (k === "any") return this.anyNode(pos, ms, parent, key, depth);
    if (k === "ref") { const n = this.node("ref", null, parent, key, null, t, depth), loc = r.u32(pos); r.need(loc, 1); this.pending.push([n, loc, t.elem]); return n; }
    if (k === "union") {
      const un = r.sc.types[t.idx], tag = r.u8(pos), off = r.fwd(r.u32(pos + 1), ms, "union payload"); const v = un.variants.find((x) => x[1] === tag);
      if (!v) throw new JsonxError("E_BIN_INVALID", `unknown union tag ${tag} in ${un.name}`);
      const node = this.node("object", null, parent, key, pos, t, depth); node.items = new Map(); node.labels = [];
      node.items.set(v[0], this.heapNode(v[2], off, ms, node, v[0], depth + 1)); return node;
    }
    const off = r.fwd(r.u32(pos), ms, k); return this.heapNode(t, off, ms, parent, key, depth, pos);
  }
  enumNode(t, pos, parent, key, depth) { const [v, en] = this.r.enumRaw(t, pos); const m = en.values.find(([, x]) => x === v); return m ? this.node("string", m[0], parent, key, pos, t, depth) : this.node("int", BigInt(v), parent, key, pos, t, depth); }
  heapNode(t, off, ms, parent, key, depth, slotpos = null) {
    const r = this.r, k = t.kind;
    if (k === "string") { const s = r.hstr(off); this.charge(s.length); return this.node("string", s, parent, key, slotpos, t, depth); }
    if (k === "bytes") { const b = r.hbytes(off).slice(); this.charge(b.length); return this.node("bytes", b, parent, key, slotpos, t, depth); }
    if (k === "struct") return this.structNode(t.idx, off, parent, key, depth);
    if (k === "array") {
      const av = r.arrayView(t, off); const node = this.node("array", null, parent, key, slotpos, t, depth); node.items = [];
      for (let i = 0; i < av.length; i++) { const p = av.start + i * av.stride; node.items.push(t.elem.kind === "struct" ? this.structNode(t.elem.idx, p, node, i, depth + 1) : this.slotNode(t.elem, p, off, node, i, depth + 1)); }
      return node;
    }
    if (k in SCALAR_SIZE || k === "enum") return this.slotNode(t, off, ms, parent, key, depth);
    throw new JsonxError("E_BIN_INVALID", `type ${k} cannot be stored by offset`);
  }
}
export function decode(data, schema = null, opts = {}) { return new Document(new Dec(new Reader(data, schema, opts)).run()); }
export function blobLength(h) { if (h.length < HDR) throw new JsonxError("E_BIN_TRUNCATED", "need 28 header bytes"); return new DataView(h.buffer, h.byteOffset).getUint32(24, true); }
