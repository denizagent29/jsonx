import { parse, encode, decode, view, loadSchema, JsonxError, StructView, ArrayView, type Document } from "../implementations/js/src/jsonx.mjs";
const doc: Document = parse("{ a: [1, 2], b: @a.1 }");
const blob: Uint8Array = encode(doc, null, { checksum: true });
const v = view(blob);
const back: string = decode(blob).dump();
try { parse("{"); } catch (e) { if (e instanceof JsonxError) { const c: string = e.code; } }
declare const sc: ReturnType<typeof loadSchema>;
const tv = view(blob, sc); if (tv instanceof StructView) { const n = tv.get("users"); if (n instanceof ArrayView) n.length.toFixed(); }
