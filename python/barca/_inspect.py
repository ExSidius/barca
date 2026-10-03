"""Artifact shape for `barca status` — invoked by Rust as `python -m barca._inspect`.

Reads only artifact files, never user code:

  - parquet: row count and column schema from the file footer (needs pyarrow; without it the
    shape says so in `note`). `sample` reads just the first rows.
  - json:    the value's type; for a list, its length and (for a list of objects) the column
    names with the JSON types seen; for an object, its keys.
  - pickle:  the type of the top-level object, read from the pickle opcodes WITHOUT unpickling,
    so no module is imported and no code runs. Pickles are never sampled.

Protocol: one JSON document on stdin, {"sample": N, "artifacts": [{"path", "format"}, ...]};
one JSON array on stdout with a shape object per artifact, in order.
"""

import json
import os
import pickletools
import sys
from typing import Any

from barca import _storage

MAX_KEYS = 100


def shape(path: str, fmt: str, sample: int = 0) -> dict:
    """Shape of one artifact file. Never raises: problems are reported in `note`."""
    if _storage.is_remote(path):
        return {"note": "remote artifact; shape is only read for local files"}
    if not os.path.exists(path):
        return {"note": "artifact file not found"}
    try:
        if fmt == "parquet":
            return _parquet_shape(path, sample)
        if fmt == "json":
            return _json_shape(path, sample)
        if fmt == "pickle":
            return {"type": _pickle_type(path)}
        return {"note": f"unknown format '{fmt}'"}
    except Exception as e:  # noqa: BLE001 — report, never fail the status command
        return {"note": f"could not read artifact: {type(e).__name__}: {e}"}


# ─── parquet ──────────────────────────────────────────────────────────────────


def _parquet_shape(path: str, sample: int) -> dict:
    try:
        import pyarrow.parquet as pq
    except ImportError:
        return {"note": "pyarrow is not installed; install barca[parquet] to read parquet shape"}
    pf = pq.ParquetFile(path)
    schema = pf.schema_arrow
    out: dict[str, Any] = {
        "type": "table",
        "rows": pf.metadata.num_rows,
        "columns": [{"name": f.name, "type": str(f.type)} for f in schema],
    }
    if sample > 0:
        rows: list = []
        for batch in pf.iter_batches(batch_size=sample):
            rows.extend(batch.to_pylist())
            if len(rows) >= sample:
                break
        out["sample"] = _jsonable(rows[:sample])
    return out


def _jsonable(value: Any) -> Any:
    """Round-trip through json so dates, decimals and bytes print as strings."""
    return json.loads(json.dumps(value, default=str))


# ─── json ─────────────────────────────────────────────────────────────────────


def _json_type(v: Any) -> str:
    if v is None:
        return "null"
    return type(v).__name__


def _json_shape(path: str, sample: int) -> dict:
    with open(path) as f:
        value = json.load(f)
    out: dict[str, Any] = {"type": _json_type(value)}
    if isinstance(value, list):
        out["rows"] = len(value)
        if value and all(isinstance(r, dict) for r in value):
            seen: dict[str, list[str]] = {}
            for row in value:
                for k, v in row.items():
                    types = seen.setdefault(k, [])
                    t = _json_type(v)
                    if t not in types:
                        types.append(t)
            out["columns"] = [
                {"name": k, "type": " | ".join(sorted(ts, key=lambda t: t == "null"))}
                for k, ts in seen.items()
            ]
        if sample > 0:
            out["sample"] = value[:sample]
    elif isinstance(value, dict):
        keys = list(value)
        out["keys"] = keys[:MAX_KEYS]
        if len(keys) > MAX_KEYS:
            out["key_count"] = len(keys)
        if sample > 0:
            out["sample"] = {k: value[k] for k in keys[:sample]}
    elif sample > 0:
        out["sample"] = value
    return out


# ─── pickle ───────────────────────────────────────────────────────────────────

# Opcodes that push a value of a known builtin type.
_PUSH_TYPES = {
    "EMPTY_DICT": "dict",
    "DICT": "dict",
    "EMPTY_LIST": "list",
    "LIST": "list",
    "EMPTY_SET": "set",
    "FROZENSET": "frozenset",
    "EMPTY_TUPLE": "tuple",
    "TUPLE": "tuple",
    "TUPLE1": "tuple",
    "TUPLE2": "tuple",
    "TUPLE3": "tuple",
    "NONE": "NoneType",
    "NEWTRUE": "bool",
    "NEWFALSE": "bool",
    "INT": "int",
    "BININT": "int",
    "BININT1": "int",
    "BININT2": "int",
    "LONG": "int",
    "LONG1": "int",
    "LONG4": "int",
    "FLOAT": "float",
    "BINFLOAT": "float",
    "STRING": "str",
    "BINSTRING": "str",
    "SHORT_BINSTRING": "str",
    "UNICODE": "str",
    "BINUNICODE": "str",
    "SHORT_BINUNICODE": "str",
    "BINUNICODE8": "str",
    "BINBYTES": "bytes",
    "SHORT_BINBYTES": "bytes",
    "BINBYTES8": "bytes",
    "BYTEARRAY8": "bytearray",
}


class _Class:
    """A class reference seen in the opcode stream (GLOBAL / STACK_GLOBAL)."""

    def __init__(self, name: str):
        self.name = name


class _Str:
    def __init__(self, value: str):
        self.value = value


class _Tuple:
    def __init__(self, items: list):
        self.items = items


class _Mark:
    pass


# Opcodes that mutate the object below their operands and leave it on the stack.
_MUTATORS = {"APPEND", "APPENDS", "SETITEM", "SETITEMS", "ADDITEMS", "BUILD"}


def _label(item: Any) -> str:
    if isinstance(item, _Class):
        return item.name
    if isinstance(item, _Tuple):
        return "tuple"
    if isinstance(item, _Str):
        return "str"
    if isinstance(item, str):
        return item
    return "unknown"


def _pickle_type(path: str) -> str:
    """Type of the top-level object, by simulating the pickle VM's stack over type labels."""
    mark = pickletools.markobject
    stack: list = []
    memo: dict = {}
    with open(path, "rb") as f:
        for op, arg, _pos in pickletools.genops(f):
            name = op.name
            if name in ("PROTO", "FRAME"):
                continue
            if name == "STOP":
                return _label(stack[-1]) if stack else "unknown"
            if name == "MARK":
                stack.append(_Mark())
            elif name == "MEMOIZE":
                memo[len(memo)] = stack[-1]
            elif name in ("PUT", "BINPUT", "LONG_BINPUT"):
                memo[arg] = stack[-1]
            elif name in ("GET", "BINGET", "LONG_BINGET"):
                stack.append(memo.get(arg, "unknown"))
            elif name == "GLOBAL":
                module, _, qual = str(arg).partition(" ")
                stack.append(_Class(_qualname(module, qual)))
            elif name == "STACK_GLOBAL":
                qual, module = stack.pop(), stack.pop()
                stack.append(_Class(_qualname(_str_value(module), _str_value(qual))))
            elif name in ("SHORT_BINUNICODE", "BINUNICODE", "UNICODE", "BINUNICODE8"):
                stack.append(_Str(str(arg)))
            elif name in ("TUPLE1", "TUPLE2", "TUPLE3"):
                n = int(name[-1])
                items = stack[-n:]
                del stack[-n:]
                stack.append(_Tuple(items))
            elif name == "TUPLE":
                stack.append(_Tuple(_pop_to_mark(stack)))
            elif name == "NEWOBJ":
                stack.pop()  # args
                stack.append(_label(stack.pop()))
            elif name == "NEWOBJ_EX":
                stack.pop()  # kwargs
                stack.pop()  # args
                stack.append(_label(stack.pop()))
            elif name == "REDUCE":
                args = stack.pop()
                stack.append(_reduce_label(stack.pop(), args))
            elif name == "OBJ":
                items = _pop_to_mark(stack)
                stack.append(_label(items[0]) if items else "unknown")
            elif name == "INST":
                _pop_to_mark(stack)
                module, _, qual = str(arg).partition(" ")
                stack.append(_qualname(module, qual))
            elif name in _MUTATORS:
                if mark in op.stack_before:
                    _pop_to_mark(stack)
                else:
                    for _ in op.stack_before[1:]:
                        stack.pop()
            else:
                # Generic opcode: pop its operands, push its results (typed when known).
                before = op.stack_before
                if mark in before:
                    _pop_to_mark(stack)
                    for _ in range(before.index(mark)):
                        stack.pop()
                else:
                    for _ in before:
                        stack.pop()
                for _ in op.stack_after:
                    stack.append(_PUSH_TYPES.get(name, "unknown"))
    return "unknown"


def _pop_to_mark(stack: list) -> list:
    items: list = []
    while stack:
        top = stack.pop()
        if isinstance(top, _Mark):
            break
        items.append(top)
    items.reverse()
    return items


def _str_value(item: Any) -> str:
    return item.value if isinstance(item, _Str) else "?"


def _qualname(module: str, qual: str) -> str:
    return qual if module in ("builtins", "__builtin__") else f"{module}.{qual}"


def _reduce_label(fn: Any, args: Any) -> str:
    """A REDUCE builds `fn(*args)`. Reconstructor helpers (copyreg._reconstructor, numpy's
    _reconstruct, copyreg.__newobj__) take the real class as their first argument."""
    name = _label(fn)
    short = name.rsplit(".", 1)[-1]
    if (
        (short.startswith("_reconstruct") or short == "__newobj__")
        and isinstance(args, _Tuple)
        and args.items
        and isinstance(args.items[0], _Class)
    ):
        return args.items[0].name
    if name.startswith("numpy.") and short == "_frombuffer":
        return "numpy.ndarray"  # protocol 5 arrays
    return name


def main() -> None:
    req = json.load(sys.stdin)
    sample = int(req.get("sample") or 0)
    shapes = [shape(a["path"], a.get("format", ""), sample) for a in req.get("artifacts", [])]
    sys.stdout.write(json.dumps(shapes, default=str))


if __name__ == "__main__":
    main()
