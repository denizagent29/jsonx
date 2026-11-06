"""JSONX: text + binary serialization with native references, labels and schemas."""
from .errors import JsonxError
from .text import parse, loads, dumps, dump, Document, Node
from .schema import Schema, compile_schema, compile_file, load_schema
from .binary import encode, decode, view, Reader, blob_length
__version__ = "0.1.0"
__all__ = ["JsonxError", "parse", "loads", "dumps", "dump", "Document", "Node", "Schema", "compile_schema",
           "compile_file", "load_schema", "encode", "decode", "view", "Reader", "blob_length"]
