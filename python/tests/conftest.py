"""Shared test helpers: the IR the SDK was generated from, the fixtures, a mock transport.

The IR comes from ``codegen/ir.mjs`` through Node, with the Python names the
emitter gives (so the tests and the generator cannot drift). Node is required.
"""

from __future__ import annotations

import json
import shutil
import subprocess
from collections.abc import Callable
from pathlib import Path
from typing import Any

import httpx

ROOT = Path(__file__).resolve().parents[2]
KEY = "tf_test_key"

_DUMP = """
const { loadIr } = await import(process.argv[1] + '/codegen/ir.mjs')
const { pyMethod, pyName } = await import(process.argv[1] + '/codegen/emit-python.mjs')
const ir = loadIr()
for (const op of ir.operations) {
  op.pyMethod = pyMethod(op)
  for (const p of [...op.pathParams, ...op.queryParams]) p.py = pyName(p.name)
}
process.stdout.write(JSON.stringify({ version: ir.version, operations: ir.operations, types: Object.fromEntries(ir.types) }))
"""


def _load_ir() -> dict[str, Any]:
    node = shutil.which("node")
    if node is None:
        raise RuntimeError("the tests need Node.js: the IR is read from codegen/ir.mjs")
    out = subprocess.run(
        [node, "--input-type=module", "-e", _DUMP, str(ROOT)],
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(out.stdout)  # type: ignore[no-any-return]


IR = _load_ir()
SAMPLES: dict[str, Any] = json.loads((ROOT / "fixtures" / "samples.json").read_text())


def fixture(op_id: str) -> Any:
    return json.loads((ROOT / "fixtures" / f"{op_id}.json").read_text())


def validate(value: Any, type_: dict[str, Any], path: str, out: list[str]) -> None:
    """Strict walk of a value against an IR type: required, nullability, types, unknown fields."""
    kind = type_["kind"]
    if kind == "ref":
        t = IR["types"][type_["name"]]
        if t["kind"] == "alias":
            return validate(value, t["type"], path, out)
        if not isinstance(value, dict):
            out.append(f"{path}: expected object {type_['name']}")
            return
        known = set()
        for f in t["fields"]:
            known.add(f["name"])
            if f["name"] not in value:
                if f["required"]:
                    out.append(f"{path}.{f['name']}: required, absent")
                continue
            if value[f["name"]] is None:
                if not f["nullable"]:
                    out.append(f"{path}.{f['name']}: null, not nullable")
                continue
            validate(value[f["name"]], f["type"], f"{path}.{f['name']}", out)
        for k in value:
            if k in known:
                continue
            if t.get("extra"):
                validate(value[k], t["extra"]["type"], f"{path}.{k}", out)
            else:
                out.append(f"{path}.{k}: not in the catalogue")
    elif kind == "union":
        # One variant must take the value whole.
        tries: list[list[str]] = []
        for v in type_["variants"]:
            errs: list[str] = []
            validate(value, v["type"], path, errs)
            tries.append(errs)
        if not any(not e for e in tries):
            out.append(f"{path}: matches no variant ({'; '.join(e[0] for e in tries)})")
    elif kind == "array":
        if not isinstance(value, list):
            out.append(f"{path}: expected array")
            return
        for i, v in enumerate(value):
            validate(v, type_["items"], f"{path}[{i}]", out)
    elif kind == "map":
        if not isinstance(value, dict):
            out.append(f"{path}: expected map")
            return
        for k, v in value.items():
            validate(v, type_["values"], f"{path}.{k}", out)
    elif kind in ("string", "enum"):
        if not isinstance(value, str):
            out.append(f"{path}: expected string, got {type(value).__name__}")
    elif kind == "integer":
        # bool is an int in Python; a JSON integer may parse as float only if written 1.0
        if isinstance(value, bool) or not (
            isinstance(value, int) or (isinstance(value, float) and value.is_integer())
        ):
            out.append(f"{path}: expected integer")
    elif kind == "number":
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            out.append(f"{path}: expected number")
    elif kind == "boolean":
        if not isinstance(value, bool):
            out.append(f"{path}: expected boolean")


Answer = Callable[[httpx.Request], httpx.Response]


def json_answer(body: Any, status: int = 200, headers: dict[str, str] | None = None) -> Answer:
    def answer(request: httpx.Request) -> httpx.Response:
        return httpx.Response(status, json=body, headers=headers or {})

    return answer


class Mock:
    """An httpx transport that answers from a list, recording every request."""

    def __init__(self, answers: list[Answer]) -> None:
        self.answers = list(answers)
        self.calls: list[httpx.Request] = []

    def _handle(self, request: httpx.Request) -> httpx.Response:
        self.calls.append(request)
        if not self.answers:
            raise AssertionError("no more answers")
        return self.answers.pop(0)(request)

    def transport(self) -> httpx.MockTransport:
        return httpx.MockTransport(self._handle)


def encode_uri_component(s: str) -> str:
    """JavaScript's ``encodeURIComponent``, written out independently of the SDK."""
    keep = set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()")
    return "".join(c if c in keep else "".join(f"%{b:02X}" for b in c.encode()) for c in s)
