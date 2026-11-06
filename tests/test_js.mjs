import fs from "node:fs"; import path from "node:path"; import { fileURLToPath } from "node:url";
import * as J from "../implementations/js/src/jsonx.mjs";
const V = path.join(path.dirname(fileURLToPath(import.meta.url)), "vectors");
const rd = (p) => fs.readFileSync(path.join(V, p), "utf8"), rb = (p) => new Uint8Array(fs.readFileSync(path.join(V, p)));
let n = 0, fail = 0; const ok = (c, m) => { n++; if (!c) { fail++; console.log("FAIL", m); } };
const eq = (a, b) => a.length === b.length && a.every((x, i) => x === b[i]);
const code = (f) => { try { f(); return "OK"; } catch (e) { if (!(e instanceof J.JsonxError)) return "CRASH " + e.stack; return `${e.code} ${e.line}:${e.col}`; } };
const ls = (d, ext) => fs.readdirSync(path.join(V, d)).filter((f) => f.endsWith(ext)).map((f) => f.slice(0, -ext.length)).sort();
for (const nm of ls("text/valid", ".jsonx")) {
  const d = J.parse(rd(`text/valid/${nm}.jsonx`)); ok(d.dump() === rd(`text/valid/${nm}.dump`), nm + " dump");
  const b = J.encode(d); ok(eq(b, rb(`text/valid/${nm}.bjsonx`)), nm + " golden bytes"); ok(J.decode(b).dump() === d.dump(), nm + " roundtrip");
  ok(eq(J.encode(d, null, { checksum: true }), rb(`text/valid/${nm}.crc.bjsonx`)), nm + " crc");
  ok(J.decode(rb(`text/valid/${nm}.bjsonx`)).dump() === rd(`text/valid/${nm}.dump`), nm + " decode golden");
}
for (const nm of ls("text/errors", ".jsonx")) ok(code(() => J.parse(rd(`text/errors/${nm}.jsonx`))) === rd(`text/errors/${nm}.err`).trim(), "error " + nm + " got " + code(() => J.parse(rd(`text/errors/${nm}.jsonx`))));
const sc = J.loadSchema(rb("schema/lesson.jxsb")), sc1 = J.loadSchema(rb("schema/lesson_v1.jxsb"));
ok(sc.id.toString(16) === rd("schema/ids.txt").match(/lesson_v2_id=(\w+)/)[1].replace(/^0+/, ""), "schema id");
for (const nm of rd("typed/manifest.txt").split("\n").filter(Boolean)) {
  const d = J.parse(rd(`typed/${nm}.jsonx`)), b = J.encode(d, sc);
  ok(eq(b, rb(`typed/${nm}.bjsonx`)), nm + " typed golden"); ok(J.decode(b, sc).dump() === rd(`typed/${nm}.dump`), nm + " typed dump");
  ok(eq(J.encode(d, sc, { checksum: true }), rb(`typed/${nm}.crc.bjsonx`)), nm + " typed crc");
  ok(eq(J.encode(J.decode(b, sc), sc), b), nm + " re-encode identical");
}
for (const nm of ls("typed/errors", ".jsonx")) { const g = code(() => J.encode(J.parse(rd(`typed/errors/${nm}.jsonx`)), sc)); ok(g === rd(`typed/errors/${nm}.err`).trim(), `typed error ${nm}: ${g}`); }
ok(J.decode(rb("compat/v1_data.bjsonx"), sc).dump() === rd("compat/v1_data_read_by_v2.dump"), "v1 data / v2 reader");
ok(J.decode(rb("compat/v2_data.bjsonx"), sc1).dump() === rd("compat/v2_data_read_by_v1.dump"), "v2 data / v1 reader");
ok(J.view(rb("compat/v2_data.bjsonx"), sc1).get("users").get(1).get("name") === "b", "strided view");
for (const nm of ls("binary/malformed", ".bin")) { const g = code(() => J.decode(rb(`binary/malformed/${nm}.bin`), nm.startsWith("typed") ? sc : null)); ok(g.split(" ")[0] === rd(`binary/malformed/${nm}.err`).trim(), `malformed ${nm}: ${g}`); }
{ const d = J.parse(rd("text/valid/refs_basic.jsonx")), o = J.decode(J.encode(d)).toJS(); ok(o.user_related_to === o.users[1], "alias identity after binary"); }
{ const o = J.decode(J.encode(J.parse(rd("text/valid/structure_cycle.jsonx")))).toJS(); ok(o.node.up === o && o.node.self === o.node, "cycle preserved"); }
{ const v = J.view(rb("typed/lesson.bjsonx"), sc); ok(v.get("attendees").get(0).get("user").get("name") === "Adil", "view through ref"); ok(v.get("shape").name === "circle", "union view"); ok(v.get("attendees").get(0).get("uid") === 23n || v.get("attendees").get(0).get("uid") === 23, "field ref"); }
let seed = 12345; const rnd = (m) => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) % m;
const seeds = [[rb("typed/lesson.bjsonx"), sc], [rb("typed/lesson_many_users.bjsonx"), sc], [rb("text/valid/many_refs.bjsonx"), null], [rb("text/valid/scalars.bjsonx"), null]];
for (let it = 0; it < 4000; it++) { const [b0, s] = seeds[rnd(4)]; let m = b0.slice(); for (let k = 0; k <= rnd(4); k++) m[rnd(m.length)] = rnd(256); if (rnd(5) === 0) m = m.slice(0, rnd(m.length)); const g = code(() => J.decode(m, s)); ok(!g.startsWith("CRASH"), g); }
console.log(`js: ${n} checks, ${fail} failed`); process.exit(fail ? 1 : 0);
