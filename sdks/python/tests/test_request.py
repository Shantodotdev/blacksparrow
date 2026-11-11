"""The arguments are validated before anything runs, and become one safe command line."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import pytest

import blacksparrow
from blacksparrow._process import Request, build_request
from support import ON_WINDOWS, executable_file, script

pytestmark = pytest.mark.skipif(ON_WINDOWS, reason="uses POSIX shell scripts")


def request(tmp_path: Path, **overrides: Any) -> Request:
    options: dict[str, Any] = {
        "chrome_path": None,
        "render": "never",
        "max_pages": 6,
        "timeout_seconds": 30,
        "allow_private_network": False,
        "proxy": None,
        "user_agent": None,
        "chrome_no_sandbox": False,
        "pages": (),
        "env": None,
        "max_output_bytes": 1 << 20,
    }
    options.update(overrides)
    return build_request("https://shop.test/", **options)


def test_the_command_reads_the_site_without_the_private_network_by_default(tmp_path: Path):
    command = request(tmp_path).command(Path("/opt/blacksparrow"), Path("/w/out.json"))

    assert command[:2] == ["/opt/blacksparrow", "extract"]
    assert command[-2:] == ["--", "https://shop.test/"]
    assert "--max-pages=6" in command and "--render=never" in command
    assert "--timeout-secs=30" in command and "--output=/w/out.json" in command
    assert not [arg for arg in command if arg.startswith("--chrome-path")]
    assert "--chrome-no-sandbox" not in command and "--allow-private-network" not in command


def test_every_option_reaches_the_command_before_the_address(tmp_path: Path):
    chrome = executable_file(tmp_path)

    command = request(
        tmp_path,
        chrome_path=chrome,
        render="always",
        allow_private_network=True,
        chrome_no_sandbox=True,
        proxy="http://user:pw@proxy.test:3128",
        user_agent="-x agent",
        pages=["https://shop.test/a", "https://shop.test/b"],
    ).command(Path("/opt/blacksparrow"), Path("/w/out.json"))

    assert command[command.index("--") :] == ["--", "https://shop.test/"]
    assert f"--chrome-path={chrome}" in command and "--render=always" in command
    assert "--chrome-no-sandbox" in command and "--allow-private-network" in command
    assert "--proxy=http://user:pw@proxy.test:3128" in command
    # A value that starts with a dash stays attached to its option.
    assert "--user-agent=-x agent" in command
    assert command.count("--page=https://shop.test/a") == 1
    assert "--page=https://shop.test/b" in command


def test_a_relative_chrome_path_is_resolved_against_the_callers_directory(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    chrome = executable_file(tmp_path)
    monkeypatch.chdir(tmp_path)

    assert request(tmp_path, chrome_path="chrome", render="auto").chrome_path == chrome


@pytest.mark.parametrize("render", ["auto", "always"])
def test_rendering_needs_a_chrome_path(tmp_path: Path, render: str):
    with pytest.raises(ValueError, match="chrome_path is required"):
        request(tmp_path, render=render)


def test_a_chrome_path_that_is_not_an_executable_file_is_rejected(tmp_path: Path):
    plain = tmp_path / "plain"
    plain.write_text("x")

    for bad in (tmp_path / "missing", plain, tmp_path):
        with pytest.raises(ValueError, match="not an executable file"):
            request(tmp_path, chrome_path=bad, render="auto")


def test_a_bare_chrome_name_is_never_looked_up_on_the_path(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv("PATH", "/usr/bin:/bin")

    with pytest.raises(ValueError, match="not an executable file"):
        request(tmp_path, chrome_path="sh", render="auto")


def test_chrome_path_is_ignored_when_nothing_is_rendered(tmp_path: Path):
    assert request(tmp_path, chrome_path=tmp_path / "missing", render="never").chrome_path is None


@pytest.mark.parametrize(
    ("name", "value"),
    [
        ("render", "sometimes"),
        ("max_pages", 0),
        ("max_pages", 101),
        ("timeout_seconds", 4),
        ("timeout_seconds", 100_000),
        ("max_output_bytes", 10),
        ("proxy", "socks5://proxy.test:1080"),
        ("proxy", "proxy.test:3128"),
        ("proxy", "http://"),
        ("proxy", "http://proxy.test:99999"),
        ("proxy", "http://proxy.test/ x"),
        ("user_agent", ""),
        ("user_agent", "bad\nagent"),
        ("pages", ["javascript:alert(1)"]),
        ("pages", ["/relative"]),
        ("env", {"": "x"}),
        ("env", {"A=B": "x"}),
        ("env", {"A": "x\x00"}),
    ],
)
def test_an_unusable_value_is_rejected(tmp_path: Path, name: str, value: Any):
    with pytest.raises(ValueError, match=name if name != "env" else "env"):
        request(tmp_path, **{name: value})


@pytest.mark.parametrize(
    ("name", "value"),
    [
        ("max_pages", "6"),
        ("max_pages", True),
        ("max_pages", 6.0),
        ("timeout_seconds", None),
        ("allow_private_network", 1),
        ("chrome_no_sandbox", "yes"),
        ("proxy", 5),
        ("user_agent", 5),
        ("pages", "https://shop.test/"),
        ("pages", 5),
        ("env", {"A": 1}),
    ],
)
def test_a_value_of_the_wrong_type_is_rejected(tmp_path: Path, name: str, value: Any):
    with pytest.raises((TypeError, ValueError)):
        request(tmp_path, **{name: value})


def test_the_url_must_be_text():
    bytes_url: Any = b"https://shop.test/"

    with pytest.raises(TypeError, match="url"):
        blacksparrow.extract(
            bytes_url, chrome_path=None, render="never", max_pages=6, timeout_seconds=30
        )


def test_chrome_path_has_no_default():
    extract: Any = blacksparrow.extract

    with pytest.raises(TypeError, match="chrome_path"):
        extract("https://shop.test/", max_pages=6, timeout_seconds=30)


@pytest.mark.parametrize(
    "url",
    [
        "javascript:alert(1)",
        "-x",
        "file:///etc/passwd",
        "ftp://x.test/",
        "x.test",
        "",
        " ",
        "http://",
    ],
)
def test_an_address_that_is_not_http_is_a_result_and_the_binary_is_not_needed(
    tmp_path: Path, url: str
):
    # The binary does not exist, yet there is no error: nothing is run.
    result, raw = blacksparrow.extract_raw(
        url,
        chrome_path=None,
        render="never",
        max_pages=6,
        timeout_seconds=30,
        binary=tmp_path / "missing",
    )

    assert result.outcome == "unsupported_url" and result.pages == [] and not result.ok
    assert raw["outcome"] == "unsupported_url" and raw["schema_version"] == 1


def test_the_arguments_are_checked_before_the_address(tmp_path: Path):
    with pytest.raises(ValueError, match="max_pages"):
        blacksparrow.extract(
            "javascript:x", chrome_path=None, render="never", max_pages=0, timeout_seconds=30
        )


def test_the_process_is_not_started_for_an_invalid_argument(tmp_path: Path):
    marker = tmp_path / "ran"
    binary = script(tmp_path, f"touch '{marker}'")

    with pytest.raises(ValueError, match="timeout_seconds"):
        blacksparrow.extract(
            "https://shop.test/",
            chrome_path=None,
            render="never",
            max_pages=6,
            timeout_seconds=1,
            binary=binary,
        )

    assert not marker.exists()
