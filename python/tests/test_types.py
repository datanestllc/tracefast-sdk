"""The response types resolve at runtime, as documentation tools and validators read them."""

from __future__ import annotations

import typing

import pytest

from tracefast import types


@pytest.mark.parametrize("name", types.__all__)
def test_every_type_resolves_with_get_type_hints(name: str) -> None:
    obj = getattr(types, name)
    if isinstance(obj, type):
        hints = typing.get_type_hints(obj, include_extras=True)
        assert hints, f"{name} has no fields"
    else:
        # An alias of an open enum: a real typing object, not a string.
        assert not isinstance(obj, str), f"{name} is a quoted alias"
        assert typing.get_args(obj), name


def test_a_field_named_like_a_keyword_keeps_the_class_and_its_docstrings() -> None:
    t = types.Transaction
    assert isinstance(t, type) and t.__doc__ and "`from`" in t.__doc__
    hints = typing.get_type_hints(t, include_extras=True)
    assert "from" in hints and "to" in hints
    assert "from" in getattr(t, "__required_keys__")
    assert "from" in getattr(types.BlockTokenTransfer, "__optional_keys__")
    assert not any(n.startswith("_") for n in types.__all__)
