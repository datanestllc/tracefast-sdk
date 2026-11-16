"""The official Python SDK for the TraceFast API.

Ethereum blocks, transactions, traces, tokens, contracts, DEX pools, the
consensus layer, and what TraceFast states about an address. Every method
returns the whole `{data, meta}` envelope as a dict typed by the TypedDicts in
`tracefast.types`.

Example:
    ```python
    import os
    from tracefast import TraceFast

    with TraceFast(api_key=os.environ["TRACEFAST_API_KEY"]) as tf:
        latest = tf.blocks.latest()
        print(latest["data"]["number"])
    ```
"""

from . import types as types
from ._client import AsyncTraceFast, TraceFast
from ._errors import (
    APIConnectionError,
    APITimeoutError,
    AuthenticationError,
    BadRequestError,
    InternalServerError,
    NotFoundError,
    PermissionDeniedError,
    RateLimitError,
    TraceFastError,
    UnexpectedResponseError,
    UnprocessableEntityError,
)
from ._generated import CATALOGUE_VERSION
from ._version import VERSION

__version__ = VERSION

__all__ = [
    "CATALOGUE_VERSION",
    "VERSION",
    "APIConnectionError",
    "APITimeoutError",
    "AsyncTraceFast",
    "AuthenticationError",
    "BadRequestError",
    "InternalServerError",
    "NotFoundError",
    "PermissionDeniedError",
    "RateLimitError",
    "TraceFast",
    "TraceFastError",
    "UnexpectedResponseError",
    "UnprocessableEntityError",
    "__version__",
    "types",
]
