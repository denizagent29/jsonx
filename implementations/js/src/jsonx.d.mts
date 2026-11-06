export class JsonxError extends Error {
  code: string; pos: number | null; line: number | null; col: number | null;
}
export type NodeKind = "null" | "bool" | "int" | "float" | "f32" | "string" | "bytes" | "array" | "object" | "ref";
export class Node {
  kind: NodeKind; value: unknown; pos: number; parent: Node | null; key: string | number | null;
  items: Node[] | Map<string, Node> | null; target: Node | null;
  deref(): Node; path(): string; get(key: string | number): Node;
}
export class Document {
  root: Node; src: string;
  /** Canonical dump shared by all JSONX implementations. */
  dump(): string;
  /** Plain JS values; references to objects/arrays preserve identity; integers beyond 2^53 become BigInt. */
  toJS(): unknown;
}
export function parse(text: string | Uint8Array): Document;
export function dump(node: Node): string;
export function lineCol(src: string, pos: number): [number, number];

export class Schema {
  namespace: string; version: number; root: number | null;
  static fromBytes(data: Uint8Array | ArrayBuffer): Schema;
  readonly id: bigint;
}
export function loadSchema(data: Uint8Array | ArrayBuffer): Schema;
export function fnv1a64(bytes: Uint8Array): bigint;
export function crc32(data: Uint8Array, end?: number): number;

export interface EncodeOptions { version?: number; root?: string; checksum?: boolean }
export interface ReadOptions { verifyCrc?: boolean; strictVersion?: boolean }
/** Encode to BJSONX. Without a schema the dynamic (`any`) layout is used. */
export function encode(doc: Document | Node, schema?: Schema | null, opts?: EncodeOptions): Uint8Array;
/** Validate the whole blob and build an owned Document (aliasing preserved). */
export function decode(data: Uint8Array | ArrayBuffer, schema?: Schema | null, opts?: ReadOptions): Document;
export function blobLength(header: Uint8Array): number;

export type Scalar = null | boolean | number | bigint | string | Uint8Array;
export type Value = Scalar | StructView | ArrayView | UnionView | AnyArray | AnyObject;
/** Zero-copy views read lazily from the underlying buffer; byte fields alias it. */
export class StructView { keys(): string[]; get(name: string): Value }
export class ArrayView implements Iterable<Value> { readonly length: number; get(i: number): Value; [Symbol.iterator](): Iterator<Value> }
export class UnionView { readonly tag: number; readonly name: string | null; readonly value: Value }
export class AnyArray implements Iterable<Value> { readonly length: number; get(i: number): Value; [Symbol.iterator](): Iterator<Value> }
export class AnyObject { readonly length: number; keys(): string[]; get(key: string): Value | undefined }
export function view(data: Uint8Array | ArrayBuffer, schema?: Schema | null, opts?: ReadOptions): Value;
