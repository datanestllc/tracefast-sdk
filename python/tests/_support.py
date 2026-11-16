"""Run the same test against the sync and the async client."""

from __future__ import annotations

import asyncio
from collections.abc import Callable
from typing import Any

import httpx
from conftest import KEY, Answer, Mock

from tracefast import AsyncTraceFast, TraceFast

MODES = ["sync", "async"]


def make(mode: str, answers: list[Answer], **options: Any) -> tuple[Any, Mock]:
    """A client of the given mode over a mock transport, with sleeps recorded instead of slept."""
    mock = Mock(answers)
    options.setdefault("api_key", KEY)
    slept: list[float] = []
    if mode == "sync":
        tf: Any = TraceFast(http_client=httpx.Client(transport=mock.transport()), **options)
        tf._http.sleep = slept.append
    else:
        tf = AsyncTraceFast(http_client=httpx.AsyncClient(transport=mock.transport()), **options)

        async def sleep(s: float) -> None:
            slept.append(s)

        tf._http.sleep = sleep
    tf.slept = slept
    return tf, mock


def run(mode: str, tf: Any, fn: Callable[[Any], Any]) -> Any:
    """``fn(tf)`` — awaited when the client is async."""
    if mode == "sync":
        return fn(tf)
    return asyncio.run(fn(tf))


def collect(mode: str, tf: Any, fn: Callable[[Any], Any]) -> list[Any]:
    """Every page a pager yields."""
    if mode == "sync":
        return list(fn(tf))

    async def go() -> list[Any]:
        return [page async for page in fn(tf)]

    return asyncio.run(go())
