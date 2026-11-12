"""The document is read leniently: wrong types become absent, lists are capped, extras ignored."""

from __future__ import annotations

import copy
from typing import Any

import pytest
from pydantic import ValidationError

import blacksparrow
from blacksparrow import BlackSparrowError, SiteExtract
from blacksparrow._document import parse_extract
from support import document


def test_a_real_document_is_read_completely():
    result = parse_extract(document())

    assert result.outcome == "ok" and result.ok and result.homepage is not None
    assert result.schema_version == blacksparrow.SCHEMA_VERSION == 1
    assert result.generator == "blacksparrow 0.1.0"
    assert [page.kind for page in result.pages] == ["home", "contact", "about", "catalog"]
    assert result.homepage.lang == "bn" and result.homepage.status == 200
    assert result.homepage.phones == ["01711-223344"]
    assert result.homepage.facebook == ["https://www.facebook.com/SweetCrumbsBD"]
    assert result.contacts.emails == ["hello@sweetcrumbs.com.bd"]
    assert result.structured.names == ["Sweet Crumbs Bakery"]
    assert result.design is not None
    assert result.design.theme_color == "#C0392B" and result.design.site_name == "Sweet Crumbs"
    assert [color.hex for color in result.design.colors][:2] == ["#C0392B", "#2E86C1"]
    assert result.design.colors[0].evidence[1].source_url is not None
    assert result.design.colors[0].evidence[1].name == "--brand"
    assert [(font.family, font.weights) for font in result.design.fonts][0] == (
        "Hind Siliguri",
        [400, 500],
    )
    assert [logo.reason for logo in result.design.logos][:2] == ["header_logo", "header_logo"]
    assert result.design.logos[1].svg is not None and result.design.logos[1].url is None
    assert [sheet.status for sheet in result.design.stylesheets] == [200, 200]
    assert result.stats.http_fetches == 6 and result.stats.peak_rss_kib is not None


def test_every_field_of_the_document_is_modelled():
    raw = document()
    pages = raw["pages"][0]
    design = raw["design"]

    assert set(raw) <= set(SiteExtract.model_fields)
    assert set(pages) <= set(blacksparrow.Page.model_fields)
    assert set(design) <= set(blacksparrow.Design.model_fields)
    assert set(raw["contacts"]) == set(blacksparrow.Contacts.model_fields)
    assert set(raw["structured"]) == set(blacksparrow.Structured.model_fields)
    assert set(raw["stats"]) <= set(blacksparrow.Stats.model_fields)
    assert set(design["stylesheets"][0]) <= set(blacksparrow.Stylesheet.model_fields)
    assert set(design["logos"][0]) == set(blacksparrow.Logo.model_fields)
    assert set(design["fonts"][0]) == set(blacksparrow.Font.model_fields)
    assert set(design["colors"][0]) == set(blacksparrow.Color.model_fields)
    assert set(pages["links"][0]) == set(blacksparrow.PageLink.model_fields)


@pytest.mark.parametrize("version", [0, 2, "1", None, True])
def test_an_unknown_schema_version_is_rejected(version: Any):
    payload = document()
    payload["schema_version"] = version

    with pytest.raises(BlackSparrowError, match="unknown version"):
        parse_extract(payload)


def test_a_missing_schema_version_or_a_non_object_is_rejected():
    payload = document()
    del payload["schema_version"]

    for bad in (payload, [], "x", None, 5):
        with pytest.raises(BlackSparrowError):
            parse_extract(bad)


@pytest.mark.parametrize("outcome", [None, "exploded", 5, ["ok"]])
def test_a_document_without_a_valid_outcome_is_unusable(outcome: Any):
    payload = document()
    payload["outcome"] = outcome

    with pytest.raises(BlackSparrowError, match="unusable"):
        parse_extract(payload)


@pytest.mark.parametrize(
    "outcome",
    [
        "unsupported_url",
        "unresolved",
        "blocked_address",
        "unreachable",
        "bot_protected",
        "http_error",
        "not_html",
    ],
)
def test_an_unreadable_site_keeps_its_outcome_and_has_no_homepage(outcome: str):
    result = parse_extract({"schema_version": 1, "outcome": outcome, "outcome_detail": "dns"})

    assert result.outcome == outcome and not result.ok and result.outcome_detail == "dns"
    assert result.pages == [] and result.homepage is None and result.design is None
    assert result.contacts.phones == [] and result.stats.elapsed_ms == 0


def test_the_homepage_is_the_page_of_kind_home_not_the_first_page():
    payload = document()
    payload["pages"].reverse()

    homepage = parse_extract(payload).homepage

    assert homepage is not None and homepage.kind == "home"


def test_without_a_page_of_kind_home_there_is_no_homepage():
    payload = document()
    payload["pages"] = payload["pages"][1:]

    result = parse_extract(payload)

    assert result.ok and result.homepage is None


def test_an_invalid_homepage_leaves_no_homepage():
    payload = document()
    payload["pages"][0]["url"] = "not a url"

    assert parse_extract(payload).homepage is None


def test_wrongly_typed_fields_become_absent_instead_of_failing():
    payload = document()
    home = payload["pages"][0]
    home["phones"] = {"number": "01711000000"}
    home["emails"] = 5
    home["markdown"] = ["not", "text"]
    home["title"] = {"x": 1}
    home["word_count"] = "many"
    home["status"] = True
    home["rendered"] = "yes"
    home["lang"] = 5
    payload["design"]["colors"] = "red"
    payload["design"]["stylesheets"] = 3
    payload["structured"]["addresses"] = {"street": "x"}
    payload["contacts"] = "none"
    payload["stats"] = [1]
    payload["generator"] = 7

    result = parse_extract(payload)

    page = result.pages[0]
    assert page.phones == [] and page.emails == [] and page.markdown == ""
    assert page.title is None and page.word_count == 0 and page.status == 0
    assert page.rendered is False and page.lang is None
    assert result.design is not None and result.design.colors == []
    assert result.design.stylesheets == []
    assert result.structured.addresses == []
    assert result.contacts.phones == [] and result.stats.elapsed_ms == 0
    assert result.generator is None


def test_a_design_that_is_not_an_object_is_absent():
    payload = document()
    payload["design"] = "blue"

    assert parse_extract(payload).design is None


def test_bad_urls_are_dropped_from_links_and_social_lists():
    payload = document()
    home = payload["pages"][0]
    home["links"] = [
        {"text": "ok", "url": "https://shop.test/about"},
        {"text": "script", "url": "javascript:alert(1)"},
        {"text": "relative", "url": "/relative"},
        {"text": "data", "url": "data:text/html,x"},
        {"text": "spaces", "url": "https://shop.test/a b"},
        {"text": "port", "url": "https://shop.test:99999/"},
        "garbage",
        {"text": "second", "url": "https://shop.test/menu"},
    ]
    home["facebook"] = ["https://www.facebook.com/a", "ftp://x", 5, "https://www.facebook.com/a"]
    home["whatsapp"] = ["not a url"]
    payload["design"]["logos"] = [
        {"reason": "x", "url": "file:///etc/passwd", "svg": None},
        {"reason": "y", "url": "https://shop.test/logo.svg", "svg": None},
    ]

    result = parse_extract(payload)

    page = result.pages[0]
    assert [link.url for link in page.links] == [
        "https://shop.test/about",
        "https://shop.test/menu",
    ]
    assert page.facebook == ["https://www.facebook.com/a"] and page.whatsapp == []
    assert result.design is not None
    assert [logo.url for logo in result.design.logos] == [None, "https://shop.test/logo.svg"]


def test_a_protocol_relative_url_is_read_as_https():
    payload = document()
    payload["design"]["logos"] = [{"reason": "icon", "url": "//cdn.shop.test/x.png"}]

    result = parse_extract(payload)

    assert result.design is not None and result.design.logos[0].url == "https://cdn.shop.test/x.png"


def test_invalid_pages_colors_and_fonts_are_skipped_not_fatal():
    payload = document()
    payload["pages"] = ["junk", {"url": "not a url"}, *payload["pages"]]
    payload["page_errors"] = [{"url": "https://shop.test/x"}, {"error": "no url"}, 4]
    payload["design"]["colors"] = [
        {"hex": "red"},
        {"hex": "#abcdef"},
        {"hex": "#ABCDEF", "usage_score": 3},
        {"hex": "#ABCDEF", "usage_score": float("nan")},
        {"hex": "#ABCDEF", "occurrences": -4, "usage_score": 0.5},
    ]
    payload["design"]["fonts"] = [{"family": None}, {"family": "Lora", "weights": [0, 400, "x"]}]
    payload["design"]["theme_color"] = "teal"

    result = parse_extract(payload)

    assert [page.kind for page in result.pages] == ["home", "contact", "about", "catalog"]
    assert [(error.url, error.error) for error in result.page_errors] == [
        ("https://shop.test/x", "unknown error")
    ]
    assert result.design is not None
    assert [(c.hex, c.occurrences) for c in result.design.colors] == [("#ABCDEF", 0)]
    assert result.design.theme_color is None
    assert [(font.family, font.weights) for font in result.design.fonts] == [("Lora", [400])]


def test_lists_and_texts_are_capped():
    payload = document()
    payload["pages"] = payload["pages"] * 40
    payload["pages"][0]["markdown"] = "x" * 400_000
    payload["pages"][0]["phones"] = [f"0171100{n:04d}" for n in range(60)]
    payload["pages"][0]["links"] = [
        {"text": "t" * 500, "url": f"https://s.test/{n}"} for n in range(300)
    ]
    payload["design"]["logos"] = [
        {"reason": "icon", "url": f"https://s.test/{n}"} for n in range(50)
    ]
    payload["design"]["logos"][0]["svg"] = "<svg/>" * 100_000

    result = parse_extract(payload)

    assert len(result.pages) == 100
    page = result.pages[0]
    assert len(page.phones) == 20 and len(page.links) == 200
    assert len(page.markdown) == 300_000 and len(page.links[0].text) == 200
    assert result.design is not None and len(result.design.logos) == 12
    assert result.design.logos[0].svg is not None and len(result.design.logos[0].svg) == 200_000


def test_duplicates_are_removed_from_string_lists():
    payload = document()
    payload["pages"][0]["emails"] = ["a@s.test", " a@s.test ", "b@s.test", "", None]

    assert parse_extract(payload).pages[0].emails == ["a@s.test", "b@s.test"]


def test_unknown_fields_are_ignored():
    payload = copy.deepcopy(document())
    payload["surprise"] = {"x": 1}
    payload["pages"][0]["surprise"] = 1
    payload["design"]["colors"][0]["surprise"] = 1

    assert parse_extract(payload).ok


def test_results_are_frozen():
    result = parse_extract(document())

    with pytest.raises(ValidationError):
        result.outcome = "unreachable"  # type: ignore[misc]
    with pytest.raises(ValidationError):
        result.pages[0].title = "x"  # type: ignore[misc]


def test_a_result_survives_a_round_trip_through_json():
    result = parse_extract(document())

    assert SiteExtract.model_validate_json(result.model_dump_json()) == result
