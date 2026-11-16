"""The errors of the SDK: `TraceFastError` and one subclass per kind of failure."""

from __future__ import annotations

from typing import Any


class TraceFastError(Exception):
    """Every failure of a TraceFast call.

    A refusal from the API, an answer that is not the API's, a network failure
    or a timeout. Branch on `code`, `reason` and the subclass — never on
    `message`, which is for humans and may be reworded.

    Attributes:
        status: HTTP status of the answer; `0` when no answer arrived.
        code: The stable word to branch on: the API's error registry
            (`bad_request`, `invalid_cursor`, `not_found`, `block_not_available`,
            `quota_exceeded`, …), the gateway's (`auth_failed`, `forbidden`,
            `rate_limited`, `concurrency_limited`), or the SDK's own
            (`unexpected_response`, `connection_error`, `timeout`).
        reason: The stable sub-code: `error.reason` on the gateway's key, plan
            and limit refusals (`api_key_required`, `plan_scope_missing`, …),
            otherwise `details["reason"]`.
        bucket: On a `429`, which limit refused (`base`, `class:…`,
            `concurrency`, `quota_daily`, `quota_method`, …).
        details: What to act on, when the API states it (`maxAvailableBlock`,
            `maxLimit`, `minimumPlan`, `currentPlan`, `upgrade`, …).
        request_id: The request's id. Quote it when asking for support.
        retry_after: Seconds from the `Retry-After` header, exactly as sent.
        body: The raw `error` object of the answer, so a field this SDK does not
            know survives.
        message: `"<status> <code>: <the API's message>"`, for humans. Never
            parse it.
    """

    status: int
    code: str
    message: str
    reason: str | None
    bucket: str | None
    details: dict[str, Any] | None
    request_id: str | None
    retry_after: float | None
    body: dict[str, Any] | None

    def __init__(
        self,
        *,
        status: int,
        code: str,
        message: str,
        reason: str | None = None,
        bucket: str | None = None,
        details: dict[str, Any] | None = None,
        request_id: str | None = None,
        retry_after: float | None = None,
        body: dict[str, Any] | None = None,
    ) -> None:
        text = f"{status} {code}: {message}" if status else f"{code}: {message}"
        super().__init__(text)
        self.status = status
        self.code = code
        self.message = text
        self.reason = reason
        self.bucket = bucket
        self.details = details
        self.request_id = request_id
        self.retry_after = retry_after
        self.body = body

    def __str__(self) -> str:
        return self.message + (f" (request {self.request_id})" if self.request_id else "")

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}(status={self.status!r}, code={self.code!r}, "
            f"reason={self.reason!r}, request_id={self.request_id!r})"
        )

    def __reduce__(self) -> tuple[Any, ...]:
        # Keyword-only __init__: rebuild from the fields, so the error pickles.
        return (_rebuild, (type(self), self.__dict__.copy()))


def _rebuild(cls: type[TraceFastError], state: dict[str, Any]) -> TraceFastError:
    e = cls.__new__(cls)
    Exception.__init__(e, state.get("message", ""))
    e.__dict__.update(state)
    return e


class BadRequestError(TraceFastError):
    """`400`: the request is outside the contract; `reason` says which rule."""


class AuthenticationError(TraceFastError):
    """`401`: no key, or a key that is not valid."""


class PermissionDeniedError(TraceFastError):
    """`403`: the plan does not include the method (`plan_scope_missing`), or the key may not be used."""


class NotFoundError(TraceFastError):
    """`404`: nothing at the key the path names, or no such method."""


class UnprocessableEntityError(TraceFastError):
    """`422`: well-formed, but the data cannot answer it (`block_not_available`, `coverage_gap`, …)."""


class RateLimitError(TraceFastError):
    """`429`: a rate limit, a concurrency cap or a quota; `bucket` says which."""


class InternalServerError(TraceFastError):
    """`5xx`: a failure on the API's side."""


class APIConnectionError(TraceFastError):
    """`connection_error`: the connection failed or broke before the whole answer arrived."""


class APITimeoutError(TraceFastError):
    """`timeout`: no answer within the timeout."""


class UnexpectedResponseError(TraceFastError):
    """`unexpected_response`: an answer that is not the API's (a redirect, a proxy page, not JSON)."""


_BY_STATUS: dict[int, type[TraceFastError]] = {
    400: BadRequestError,
    401: AuthenticationError,
    403: PermissionDeniedError,
    404: NotFoundError,
    422: UnprocessableEntityError,
    429: RateLimitError,
}


def error_class_for(status: int, code: str) -> type[TraceFastError]:
    """The subclass for the SDK's own code, else for the answer's status."""
    if code == "unexpected_response":
        return UnexpectedResponseError
    if code == "connection_error":
        return APIConnectionError
    if code == "timeout":
        return APITimeoutError
    if status in _BY_STATUS:
        return _BY_STATUS[status]
    if status >= 500:
        return InternalServerError
    return TraceFastError


def make_error(*, status: int, code: str, message: str, **fields: Any) -> TraceFastError:
    """Build the error of the right subclass."""
    return error_class_for(status, code)(status=status, code=code, message=message, **fields)
