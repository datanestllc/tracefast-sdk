"""The response types of the TraceFast API: one TypedDict per catalogue object.

At runtime every response is a plain `dict`: a key the API leaves out is
absent, a `null` is `None`. Enums are open (`Literal[...] | str`).
"""

from ._generated.types import *  # noqa: F403
from ._generated.types import __all__ as __all__
