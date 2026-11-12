"""The models match the JSON Schema the binary publishes (``blacksparrow schema extract``)."""

from __future__ import annotations

import json
import subprocess
import typing
from pathlib import Path
from typing import Any

import pytest
from pydantic import AliasChoices, BaseModel
from pydantic.fields import FieldInfo

import blacksparrow
from blacksparrow import _document

SCHEMA_PATH = Path(__file__).resolve().parents[3] / "schema" / "extract.schema.json"


def wire_names(model: type[BaseModel]) -> set[str]:
    """The names the document uses for the model's fields."""

    def wire(name: str, field: FieldInfo) -> str:
        alias = field.validation_alias
        if isinstance(alias, AliasChoices):
            return str(alias.choices[0])
        return alias if isinstance(alias, str) else name

    return {wire(name, field) for name, field in model.model_fields.items()}


def resolve(schema: dict[str, Any], root: dict[str, Any]) -> dict[str, Any]:
    """The object schema behind ``$ref``, nullable ``anyOf`` and array ``items``."""
    while True:
        if "$ref" in schema:
            node: Any = root
            for part in schema["$ref"].removeprefix("#/").split("/"):
                node = node[part]
            schema = node
        elif "items" in schema:
            schema = schema["items"]
        elif "anyOf" in schema or "oneOf" in schema:
            options = [
                option
                for option in schema.get("anyOf", schema.get("oneOf", []))
                if option.get("type") != "null"
            ]
            assert len(options) == 1, f"cannot pick one schema of {schema}"
            schema = options[0]
        else:
            return schema


def children(schema: dict[str, Any], root: dict[str, Any], name: str) -> dict[str, Any]:
    return resolve(resolve(schema, root)["properties"][name], root)


# Where each model sits in the document, as a path of property names from the root.
MODELS: list[tuple[tuple[str, ...], type[BaseModel]]] = [
    ((), blacksparrow.SiteExtract),
    (("pages",), blacksparrow.Page),
    (("pages", "links"), blacksparrow.PageLink),
    (("page_errors",), blacksparrow.PageError),
    (("design",), blacksparrow.Design),
    (("design", "colors"), blacksparrow.Color),
    (("design", "colors", "evidence"), blacksparrow.ColorEvidence),
    (("design", "fonts"), blacksparrow.Font),
    (("design", "logos"), blacksparrow.Logo),
    (("design", "stylesheets"), blacksparrow.Stylesheet),
    (("contacts",), blacksparrow.Contacts),
    (("structured",), blacksparrow.Structured),
    (("stats",), blacksparrow.Stats),
]


def schema_at(root: dict[str, Any], path: tuple[str, ...]) -> dict[str, Any]:
    node = root
    for name in path:
        node = children(node, root, name)
    return resolve(node, root)


def test_the_helpers_follow_refs_nullable_options_and_arrays():
    root: dict[str, Any] = {
        "type": "object",
        "properties": {
            "list": {"type": "array", "items": {"$ref": "#/$defs/Item"}},
            "maybe": {"anyOf": [{"$ref": "#/$defs/Item"}, {"type": "null"}]},
        },
        "$defs": {"Item": {"type": "object", "properties": {"a": {"type": "string"}}}},
    }

    assert set(schema_at(root, ("list",))["properties"]) == {"a"}
    assert set(schema_at(root, ("maybe",))["properties"]) == {"a"}


def test_wire_names_use_the_documents_spelling():
    assert "sourceUrl" in wire_names(blacksparrow.ColorEvidence)
    assert "source_url" not in wire_names(blacksparrow.ColorEvidence)


def binary_schema() -> dict[str, Any] | None:
    """The schema the binary under test prints, or None when there is no such binary."""
    try:
        binary = blacksparrow.binary_path()
    except blacksparrow.BinaryNotFoundError:
        return None
    completed = subprocess.run(
        [str(binary), "schema", "extract"],
        capture_output=True,
        timeout=60,
        check=False,
        stdin=subprocess.DEVNULL,
    )
    if completed.returncode != 0:
        return None
    return json.loads(completed.stdout)


@pytest.fixture(scope="module")
def schema() -> dict[str, Any]:
    """The schema of the binary under test, else the one in the repository."""
    printed = binary_schema()
    if printed is not None:
        return printed
    if not SCHEMA_PATH.is_file():
        pytest.skip("neither the binary nor schema/extract.schema.json provides the schema")
    return json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))


@pytest.mark.parametrize(("path", "model"), MODELS, ids=lambda value: str(value))
def test_the_models_have_the_properties_of_the_schema(
    schema: dict[str, Any], path: tuple[str, ...], model: type[BaseModel]
):
    properties = set(schema_at(schema, path)["properties"])

    assert wire_names(model) == properties


def test_the_schema_version_is_the_one_the_models_read(schema: dict[str, Any]):
    assert blacksparrow.SCHEMA_VERSION == 1
    assert "schema_version" in schema["required"]


def test_the_outcomes_are_the_ones_of_the_schema(schema: dict[str, Any]):
    options = schema["$defs"]["Outcome"]["oneOf"]

    assert {option["const"] for option in options} == set(typing.get_args(blacksparrow.Outcome))


def test_required_properties_of_the_schema_have_a_value_after_a_minimal_document(
    schema: dict[str, Any],
):
    minimal = {"schema_version": 1, "outcome": "unreachable"}

    result = _document.parse_extract(minimal)

    for name in schema["required"]:
        assert hasattr(result, name)
