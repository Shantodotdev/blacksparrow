"""Reading the JSON document the binary wrote."""

from __future__ import annotations

import json
from typing import Any, cast

from pydantic import ValidationError

from ._errors import BlackSparrowError
from ._models import SCHEMA_VERSION, SiteExtract, is_list, web_url


def parse_extract(payload: Any) -> SiteExtract:
    """Validate a decoded document.

    Raises:
        BlackSparrowError: The document is not an object of a version this package understands,
            or has no valid ``outcome``.
    """
    if not isinstance(payload, dict):
        raise BlackSparrowError("blacksparrow printed a document of an unknown version")
    version = cast("dict[str, Any]", payload).get("schema_version")
    # ``True == 1`` in Python, so the type is checked too.
    if type(version) is not int or version != SCHEMA_VERSION:
        raise BlackSparrowError("blacksparrow printed a document of an unknown version")
    try:
        return SiteExtract.model_validate(payload)
    except ValidationError as error:
        raise BlackSparrowError("blacksparrow printed an unusable document") from error


def parse_document(raw: bytes) -> tuple[SiteExtract, dict[str, Any]]:
    """Decode and validate the bytes the binary wrote.

    NUL characters are removed from every string and key of the returned raw document (they are
    valid JSON, but PostgreSQL cannot store them), so the raw document is safe to store as it is.

    Returns:
        The validated result and the raw document.

    Raises:
        BlackSparrowError: The bytes are not a JSON object, or ``parse_extract`` rejects it.
    """
    try:
        decoded = json.loads(raw, parse_constant=_reject_constant)
    except (ValueError, RecursionError) as error:
        raise BlackSparrowError("blacksparrow printed no JSON") from error
    if not isinstance(decoded, dict):
        raise BlackSparrowError("blacksparrow printed a document of an unknown version")
    payload = _without_nul(cast("dict[str, Any]", decoded))
    return parse_extract(payload), payload


def unsupported_url(url: str) -> tuple[SiteExtract, dict[str, Any]]:
    """The result for an address that is not an http(s) address; the binary is not run."""
    payload: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "outcome": "unsupported_url",
        "outcome_detail": "not an http(s) address",
        "pages": [],
        "page_errors": [],
    }
    start_url = web_url(url)
    if start_url is not None:
        payload["start_url"] = start_url
    return parse_extract(payload), payload


def _reject_constant(name: str) -> Any:
    raise ValueError(f"{name} is not valid JSON")


def _without_nul(payload: dict[str, Any]) -> dict[str, Any]:
    """``payload`` with the NUL characters removed from every string and key, in place.

    Walks the document without recursion, so a deeply nested document cannot overflow the stack.
    """
    containers: list[Any] = [payload]
    while containers:
        node = containers.pop()
        if isinstance(node, dict):
            # Keys are re-inserted in their order, so the document keeps its shape.
            mapping = cast("dict[str, Any]", node)
            for key in list(mapping):
                value = mapping.pop(key)
                mapping[key.replace("\x00", "")] = _cleaned(value, containers)
        elif is_list(node):
            for index, value in enumerate(node):
                node[index] = _cleaned(value, containers)
    return payload


def _cleaned(value: Any, containers: list[Any]) -> Any:
    """A string without NULs, or the container queued for cleaning; other values as they are."""
    if isinstance(value, str):
        return value.replace("\x00", "")
    if isinstance(value, dict | list):
        containers.append(cast("Any", value))
    return cast("Any", value)
