"""Fixtures for the tests that run the real binary against the repository's fixture sites."""

from __future__ import annotations

import functools
import http.server
import os
import threading
from collections.abc import Iterator
from pathlib import Path

import pytest

import blacksparrow

SITES = Path(__file__).resolve().parents[3] / "tests" / "fixtures" / "sites"
CHROME_VARIABLE = "BLACKSPARROW_TEST_CHROME"


class _QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, format: str, *args: object) -> None:  # noqa: A002
        return


@pytest.fixture(scope="session")
def real_binary() -> Path:
    """The binary under test: BLACKSPARROW_BINARY, or the one of an installed wheel.

    A binary that is named but unusable is an error; the live tests are skipped only when there
    is none at all.
    """
    try:
        return blacksparrow.binary_path()
    except blacksparrow.BinaryNotFoundError:
        if os.environ.get(blacksparrow.ENVIRONMENT_VARIABLE):
            raise
        pytest.skip("no blacksparrow binary: set BLACKSPARROW_BINARY or install a wheel")


@pytest.fixture(scope="session")
def chrome() -> Path:
    """The Chromium for the rendering tests; they are skipped without BLACKSPARROW_TEST_CHROME."""
    path = os.environ.get(CHROME_VARIABLE)
    if not path:
        pytest.skip(f"{CHROME_VARIABLE} is not set")
    return Path(path)


@pytest.fixture
def serve() -> Iterator[object]:
    """Serves a fixture site on 127.0.0.1; ``serve("bakery")`` returns its base URL."""
    servers: list[http.server.ThreadingHTTPServer] = []

    def start(site: str) -> str:
        directory = SITES / site
        if not directory.is_dir():
            pytest.skip(f"fixture site {site} is not available")
        handler = functools.partial(_QuietHandler, directory=str(directory))
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        servers.append(server)
        return f"http://127.0.0.1:{server.server_address[1]}/"

    yield start
    for server in servers:
        server.shutdown()
        server.server_close()
