"""The behaviour every SDK shares (SPEC.md): the client, errors, retries, pagination."""

from __future__ import annotations

import asyncio
import logging
import pickle
import traceback
from collections.abc import AsyncIterator, Iterator
from datetime import datetime, timedelta, timezone
from email.utils import format_datetime
from typing import Any
from urllib.parse import parse_qs

import httpx
import pytest
from _support import MODES, collect, make, run
from conftest import KEY, Mock, json_answer

import tracefast
from tracefast import (
    APIConnectionError,
    APITimeoutError,
    AsyncTraceFast,
    AuthenticationError,
    BadRequestError,
    InternalServerError,
    NotFoundError,
    PermissionDeniedError,
    RateLimitError,
    TraceFast,
    TraceFastError,
    UnexpectedResponseError,
    UnprocessableEntityError,
)

USDC = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
OK = json_answer({"data": {"number": 1}, "meta": {}})


def raises(mode: str, tf: Any, fn: Any) -> TraceFastError:
    with pytest.raises(TraceFastError) as info:
        run(mode, tf, fn)
    return info.value


def error(code: str, status: int, headers: dict[str, str] | None = None, **fields: Any) -> Any:
    return json_answer({"error": {"code": code, "message": "m", **fields}}, status, headers)


def printed(e: BaseException) -> str:
    """What `logging.exception` prints: the error and every chained cause."""
    return "".join(traceback.format_exception(type(e), e, e.__traceback__))


def page(rows: list[Any], cursor: str | None = None, field: str = "transactions") -> Any:
    data: dict[str, Any] = {field: rows}
    if cursor is not None:
        data["nextCursor"] = cursor
    return json_answer({"data": data, "meta": {}})


def queries(mock: Mock) -> list[dict[str, list[str]]]:
    return [parse_qs(c.url.query.decode()) for c in mock.calls]


# ---- errors


@pytest.mark.parametrize("mode", MODES)
def test_refusal_becomes_error_with_every_field(mode: str) -> None:
    body = {
        "error": {
            "code": "block_not_available",
            "message": "past the covered history",
            "details": {"reason": "block_not_available", "maxAvailableBlock": 25901228},
            "requestId": "req-1",
            "hint": "a field the SDK does not know",
        }
    }
    tf, _ = make(mode, [json_answer(body, 422)])
    e = raises(mode, tf, lambda t: t.addresses.portfolio(USDC, block=99999999))
    assert isinstance(e, UnprocessableEntityError)
    assert (e.status, e.code, e.reason, e.request_id) == (422, "block_not_available", "block_not_available", "req-1")
    assert e.details is not None and e.details["maxAvailableBlock"] == 25901228
    assert e.body == body["error"], "the raw error object survives"
    assert e.message == "422 block_not_available: past the covered history"
    assert str(e) == "422 block_not_available: past the covered history (request req-1)"


@pytest.mark.parametrize("mode", MODES)
def test_reason_and_bucket_come_from_the_gateways_top_level_fields(mode: str) -> None:
    auth = {"error": {"code": "auth_failed", "message": "An API key is required.", "reason": "api_key_required"}}
    tf, _ = make(mode, [json_answer(auth, 401)])
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert isinstance(e, AuthenticationError)
    assert (e.code, e.reason, e.bucket) == ("auth_failed", "api_key_required", None)

    limited = {"error": {"code": "rate_limited", "message": "Too many requests.", "bucket": "base"}}
    tf, _ = make(mode, [json_answer(limited, 429, {"retry-after": "1"})], max_retries=0)
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert isinstance(e, RateLimitError)
    assert (e.code, e.bucket, e.retry_after, e.reason) == ("rate_limited", "base", 1.0, None)

    plan = {
        "error": {
            "code": "forbidden",
            "message": "This method needs a higher plan.",
            "reason": "plan_scope_missing",
            "details": {"minimumPlan": "investigation", "currentPlan": "data"},
        }
    }
    tf, _ = make(mode, [json_answer(plan, 403)])
    e = raises(mode, tf, lambda t: t.addresses.transactions(USDC))
    assert isinstance(e, PermissionDeniedError)
    assert e.reason == "plan_scope_missing" and e.details == {"minimumPlan": "investigation", "currentPlan": "data"}


@pytest.mark.parametrize("mode", MODES)
def test_the_subclass_follows_the_status(mode: str) -> None:
    cases = {
        400: BadRequestError,
        401: AuthenticationError,
        403: PermissionDeniedError,
        404: NotFoundError,
        422: UnprocessableEntityError,
        429: RateLimitError,
        500: InternalServerError,
        502: InternalServerError,
        402: TraceFastError,
    }
    for status, cls in cases.items():
        tf, _ = make(mode, [error("x", status)], max_retries=0)
        e = raises(mode, tf, lambda t: t.blocks.latest())
        assert type(e) is cls, status


@pytest.mark.parametrize("mode", MODES)
def test_body_that_is_not_the_error_envelope_is_unexpected_response(mode: str) -> None:
    tf, _ = make(mode, [lambda r: httpx.Response(502, text="<html>bad gateway</html>", headers={"x-request-id": "r-9"})])
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert isinstance(e, UnexpectedResponseError)
    assert (e.code, e.status, e.request_id) == ("unexpected_response", 502, "r-9")


@pytest.mark.parametrize("mode", MODES)
def test_success_that_is_not_json_or_not_the_envelope_is_unexpected_response(mode: str) -> None:
    for answer in (lambda r: httpx.Response(200, text="<html>"), json_answer([1, 2]), json_answer({"data": {}})):
        tf, _ = make(mode, [answer])
        e = raises(mode, tf, lambda t: t.blocks.latest())
        assert isinstance(e, UnexpectedResponseError)
        assert (e.code, e.status) == ("unexpected_response", 200)


@pytest.mark.parametrize("mode", MODES)
def test_a_body_that_does_not_decode_is_unexpected_response(mode: str) -> None:
    def bad(r: httpx.Request) -> httpx.Response:
        # A stream, not content=: httpx would decode bytes given at construction.
        return httpx.Response(200, stream=httpx.ByteStream(b"not gzip"), headers={"content-encoding": "gzip"})

    tf, mock = make(mode, [bad, OK])
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert isinstance(e, UnexpectedResponseError)
    assert (e.code, e.status) == ("unexpected_response", 200)
    assert isinstance(e.__cause__, httpx.DecodingError)
    assert len(mock.calls) == 1, "not retried"


@pytest.mark.parametrize("mode", MODES)
def test_key_never_appears_in_an_error(mode: str) -> None:
    def timeout(r: httpx.Request) -> httpx.Response:
        raise httpx.ReadTimeout("slow", request=r)

    for answer in (error("not_found", 404), _boom, timeout):
        tf, _ = make(mode, [answer], max_retries=0)
        e = raises(mode, tf, lambda t: t.blocks.get(1))
        cause = ""
        if e.__cause__ is not None:
            request = vars(e.__cause__).get("_request")  # httpx keeps the request it failed on
            cause = repr(vars(e.__cause__)) + (repr(dict(request.headers)) if request is not None else "")
        for text in (str(e), repr(e), repr(vars(e)), str(e.args), printed(e), cause):
            assert KEY not in text
    assert KEY not in repr(tf) and KEY not in repr(tf._http) and KEY not in repr(tf._http.settings)


@pytest.mark.parametrize("mode", MODES)
def test_a_local_protocol_error_is_not_retried_and_carries_nothing_of_the_request(mode: str) -> None:
    def local(r: httpx.Request) -> httpx.Response:
        raise httpx.LocalProtocolError(f"Illegal header value {KEY!r}")

    tf, mock = make(mode, [local, OK])
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert isinstance(e, APIConnectionError) and e.status == 0
    assert e.__cause__ is None and e.__context__ is None
    assert KEY not in printed(e)
    assert len(mock.calls) == 1


def test_error_pickles_as_its_subclass_and_reads_well() -> None:
    e = NotFoundError(status=404, code="not_found", message="no such block", reason="r", request_id="q", body={"code": "not_found"})
    back = pickle.loads(pickle.dumps(e))
    assert type(back) is NotFoundError
    assert (back.status, back.code, back.message, back.reason, back.request_id, back.body) == (
        404,
        "not_found",
        "404 not_found: no such block",
        "r",
        "q",
        {"code": "not_found"},
    )
    assert str(back) == "404 not_found: no such block (request q)"
    assert str(APITimeoutError(status=0, code="timeout", message="no answer")) == "timeout: no answer"


def test_every_subclass_is_a_tracefast_error() -> None:
    for name in tracefast.__all__:
        obj = getattr(tracefast, name)
        if isinstance(obj, type) and name.endswith("Error"):
            assert issubclass(obj, TraceFastError), name


# ---- redirects


@pytest.mark.parametrize("mode", MODES)
def test_a_redirect_is_never_followed_even_by_a_client_that_follows_them(mode: str) -> None:
    mock = Mock(
        [
            lambda r: httpx.Response(302, headers={"location": "https://elsewhere.example/steal"}),
            json_answer({"data": {}, "meta": {}}),
        ]
    )
    if mode == "sync":
        tf: Any = TraceFast(KEY, http_client=httpx.Client(transport=mock.transport(), follow_redirects=True))
    else:
        tf = AsyncTraceFast(KEY, http_client=httpx.AsyncClient(transport=mock.transport(), follow_redirects=True))
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert isinstance(e, UnexpectedResponseError) and e.status == 302
    assert [c.url.host for c in mock.calls] == ["tracefast.xyz"], "the key never travels to another host"


@pytest.mark.parametrize("mode", MODES)
def test_any_3xx_is_unexpected_response_even_with_an_error_body(mode: str) -> None:
    for status in (301, 304, 307, 308):
        tf, mock = make(mode, [error("not_found", status, {"location": "/elsewhere"})])
        e = raises(mode, tf, lambda t: t.blocks.latest())
        assert isinstance(e, UnexpectedResponseError)
        assert (e.code, e.status) == ("unexpected_response", status)
        assert len(mock.calls) == 1


# ---- retries


@pytest.mark.parametrize("mode", MODES)
def test_429_with_a_short_retry_after_is_retried(mode: str) -> None:
    tf, mock = make(mode, [error("rate_limited", 429, {"retry-after": "7"}, bucket="base"), OK])
    assert run(mode, tf, lambda t: t.blocks.latest())["data"] == {"number": 1}
    assert len(mock.calls) == 2 and tf.slept == [7.0]


@pytest.mark.parametrize("mode", MODES)
def test_429_with_a_long_or_no_retry_after_is_raised_at_once_with_the_real_wait(mode: str) -> None:
    tf, mock = make(mode, [error("rate_limited", 429, {"retry-after": "3600"}, bucket="quota_daily"), OK])
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert (e.retry_after, e.bucket) == (3600.0, "quota_daily"), "retry_after is exactly as sent"
    assert len(mock.calls) == 1 and tf.slept == []

    tf, mock = make(mode, [error("rate_limited", 429, {"retry-after": "61"}), OK], max_retry_wait=60)
    assert raises(mode, tf, lambda t: t.blocks.latest()).retry_after == 61.0
    assert len(mock.calls) == 1

    tf, mock = make(mode, [error("concurrency_limited", 429, bucket="concurrency"), OK])
    assert raises(mode, tf, lambda t: t.blocks.latest()).retry_after is None
    assert len(mock.calls) == 1

    tf, mock = make(mode, [error("rate_limited", 429, {"retry-after": "120"}), OK], max_retry_wait=300)
    run(mode, tf, lambda t: t.blocks.latest())
    assert tf.slept == [120.0], "max_retry_wait raises the bar"


@pytest.mark.parametrize("mode", MODES)
def test_quota_exceeded_is_never_retried(mode: str) -> None:
    for status in (429, 503):
        tf, mock = make(mode, [error("quota_exceeded", status, {"retry-after": "1"}), OK])
        e = raises(mode, tf, lambda t: t.blocks.latest())
        assert (e.code, e.retry_after) == ("quota_exceeded", 1.0)
        assert len(mock.calls) == 1


@pytest.mark.parametrize("mode", MODES)
def test_retry_after_as_an_http_date(mode: str) -> None:
    later = format_datetime(datetime.now(timezone.utc) + timedelta(seconds=30), usegmt=True)
    tf, _ = make(mode, [error("service_unavailable", 503, {"retry-after": later}), OK])
    run(mode, tf, lambda t: t.blocks.latest())
    assert 28 <= tf.slept[0] <= 31


@pytest.mark.parametrize("mode", MODES)
def test_503_waits_retry_after_at_most_max_retry_wait(mode: str) -> None:
    tf, _ = make(mode, [error("service_unavailable", 503, {"retry-after": "500"}), OK], max_retry_wait=10)
    run(mode, tf, lambda t: t.blocks.latest())
    assert tf.slept == [10.0]


@pytest.mark.parametrize("mode", MODES)
def test_503_is_retried_other_statuses_are_not(mode: str) -> None:
    tf, mock = make(mode, [error("service_unavailable", 503, {"retry-after": "0"}), OK])
    run(mode, tf, lambda t: t.blocks.latest())
    assert len(mock.calls) == 2
    for status in (400, 401, 402, 403, 404, 422, 500, 502):
        tf, mock = make(mode, [error("x", status), OK])
        assert raises(mode, tf, lambda t: t.blocks.latest()).status == status
        assert len(mock.calls) == 1, f"status {status}"


@pytest.mark.parametrize("mode", MODES)
def test_backoff_is_exponential_with_jitter_and_each_wait_at_most_8_s(mode: str) -> None:
    busy = error("service_unavailable", 503)
    tf, mock = make(mode, [busy, busy, OK])
    run(mode, tf, lambda t: t.blocks.latest())
    assert len(mock.calls) == 3
    assert 0.25 <= tf.slept[0] <= 0.5 and 0.5 <= tf.slept[1] <= 1.0

    tf, mock = make(mode, [_boom] * 8 + [OK], max_retries=8)
    run(mode, tf, lambda t: t.blocks.latest())
    assert len(tf.slept) == 8 and max(tf.slept) <= 8.0 and tf.slept[-1] >= 4.0


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
    assert isinstance(e, APIConnectionError)
    assert (e.code, e.status) == ("connection_error", 0)
    assert isinstance(e.__cause__, httpx.ConnectError)
    assert len(mock.calls) == 2 and len(tf.slept) == 1

    tf, mock = make(mode, [_boom, OK], max_retries=1)
    assert run(mode, tf, lambda t: t.blocks.latest())["data"] == {"number": 1}


class _BrokenBody(httpx.SyncByteStream, httpx.AsyncByteStream):
    """A body that breaks after the status line arrived."""

    def __iter__(self) -> Iterator[bytes]:
        yield b'{"data":'
        raise httpx.ReadError("connection reset")

    async def __aiter__(self) -> AsyncIterator[bytes]:
        yield b'{"data":'
        raise httpx.ReadError("connection reset")


@pytest.mark.parametrize("mode", MODES)
def test_a_failure_while_reading_the_body_is_a_connection_error_and_retried(mode: str) -> None:
    broken = lambda r: httpx.Response(200, stream=_BrokenBody())  # noqa: E731
    tf, mock = make(mode, [broken, OK], max_retries=1)
    assert run(mode, tf, lambda t: t.blocks.latest())["data"] == {"number": 1}
    assert len(mock.calls) == 2

    tf, mock = make(mode, [broken], max_retries=0)
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert isinstance(e, APIConnectionError) and e.status == 200, "the status line had arrived"
    assert isinstance(e.__cause__, httpx.ReadError)


@pytest.mark.parametrize("mode", MODES)
def test_timeout_is_timeout_with_status_0_and_not_retried(mode: str) -> None:
    def slow(request: httpx.Request) -> httpx.Response:
        raise httpx.ReadTimeout("timed out", request=request)

    tf, mock = make(mode, [slow, OK])
    e = raises(mode, tf, lambda t: t.blocks.latest())
    assert isinstance(e, APITimeoutError)
    assert (e.code, e.status) == ("timeout", 0)
    assert len(mock.calls) == 1


@pytest.mark.parametrize("mode", MODES)
def test_retry_decisions_are_logged_without_the_key(mode: str, caplog: pytest.LogCaptureFixture) -> None:
    caplog.set_level(logging.DEBUG, logger="tracefast")
    tf, _ = make(mode, [json_answer({"error": {"code": "rate_limited", "message": "m", "requestId": "rq-7"}}, 429, {"retry-after": "2"}), OK])
    run(mode, tf, lambda t: t.blocks.latest())
    assert any("retrying" in r.getMessage() and "rq-7" in r.getMessage() for r in caplog.records)
    assert all(r.name == "tracefast" for r in caplog.records)
    assert KEY not in caplog.text


def test_cancellation_propagates_and_is_not_wrapped() -> None:
    async def hang(request: httpx.Request) -> httpx.Response:
        await asyncio.sleep(3600)
        raise AssertionError("unreachable")

    async def go() -> None:
        async with AsyncTraceFast(KEY, http_client=httpx.AsyncClient(transport=httpx.MockTransport(hang))) as tf:
            task = asyncio.create_task(tf.blocks.latest())
            await asyncio.sleep(0.01)
            task.cancel()
            with pytest.raises(asyncio.CancelledError):
                await task

    asyncio.run(go())


# ---- pagination


@pytest.mark.parametrize("mode", MODES)
def test_pages_follow_next_cursor_with_the_same_parameters_and_stop_when_absent(mode: str) -> None:
    tf, mock = make(mode, [page([1, 2], "c1"), page([3])])
    pages = collect(mode, tf, lambda t: t.addresses.transactions_pages(USDC, limit=2, direction="sent"))
    assert [p["data"]["transactions"] for p in pages] == [[1, 2], [3]]
    assert queries(mock) == [
        {"limit": ["2"], "direction": ["sent"]},
        {"limit": ["2"], "direction": ["sent"], "cursor": ["c1"]},
    ]


@pytest.mark.parametrize("mode", MODES)
def test_a_cursor_given_to_a_walker_is_the_first_one_sent(mode: str) -> None:
    tf, mock = make(mode, [page([3], "c2"), page([4])])
    pages = collect(mode, tf, lambda t: t.addresses.transactions_pages(USDC, cursor="c1"))
    assert len(pages) == 2
    assert [q["cursor"] for q in queries(mock)] == [["c1"], ["c2"]]


@pytest.mark.parametrize("mode", MODES)
def test_a_short_page_with_a_next_cursor_is_not_the_last_one(mode: str) -> None:
    tf, _ = make(mode, [page([1], "c1"), page([])])
    assert len(collect(mode, tf, lambda t: t.addresses.transactions_pages(USDC, limit=50))) == 2


@pytest.mark.parametrize("mode", MODES)
def test_a_null_next_cursor_ends_the_pages(mode: str) -> None:
    tf, mock = make(mode, [json_answer({"data": {"transactions": [], "nextCursor": None}, "meta": {}})])
    assert len(collect(mode, tf, lambda t: t.addresses.transactions_pages(USDC))) == 1
    assert len(mock.calls) == 1


@pytest.mark.parametrize("mode", MODES)
def test_an_empty_or_repeated_next_cursor_is_unexpected_response(mode: str) -> None:
    for answers, kwargs in (
        ([page([1], "")], {}),
        ([page([1], "c1"), page([2], "c1")], {}),
        ([page([1], "c0")], {"cursor": "c0"}),
    ):
        tf, mock = make(mode, answers)
        with pytest.raises(UnexpectedResponseError):
            collect(mode, tf, lambda t: t.addresses.transactions_pages(USDC, **kwargs))
        assert len(mock.calls) == len(answers), "no request after the bad cursor"


@pytest.mark.parametrize("mode", MODES)
def test_an_error_mid_walk_surfaces_after_the_pages_before_it(mode: str) -> None:
    bad_cursor = error("invalid_cursor", 400)
    tf, _ = make(mode, [page([1], "c1"), bad_cursor])
    seen: list[Any] = []

    if mode == "sync":
        with pytest.raises(BadRequestError) as info:
            for p in tf.addresses.transactions_pages(USDC):
                seen.append(p)
    else:

        async def go() -> None:
            async for p in tf.addresses.transactions_pages(USDC):
                seen.append(p)

        with pytest.raises(BadRequestError) as info:
            asyncio.run(go())
    assert info.value.code == "invalid_cursor"
    assert len(seen) == 1


@pytest.mark.parametrize("mode", MODES)
def test_items_walk_the_row_array_across_pages(mode: str) -> None:
    tf, mock = make(mode, [page([1, 2], "c1"), page([3])])
    assert collect(mode, tf, lambda t: t.addresses.transactions_items(USDC, limit=2)) == [1, 2, 3]
    assert len(mock.calls) == 2

    tf, _ = make(mode, [page(["a", "b"], "c1", field="transfers"), page(["c"], field="transfers")])
    assert collect(mode, tf, lambda t: t.blocks.token_transfers_items(25920000)) == ["a", "b", "c"]


@pytest.mark.parametrize("mode", MODES)
def test_max_items_and_max_pages_stop_without_fetching_another_page(mode: str) -> None:
    tf, mock = make(mode, [page([1, 2], "c1"), page([3, 4], "c2"), page([5])])
    assert collect(mode, tf, lambda t: t.addresses.transactions_items(USDC, max_items=2)) == [1, 2]
    assert len(mock.calls) == 1

    tf, mock = make(mode, [page([1, 2], "c1"), page([3, 4], "c2"), page([5])])
    assert collect(mode, tf, lambda t: t.addresses.transactions_items(USDC, max_items=3)) == [1, 2, 3]
    assert len(mock.calls) == 2

    tf, mock = make(mode, [page([1, 2], "c1"), page([3, 4], "c2"), page([5])])
    assert len(collect(mode, tf, lambda t: t.addresses.transactions_pages(USDC, max_pages=2))) == 2
    assert len(mock.calls) == 2

    tf, mock = make(mode, [])
    assert collect(mode, tf, lambda t: t.addresses.transactions_items(USDC, max_items=0)) == []
    assert collect(mode, tf, lambda t: t.addresses.transactions_pages(USDC, max_pages=0)) == []
    assert mock.calls == []
    with pytest.raises(ValueError):
        tf.addresses.transactions_items(USDC, max_items=-1)


@pytest.mark.parametrize("mode", MODES)
def test_a_page_without_the_row_array_is_unexpected_response(mode: str) -> None:
    tf, _ = make(mode, [json_answer({"data": {"nextCursor": "c1"}, "meta": {}})])
    with pytest.raises(UnexpectedResponseError):
        collect(mode, tf, lambda t: t.addresses.transactions_items(USDC))


# ---- requests


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
    assert [c.url.query.decode() for c in mock.calls] == ["from=1700000000", "bodies=true", "bodies=false"]


@pytest.mark.parametrize("mode", MODES)
def test_path_values_are_percent_encoded_and_integers_are_integers(mode: str) -> None:
    ok = json_answer({"data": {}, "meta": {}})
    tf, mock = make(mode, [ok, ok])
    run(mode, tf, lambda t: t.addresses.attribute_proof(USDC, "a:b/c d"))
    run(mode, tf, lambda t: t.blocks.get(25920000))
    assert mock.calls[0].url.raw_path.decode().endswith("/attributes/a%3Ab%2Fc%20d/proof")
    assert mock.calls[1].url.raw_path.decode().endswith("/blocks/25920000")


@pytest.mark.parametrize("mode", MODES)
def test_an_empty_or_dot_path_value_is_refused_before_any_request(mode: str) -> None:
    tf, mock = make(mode, [])
    for bad in ("", ".", ".."):
        with pytest.raises(ValueError):
            run(mode, tf, lambda t: t.contracts.proxy(bad))
        with pytest.raises(ValueError):
            tf.addresses.transactions_pages(bad)  # refused at the call, not at the first page
        with pytest.raises(ValueError):
            tf.addresses.transactions_items(bad)
    assert mock.calls == []


@pytest.mark.parametrize("mode", MODES)
def test_headers_user_agent_suffix_and_base_url(mode: str) -> None:
    tf, mock = make(mode, [OK], user_agent="my-app/1.2", base_url="https://tracefast.xyz/api/v1/")
    run(mode, tf, lambda t: t.blocks.latest())
    req = mock.calls[0]
    assert str(req.url) == "https://tracefast.xyz/api/v1/chains/1/blocks/latest"
    assert req.headers["user-agent"] == f"tracefast-python/{tracefast.VERSION} my-app/1.2"
    assert req.headers["accept"] == "application/json"
    assert req.headers["x-api-key"] == KEY


# ---- the client


@pytest.mark.parametrize("cls", [TraceFast, AsyncTraceFast])
def test_the_key_is_checked_at_construction_and_never_repeated(cls: Any, monkeypatch: pytest.MonkeyPatch) -> None:
    def no_pool(*args: Any, **kwargs: Any) -> Any:
        raise AssertionError("a connection pool was created before the key was checked")

    monkeypatch.setattr(httpx, "Client", no_pool)
    monkeypatch.setattr(httpx, "AsyncClient", no_pool)
    for bad in ("", KEY + " ", KEY + "\n", "\t" + KEY, KEY + "’", KEY + " ", KEY + "\x7f"):
        with pytest.raises(ValueError) as info:
            cls(api_key=bad)
        text = printed(info.value) + str(info.value) + repr(info.value)
        assert KEY not in text, repr(bad)


@pytest.mark.parametrize("cls", [TraceFast, AsyncTraceFast])
def test_base_url_must_be_https_except_on_a_loopback_host(cls: Any) -> None:
    for bad in ("http://tracefast.xyz/api/v1", "http://127.evil.example/api/v1", "http://localhost.evil.example/", "ftp://tracefast.xyz", "tracefast.xyz/api/v1", "https://tracefast.xyz/api/v1?x=1"):
        with pytest.raises(ValueError):
            cls(KEY, base_url=bad)
    for good in ("http://localhost/api/v1", "http://127.0.0.1/api/v1", "http://[::1]/api/v1"):
        assert repr(cls(KEY, base_url=good))


@pytest.mark.parametrize("cls", [TraceFast, AsyncTraceFast])
def test_numbers_are_checked(cls: Any) -> None:
    for kwargs in ({"max_retries": -1}, {"timeout": 0}, {"max_retry_wait": -1}, {"chain_id": 0}, {"max_retries": True}):
        with pytest.raises((ValueError, TypeError)):
            cls(KEY, **kwargs)


@pytest.mark.parametrize("mode", MODES)
def test_with_options_changes_a_copy_that_shares_the_connections(mode: str) -> None:
    tf, mock = make(mode, [OK, OK], timeout=30.0)
    other = tf.with_options(timeout=5.0, user_agent="batch/1", max_retries=0)
    assert other._http.client is tf._http.client
    assert (other._http.settings.timeout, other._http.settings.max_retries) == (5.0, 0)
    assert (tf._http.settings.timeout, tf._http.settings.max_retries) == (30.0, 2), "the original is unchanged"
    run(mode, other, lambda t: t.blocks.latest())
    run(mode, tf, lambda t: t.blocks.latest())
    first, second = mock.calls
    assert first.headers["user-agent"].endswith(" batch/1") and not second.headers["user-agent"].endswith(" batch/1")
    assert first.extensions["timeout"]["read"] == 5.0 and second.extensions["timeout"]["read"] == 30.0
    assert first.headers["x-api-key"] == KEY
    with pytest.raises(ValueError):
        tf.with_options(api_key="bad key")


def test_use_after_close_is_a_clear_error_and_a_copy_does_not_close_the_pool() -> None:
    tf = TraceFast(KEY)
    copy = tf.with_options(timeout=1.0)
    copy.close()
    assert not tf._http.client.is_closed
    tf.close()
    for client in (tf, copy):
        with pytest.raises(RuntimeError, match="closed"):
            client.blocks.latest()

    async def go() -> None:
        atf = AsyncTraceFast(KEY)
        await atf.close()
        with pytest.raises(RuntimeError, match="closed"):
            await atf.blocks.latest()

    asyncio.run(go())


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


def test_the_wrong_kind_of_http_client_is_refused() -> None:
    with pytest.raises(TypeError):
        TraceFast(KEY, http_client=httpx.AsyncClient())  # type: ignore[arg-type]
    with pytest.raises(TypeError):
        AsyncTraceFast(KEY, http_client=httpx.Client())  # type: ignore[arg-type]
