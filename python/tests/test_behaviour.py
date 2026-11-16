"""The behaviour every SDK shares (SPEC.md): errors, retries, pagination, the client."""

from __future__ import annotations

import asyncio
import pickle
import traceback
from datetime import datetime, timedelta, timezone
from email.utils import format_datetime
from typing import Any
from urllib.parse import parse_qs

import httpx
import pytest
from _support import MODES, collect, make, run
from conftest import KEY, json_answer

from tracefast import AsyncTraceFast, TraceFast, TraceFastError

USDC = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
OK = json_answer({"data": {"number": 1}, "meta": {}})


def raises(mode: str, tf: Any, fn: Any) -> TraceFastError:
    with pytest.raises(TraceFastError) as info:
        run(mode, tf, fn)
    return info.value


def error(code: str, status: int, headers: dict[str, str] | None = None) -> Any:
    return json_answer({"error": {"code": code, "message": "m"}}, status, headers)


@pytest.mark.parametrize("mode", MODES)
def test_refusal_becomes_error_with_code_reason_details_request_id(mode: str) -> None:
    body = {
        "error": {
            "code": "block_not_available",
            "message": "past the covered history",
            "details": {"reason": "block_not_available", "maxAvailableBlock": 25901228},
            "requestId": "req-1",
        }
    }
    tf, _ = make(mode, [json_answer(body, 422)])
    e = raises(mode, tf, lambda t: t.addresses.portfolio(USDC, block=99999999))
    assert e.status == 422
    assert e.code == "block_not_available"
    assert e.reason == "block_not_available"
    assert e.details is not None and e.details["maxAvailableBlock"] == 25901228
    assert e.request_id == "req-1"
    assert e.message == "past the covered history"


@pytest.mark.parametrize("mode", MODES)
def test_body_that_is_not_the_error_envelope_is_unexpected_response(mode: str) -> None:
    tf, _ = make(mode, [lambda r: httpx.Response(502, text="<html>bad gateway</html>", headers={"x-request-id": "r-9"})])
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert (e.code, e.status, e.request_id) == ("unexpected_response", 502, "r-9")


@pytest.mark.parametrize("mode", MODES)
def test_success_that_is_not_json_is_unexpected_response(mode: str) -> None:
    tf, _ = make(mode, [lambda r: httpx.Response(200, text="<html>")])
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert (e.code, e.status) == ("unexpected_response", 200)


@pytest.mark.parametrize("mode", MODES)
def test_key_never_appears_in_an_error(mode: str) -> None:
    def timeout(r: httpx.Request) -> httpx.Response:
        raise httpx.ReadTimeout("slow", request=r)

    for answer in (error("not_found", 404), _boom, timeout):
        tf, _ = make(mode, [answer], max_retries=0)
        e = raises(mode, tf, lambda t: t.blocks.get("1"))
        # what logging.exception prints: the error and its chained cause
        printed = "".join(traceback.format_exception(type(e), e, e.__traceback__))
        for text in (str(e), repr(e), repr(vars(e)), str(e.args), printed):
            assert KEY not in text
    assert KEY not in repr(tf) and KEY not in repr(tf._http)


@pytest.mark.parametrize("mode", MODES)
def test_429_with_retry_after_is_retried_without_it_returned_at_once(mode: str) -> None:
    tf, mock = make(mode, [error("rate_limited", 429, {"retry-after": "0"}), OK])
    assert run(mode, tf, lambda t: t.blocks.latest())["data"] == {"number": 1}
    assert len(mock.calls) == 2

    tf, mock = make(mode, [error("quota_exceeded", 429), OK])
    assert raises(mode, tf, lambda t: t.blocks.latest()).code == "quota_exceeded"
    assert len(mock.calls) == 1


@pytest.mark.parametrize("mode", MODES)
def test_retry_after_is_waited_and_capped_at_60(mode: str) -> None:
    tf, _ = make(mode, [error("rate_limited", 429, {"retry-after": "7"}), error("rate_limited", 429, {"retry-after": "3600"}), OK])
    run(mode, tf, lambda t: t.blocks.latest())
    assert tf.slept == [7.0, 60.0]

    later = format_datetime(datetime.now(timezone.utc) + timedelta(seconds=30), usegmt=True)
    tf, _ = make(mode, [error("service_unavailable", 503, {"retry-after": later}), OK])
    run(mode, tf, lambda t: t.blocks.latest())
    assert 28 <= tf.slept[0] <= 31


@pytest.mark.parametrize("mode", MODES)
def test_503_is_retried_400_404_422_500_are_not(mode: str) -> None:
    tf, mock = make(mode, [error("service_unavailable", 503, {"retry-after": "0"}), OK])
    run(mode, tf, lambda t: t.blocks.latest())
    assert len(mock.calls) == 2
    for status in (400, 401, 402, 403, 404, 422, 500):
        tf, mock = make(mode, [error("x", status), OK])
        assert raises(mode, tf, lambda t: t.blocks.latest()).status == status
        assert len(mock.calls) == 1, f"status {status}"


@pytest.mark.parametrize("mode", MODES)
def test_503_without_retry_after_backs_off_exponentially(mode: str) -> None:
    busy = error("service_unavailable", 503)
    tf, mock = make(mode, [busy, busy, OK])
    run(mode, tf, lambda t: t.blocks.latest())
    assert len(mock.calls) == 3
    assert 0.25 <= tf.slept[0] <= 0.5 and 0.5 <= tf.slept[1] <= 1.0


@pytest.mark.parametrize("mode", MODES)
def test_retries_stop_at_max_retries(mode: str) -> None:
    busy = error("service_unavailable", 503, {"retry-after": "0"})
    tf, mock = make(mode, [busy, busy, busy, busy], max_retries=2)
    assert raises(mode, tf, lambda t: t.blocks.latest()).status == 503
    assert len(mock.calls) == 3


def _boom(request: httpx.Request) -> httpx.Response:
    raise httpx.ConnectError("fetch failed", request=request)


@pytest.mark.parametrize("mode", MODES)
def test_network_failure_becomes_connection_error_after_the_retries(mode: str) -> None:
    tf, mock = make(mode, [_boom, _boom], max_retries=1)
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert (e.code, e.status) == ("connection_error", 0)
    assert isinstance(e.__cause__, httpx.ConnectError)
    assert len(mock.calls) == 2 and len(tf.slept) == 1

    tf, mock = make(mode, [_boom, OK], max_retries=1)
    assert run(mode, tf, lambda t: t.blocks.latest())["data"] == {"number": 1}


@pytest.mark.parametrize("mode", MODES)
def test_timeout_is_timeout_with_status_0_and_not_retried(mode: str) -> None:
    def slow(request: httpx.Request) -> httpx.Response:
        raise httpx.ReadTimeout("timed out", request=request)

    tf, mock = make(mode, [slow, OK])
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert (e.code, e.status) == ("timeout", 0)
    assert len(mock.calls) == 1


@pytest.mark.parametrize("mode", MODES)
def test_pages_follow_next_cursor_with_the_same_parameters_and_stop_when_absent(mode: str) -> None:
    p1 = json_answer({"data": {"transactions": [1, 2], "nextCursor": "c1"}, "meta": {}})
    p2 = json_answer({"data": {"transactions": [3]}, "meta": {}})
    tf, mock = make(mode, [p1, p2])
    pages = collect(mode, tf, lambda t: t.addresses.transactions_pages(USDC, limit=2, direction="in"))
    assert [p["data"]["transactions"] for p in pages] == [[1, 2], [3]]
    first, second = (parse_qs(c.url.query.decode()) for c in mock.calls)
    assert first == {"limit": ["2"], "direction": ["in"]}
    assert second == {"limit": ["2"], "direction": ["in"], "cursor": ["c1"]}


@pytest.mark.parametrize("mode", MODES)
def test_a_short_page_with_a_next_cursor_is_not_the_last_one(mode: str) -> None:
    p1 = json_answer({"data": {"transactions": [1], "nextCursor": "c1"}, "meta": {}})
    p2 = json_answer({"data": {"transactions": []}, "meta": {}})
    tf, _ = make(mode, [p1, p2])
    assert len(collect(mode, tf, lambda t: t.addresses.transactions_pages(USDC, limit=50))) == 2


@pytest.mark.parametrize("mode", MODES)
def test_a_null_next_cursor_ends_the_pages(mode: str) -> None:
    tf, mock = make(mode, [json_answer({"data": {"transactions": [], "nextCursor": None}, "meta": {}})])
    assert len(collect(mode, tf, lambda t: t.addresses.transactions_pages(USDC))) == 1
    assert len(mock.calls) == 1


@pytest.mark.parametrize("mode", MODES)
def test_chain_id_comes_from_the_client(mode: str) -> None:
    for chain in (1, 10):
        tf, mock = make(mode, [OK], chain_id=chain)
        run(mode, tf, lambda t: t.blocks.latest())
        assert mock.calls[0].url.path == f"/api/v1/chains/{chain}/blocks/latest"


@pytest.mark.parametrize("mode", MODES)
def test_unset_parameters_are_not_sent_and_values_go_on_the_wire_as_given(mode: str) -> None:
    ok = json_answer({"data": {}, "meta": {}})
    tf, mock = make(mode, [ok, ok, ok])
    run(mode, tf, lambda t: t.tokens.price_history(USDC, from_=1700000000, to=None))
    run(mode, tf, lambda t: t.contracts.decompilation(USDC, bodies=True, cursor=None))
    run(mode, tf, lambda t: t.contracts.decompilation(USDC, bodies=False))
    queries = [c.url.query.decode() for c in mock.calls]
    assert queries == ["from=1700000000", "bodies=true", "bodies=false"]


@pytest.mark.parametrize("mode", MODES)
def test_path_values_are_percent_encoded(mode: str) -> None:
    tf, mock = make(mode, [json_answer({"data": {}, "meta": {}})])
    run(mode, tf, lambda t: t.addresses.attribute_proof(USDC, "a:b/c d"))
    assert mock.calls[0].url.raw_path.decode().endswith("/attributes/a%3Ab%2Fc%20d/proof")


@pytest.mark.parametrize("mode", MODES)
def test_headers_user_agent_suffix_and_base_url(mode: str) -> None:
    tf, mock = make(mode, [OK], user_agent="my-app/1.2", base_url="https://tracefast.xyz/api/v1/")
    run(mode, tf, lambda t: t.blocks.latest())
    req = mock.calls[0]
    assert str(req.url) == "https://tracefast.xyz/api/v1/chains/1/blocks/latest"
    assert req.headers["user-agent"].startswith("tracefast-python/0.") and req.headers["user-agent"].endswith(" my-app/1.2")
    assert req.headers["accept"] == "application/json"


def test_api_key_is_required() -> None:
    with pytest.raises(ValueError):
        TraceFast(api_key="")
    with pytest.raises(ValueError):
        AsyncTraceFast(api_key="")


def test_error_pickles_and_reads_well() -> None:
    e = TraceFastError(status=404, code="not_found", message="no such block", details={"reason": "r"}, request_id="q")
    back = pickle.loads(pickle.dumps(e))
    assert (back.status, back.code, back.message, back.reason, back.request_id) == (404, "not_found", "no such block", "r", "q")
    assert str(e) == "not_found: no such block (HTTP 404, request q)"


def test_context_managers_close_only_their_own_client() -> None:
    with TraceFast(api_key=KEY) as tf:
        own = tf._http.client
    assert own.is_closed
    given = httpx.Client()
    with TraceFast(api_key=KEY, http_client=given):
        pass
    assert not given.is_closed
    given.close()

    async def go() -> None:
        async with AsyncTraceFast(api_key=KEY) as atf:
            aown = atf._http.client
        assert aown.is_closed
        agiven = httpx.AsyncClient()
        async with AsyncTraceFast(api_key=KEY, http_client=agiven):
            pass
        assert not agiven.is_closed
        await agiven.aclose()

    asyncio.run(go())
