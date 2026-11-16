"""The one error type of the SDK."""

from __future__ import annotations

from typing import Any


class TraceFastError(Exception):
    """Every failure of a TraceFast call.

    A refusal from the API, a response that is not the API's, a network failure
    or a timeout. Branch on ``code`` and ``reason`` (``details["reason"]``) —
    never on ``message``, which is for humans and may be reworded.

    Attributes:
        status: HTTP status of the answer; ``0`` when no answer arrived.
        code: The stable word from the API's error registry (``bad_request``,
            ``not_found``, ``block_not_available``, ``rate_limited``, …), or one
            of the SDK's own: ``unexpected_response``, ``connection_error``,
            ``timeout``.
        message: For humans. Never parse it.
        details: What to act on, when the API states it — ``details["reason"]``
            is the stable sub-code.
        request_id: The request's id. Quote it when asking for support.
        retry_after: Seconds the API asked to wait, from ``Retry-After``.
    """

    status: int
    code: str
    message: str
    details: dict[str, Any] | None
    request_id: str | None
    retry_after: float | None

    def __init__(
        self,
        *,
        status: int,
        code: str,
        message: str,
        details: dict[str, Any] | None = None,
        request_id: str | None = None,
        retry_after: float | None = None,
    ) -> None:
        super().__init__(message)
        self.status = status
        self.code = code
        self.message = message
        self.details = details
        self.request_id = request_id
        self.retry_after = retry_after

    @property
    def reason(self) -> str | None:
        """``details["reason"]``, when the API gave one."""
        r = (self.details or {}).get("reason")
        return r if isinstance(r, str) else None

    def __str__(self) -> str:
        where = f"HTTP {self.status}" if self.status else "no answer"
        rid = f", request {self.request_id}" if self.request_id else ""
        return f"{self.code}: {self.message} ({where}{rid})"

    def __repr__(self) -> str:
        return (
            f"TraceFastError(status={self.status!r}, code={self.code!r}, "
            f"message={self.message!r}, request_id={self.request_id!r})"
        )

    def __reduce__(self) -> tuple[Any, ...]:
        # Keyword-only __init__: rebuild from the fields, so the error pickles.
        return (_rebuild, (self.__dict__.copy(),))


def _rebuild(state: dict[str, Any]) -> TraceFastError:
    e = TraceFastError(
        status=state["status"], code=state["code"], message=state["message"]
    )
    e.__dict__.update(state)
    return e
