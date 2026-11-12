"""The models against real runs of the binary on the repository's fixture sites."""

from __future__ import annotations

import asyncio
import os
import sys
import tempfile
import threading
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

import blacksparrow
from blacksparrow import SiteExtract

pytestmark = pytest.mark.live

Serve = Callable[[str], str]


def read(binary: Path, url: str, **options: Any) -> tuple[SiteExtract, dict[str, Any]]:
    options.setdefault("chrome_path", None)
    options.setdefault("render", "never")
    options.setdefault("max_pages", 6)
    options.setdefault("timeout_seconds", 60)
    options.setdefault("allow_private_network", True)
    return blacksparrow.extract_raw(url, binary=binary, **options)


def unmodelled(raw: dict[str, Any]) -> list[str]:
    """The names in the raw document that the models do not have."""
    models = blacksparrow
    missing: list[str] = []

    def check(where: str, data: Any, model: type) -> None:
        if isinstance(data, dict):
            missing.extend(f"{where}.{key}" for key in data if key not in model.model_fields)

    check("document", raw, models.SiteExtract)
    for page in raw.get("pages", []):
        check("page", page, models.Page)
        for link in page.get("links", []):
            check("link", link, models.PageLink)
    for error in raw.get("page_errors", []):
        check("page_error", error, models.PageError)
    design = raw.get("design") or {}
    check("design", design, models.Design)
    for color in design.get("colors", []):
        check("color", color, models.Color)
        for evidence in color.get("evidence", []):
            # ``sourceUrl`` is the document's name for ``source_url``.
            check(
                "evidence",
                {k.replace("sourceUrl", "source_url"): v for k, v in evidence.items()},
                models.ColorEvidence,
            )
    for font in design.get("fonts", []):
        check("font", font, models.Font)
    for logo in design.get("logos", []):
        check("logo", logo, models.Logo)
    for sheet in design.get("stylesheets", []):
        check("stylesheet", sheet, models.Stylesheet)
    check("contacts", raw.get("contacts"), models.Contacts)
    check("structured", raw.get("structured"), models.Structured)
    check("stats", raw.get("stats"), models.Stats)
    return missing


def test_the_models_read_a_real_run_of_the_bakery_site(real_binary: Path, serve: Serve):
    url = serve("bakery")

    result, raw = read(real_binary, url)

    assert result.ok and result.outcome_detail is None
    assert result.start_url == url and result.final_url == url
    assert result.schema_version == blacksparrow.SCHEMA_VERSION
    assert result.generator == f"blacksparrow {blacksparrow.version(real_binary)}"
    assert unmodelled(raw) == []
    assert result.homepage is not None and result.homepage.status == 200
    assert result.homepage.title is not None and "Sweet Crumbs" in result.homepage.title
    assert result.homepage.markdown and result.homepage.lang == "bn"
    assert {page.kind for page in result.pages} >= {"home", "contact", "about"}
    assert all(not page.rendered for page in result.pages)
    assert result.contacts.emails == ["hello@sweetcrumbs.com.bd"]
    assert result.contacts.phones and result.contacts.whatsapp and result.contacts.facebook
    assert result.structured.names == ["Sweet Crumbs Bakery"]
    assert result.design is not None and result.design.theme_color == "#C0392B"
    assert result.design.colors and result.design.colors[0].hex == "#C0392B"
    assert {font.family for font in result.design.fonts} >= {"Playfair Display"}
    assert result.design.logos and result.design.stylesheets
    assert result.stats.http_fetches > 0 and result.stats.rendered_pages == 0
    # Nothing in the raw document was lost on the way into the models.
    assert len(result.pages) == len(raw["pages"])
    assert len(result.design.colors) == len(raw["design"]["colors"])
    assert len(result.design.fonts) == len(raw["design"]["fonts"])
    assert len(result.design.logos) == len(raw["design"]["logos"])
    assert [page.url for page in result.pages] == [page["url"] for page in raw["pages"]]


def test_max_pages_and_requested_pages_are_honoured(real_binary: Path, serve: Serve):
    url = serve("bakery")

    one, _ = read(real_binary, url, max_pages=1)
    named, _ = read(real_binary, url, pages=[f"{url}shop.html"], max_pages=2)

    assert [page.kind for page in one.pages] == ["home"]
    assert [page.kind for page in named.pages] == ["home", "requested"]
    assert named.pages[1].url == f"{url}shop.html"


def test_the_async_api_reads_the_same_site(real_binary: Path, serve: Serve):
    url = serve("bakery")

    result = asyncio.run(
        blacksparrow.extract_async(
            url,
            chrome_path=None,
            render="never",
            max_pages=6,
            timeout_seconds=60,
            allow_private_network=True,
            binary=real_binary,
        )
    )
    blocking, _ = read(real_binary, url)

    assert result.ok and [p.url for p in result.pages] == [p.url for p in blocking.pages]


def test_loopback_is_refused_unless_the_private_network_is_allowed(real_binary: Path, serve: Serve):
    url = serve("bakery")

    result, raw = read(real_binary, url, allow_private_network=False)

    assert result.outcome == "blocked_address" and not result.ok
    assert result.pages == [] and result.homepage is None and result.design is None
    assert raw["outcome"] == "blocked_address"


def test_a_site_that_cannot_be_reached_is_a_result(real_binary: Path):
    result, _ = read(real_binary, "http://127.0.0.1:9/", timeout_seconds=20)

    assert result.outcome == "unreachable" and result.pages == []
    assert result.outcome_detail is not None


def test_a_page_that_is_not_html_is_a_result(real_binary: Path, serve: Serve):
    url = serve("spa")

    result, _ = read(real_binary, f"{url}app.js", timeout_seconds=20)

    assert result.outcome == "not_html" and result.pages == []


def test_a_missing_page_is_an_http_error_result(real_binary: Path, serve: Serve):
    url = serve("bakery")

    result, _ = read(real_binary, f"{url}no-such-page.html", timeout_seconds=20)

    assert result.outcome == "http_error" and not result.ok
    assert result.homepage is not None and result.homepage.status == 404


def test_version_and_module_entry_point_report_the_binary(real_binary: Path):
    import subprocess

    version = blacksparrow.version(real_binary)
    completed = subprocess.run(
        [sys.executable, "-m", "blacksparrow", "--version"],
        capture_output=True,
        text=True,
        check=False,
        env={
            "PYTHONPATH": str(Path(blacksparrow.__file__).parent.parent),
            "BLACKSPARROW_BINARY": str(real_binary),
        },
    )

    assert version and completed.stdout.strip() == f"blacksparrow {version}"


# --- rendering (needs BLACKSPARROW_TEST_CHROME) --------------------------------------------


def chrome_processes(directory: Path, needle: bytes = b"") -> list[int]:
    """Pids of processes that have ``directory`` (the temp dir of the runs) in their command line.

    The private directory of a run is the home of the binary and of its browser, so these are the
    processes of the runs made while ``tempfile.tempdir`` was ``directory``. ``needle`` narrows
    them to command lines that also contain it.
    """
    pids: list[int] = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            command = (entry / "cmdline").read_bytes()
        except OSError:
            continue
        if (
            str(directory).encode() in command
            and needle in command
            and int(entry.name) != os.getpid()
        ):
            pids.append(int(entry.name))
    return pids


@pytest.fixture
def private_temp(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Makes the runs' private directories appear under ``tmp_path`` (and only these runs')."""
    directory = tmp_path / "runs"
    directory.mkdir()
    monkeypatch.setattr(tempfile, "tempdir", str(directory))
    return directory


def test_a_javascript_site_is_rendered_and_its_browsers_are_gone_afterwards(
    real_binary: Path, serve: Serve, chrome: Path, private_temp: Path
):
    url = serve("spa")

    result, _ = read(
        real_binary,
        url,
        render="auto",
        chrome_path=chrome,
        chrome_no_sandbox=True,
        timeout_seconds=90,
    )

    assert result.ok and result.homepage is not None and result.homepage.rendered
    assert result.homepage.render_reason == "js_shell"
    assert result.stats.rendered_pages >= 1 and result.stats.render_ms > 0
    if sys.platform.startswith("linux"):
        assert chrome_processes(private_temp) == []
        assert list(private_temp.iterdir()) == []


@pytest.mark.skipif(not sys.platform.startswith("linux"), reason="reads /proc")
def test_cancelling_a_rendering_run_kills_the_browser_too(
    real_binary: Path, serve: Serve, chrome: Path, private_temp: Path
):
    url = serve("spa")
    cancel = threading.Event()

    def cancel_when_the_browser_runs() -> None:
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline and not chrome_processes(
            private_temp, b"--user-data-dir"
        ):
            time.sleep(0.02)
        cancel.set()

    threading.Thread(target=cancel_when_the_browser_runs, daemon=True).start()

    with pytest.raises(blacksparrow.BlackSparrowCancelled):
        read(
            real_binary,
            url,
            render="always",
            chrome_path=chrome,
            chrome_no_sandbox=True,
            timeout_seconds=90,
            cancel=cancel,
        )

    deadline = time.monotonic() + 10
    while chrome_processes(private_temp) and time.monotonic() < deadline:
        time.sleep(0.05)
    assert chrome_processes(private_temp) == []
