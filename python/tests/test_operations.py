"""Every operation of the catalogue, called through its generated method with the
arguments its fixture was taken with: the exact request, the whole envelope back,
and a strict check of the fixture against the catalogue types."""

from __future__ import annotations

from typing import Any
from urllib.parse import parse_qsl

import pytest
from _support import MODES, make, run
from conftest import (
    IR,
    KEY,
    SAMPLES,
    encode_uri_component,
    fixture,
    json_answer,
    validate,
)


def _label(op: dict[str, Any]) -> str:
    return f"{op['namespace'] or 'client'}.{op['pyMethod']}"


@pytest.mark.parametrize("mode", MODES)
@pytest.mark.parametrize("op", IR["operations"], ids=_label)
def test_operation(op: dict[str, Any], mode: str) -> None:
    body = fixture(op["id"])
    tf, mock = make(mode, [json_answer(body)], max_retries=0)
    own = SAMPLES["operations"].get(op["id"], {})

    def value(name: str) -> Any:
        for source in (own.get("path", {}), SAMPLES["byNamespace"].get(op["namespace"] or "", {}), SAMPLES["defaults"]):
            if name in source:
                return source[name]
        raise KeyError(f"{op['id']}: no sample for {name}")

    args = [value(p["name"]) for p in op["pathParams"]]
    query: dict[str, Any] = {}
    if any(q["name"] == "limit" for q in op["queryParams"]):
        query["limit"] = int(SAMPLES["limit"])
    query.update(own.get("query", {}))
    py = {q["name"]: q["py"] for q in op["queryParams"]}
    kwargs = {py[k]: v for k, v in query.items()}

    target = getattr(tf, op["namespace"]) if op["namespace"] else tf
    res = run(mode, tf, lambda _: getattr(target, op["pyMethod"])(*args, **kwargs))

    assert res == body, "the envelope comes back whole"
    assert len(mock.calls) == 1
    req = mock.calls[0]
    path = op["path"].replace("{chainId}", "1")
    for p in op["pathParams"]:
        path = path.replace("{" + p["name"] + "}", encode_uri_component(str(value(p["name"]))))
    assert req.url.raw_path.decode().split("?")[0] == "/api/v1" + path
    assert req.url.host == "tracefast.xyz" and req.url.scheme == "https"
    sent = parse_qsl(req.url.query.decode(), keep_blank_values=True)
    assert len(sent) == len(dict(sent)), "no parameter is sent twice"
    assert dict(sent) == {k: str(v) for k, v in query.items()}, "only the given parameters are sent"
    assert req.method == "GET"
    assert req.headers["x-api-key"] == KEY
    assert req.headers["accept"] == "application/json"
    assert req.headers["user-agent"].startswith("tracefast-python/")
    assert KEY not in str(req.url)

    problems: list[str] = []
    validate(body, op["response"], op["id"], problems)
    assert problems == [], "the fixture matches the catalogue"


def test_every_operation_has_a_fixture_and_a_method() -> None:
    from tracefast import AsyncTraceFast, TraceFast

    for cls in (TraceFast, AsyncTraceFast):
        tf = cls(api_key=KEY)
        for op in IR["operations"]:
            target = getattr(tf, op["namespace"]) if op["namespace"] else tf
            assert callable(getattr(target, op["pyMethod"]))
            if op["paged"]:
                assert callable(getattr(target, op["pyMethod"] + "_pages"))
            fixture(op["id"])
    assert len(IR["operations"]) == 97
