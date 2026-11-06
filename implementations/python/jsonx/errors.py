"""JSONX error model. Codes are shared by every implementation (see specification/errors.md)."""
from __future__ import annotations

CATEGORIES = {
    "E_SYNTAX": "syntax", "E_UNEXPECTED_EOF": "syntax", "E_INVALID_ESCAPE": "syntax",
    "E_INVALID_UTF8": "syntax", "E_DEPTH_LIMIT": "syntax", "E_DUPLICATE_KEY": "syntax",
    "E_INVALID_NUMBER": "number", "E_NUMBER_OVERFLOW": "number",
    "E_INVALID_LABEL_NAME": "label", "E_RESERVED_LABEL": "label", "E_DUPLICATE_LABEL": "label",
    "E_UNDEFINED_LABEL": "label", "E_LABEL_OUT_OF_SCOPE": "label", "E_AMBIGUOUS_LABEL": "label",
    "E_AMBIGUOUS_NAME": "label",
    "E_INVALID_POINTER": "reference", "E_POINTER_NO_FIELD": "reference", "E_POINTER_TYPE": "reference",
    "E_INVALID_INDEX": "reference", "E_INVALID_PARENT": "reference", "E_INVALID_CURRENT": "reference",
    "E_CYCLIC_REFERENCE": "reference",
    "E_INVALID_PREV": "context", "E_INVALID_NEXT": "context",
    "E_SCHEMA_SYNTAX": "schema", "E_SCHEMA_UNKNOWN_TYPE": "schema", "E_SCHEMA_DUPLICATE": "schema",
    "E_SCHEMA_INVALID": "schema", "E_SCHEMA_BINARY": "schema", "E_SCHEMA_IMPORT": "schema",
    "E_TYPE_MISMATCH": "type", "E_MISSING_FIELD": "type", "E_UNKNOWN_FIELD": "type",
    "E_RANGE": "type", "E_CONSTRAINT": "type", "E_REF_TARGET": "type",
    "E_BIN_MAGIC": "binary", "E_BIN_VERSION": "binary", "E_BIN_TRUNCATED": "binary",
    "E_BIN_BOUNDS": "binary", "E_BIN_CHECKSUM": "binary", "E_BIN_SCHEMA_MISMATCH": "binary",
    "E_BIN_INVALID": "binary", "E_BIN_UTF8": "binary", "E_BIN_LIMIT": "binary",
    "E_BIN_INVALID_REF": "binary",
}


class JsonxError(Exception):
    def __init__(self, code, message, *, pos=None, line=None, col=None,
                 expected=None, actual=None, hint=None):
        super().__init__(message)
        self.code, self.message = code, message
        self.pos, self.line, self.col = pos, line, col
        self.expected, self.actual, self.hint = expected, actual, hint

    @property
    def category(self) -> str:
        return CATEGORIES.get(self.code, "unknown")

    def __str__(self):
        loc = f" at {self.line}:{self.col}" if self.line is not None else ""
        s = f"{self.code}{loc}: {self.message}"
        if self.expected is not None or self.actual is not None:
            s += f" (expected {self.expected}, got {self.actual})"
        if self.hint:
            s += f"\n  hint: {self.hint}"
        return s


def line_col(src: str, pos: int):
    """1-based line and column (column counted in code points). \\n, \\r\\n and \\r end a line."""
    line, ls, i, n = 1, 0, 0, len(src)
    while i < pos and i < n:
        c = src[i]
        if c == "\n":
            line += 1; ls = i + 1
        elif c == "\r":
            if i + 1 < n and src[i + 1] == "\n":
                i += 1
            line += 1; ls = i + 1
        i += 1
    return line, pos - ls + 1
