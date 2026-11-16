"""The official Python SDK for the TraceFast API.

Ethereum blocks, transactions, traces, tokens, contracts and what TraceFast
proves about an address. Every method returns the whole ``{data, meta}``
envelope as a dict typed by the TypedDicts in ``tracefast.types``.

    from tracefast import TraceFast

    with TraceFast(api_key="tf_live_…") as tf:
        latest = tf.blocks.latest()
"""

from . import types as types
from ._client import AsyncTraceFast, TraceFast
from ._errors import TraceFastError
from ._generated import CATALOGUE_VERSION
from ._version import VERSION

__version__ = VERSION

__all__ = [
    "CATALOGUE_VERSION",
    "VERSION",
    "AsyncTraceFast",
    "TraceFast",
    "TraceFastError",
    "__version__",
    "types",
]
