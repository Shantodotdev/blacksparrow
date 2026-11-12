"""The process is run with a hard limit, a minimal environment and a private directory.

Fake executables stand in for the binary, so these tests never touch the network. The same
scenarios run through the synchronous and the asynchronous API.
"""

from __future__ import annotations

import asyncio
import json
import os
import threading
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

import blacksparrow
from blacksparrow import (
    BlackSparrowCancelled,
    BlackSparrowError,
    BlackSparrowTimeout,
    SiteExtract,
    _process,
)
from support import BAKERY, ON_WINDOWS, document, ends, printing, script, writing

pytestmark = pytest.mark.skipif(ON_WINDOWS, reason="uses POSIX shell scripts")

Reader = Callable[..., tuple[SiteExtract, dict[str, Any]]]


def sync_read(binary: Path, url: str = "https://shop.test/", **options: Any):
    options.setdefault("timeout_seconds", 30)
    return blacksparrow.extract_raw(
        url, chrome_path=None, render="never", max_pages=6, binary=binary, **options
    )


def async_read(binary: Path, url: str = "https://shop.test/", **options: Any):
    options.setdefault("timeout_seconds", 30)
    options.pop("cancel", None)
    return asyncio.run(
        blacksparrow.extract_raw_async(
            url, chrome_path=None, render="never", max_pages=6, binary=binary, **options
        )
    )


@pytest.fixture(params=[sync_read, async_read], ids=["sync", "async"])
def read(request: pytest.FixtureRequest) -> Reader:
    return request.param


@pytest.fixture(autouse=True)
def short_grace(monkeypatch: pytest.MonkeyPatch):
    """A killed process takes no 30 seconds of grace in tests."""
    monkeypatch.setattr(_process, "KILL_GRACE_SECONDS", 0)
    monkeypatch.setattr(_process, "POLL_SECONDS", 0.05)
    monkeypatch.setattr(_process, "MIN_TIMEOUT_SECONDS", 1)


def test_extract_returns_the_result_and_the_raw_document(tmp_path: Path, read: Reader):
    binary = writing(tmp_path, f"cat '{BAKERY}' > \"$out\"")

    result, raw = read(binary, "https://www.samplebakery.test/")

    assert result.ok
    assert len(result.pages) == 4 and raw == document()


def test_extract_returns_only_the_result_and_extract_async_too(tmp_path: Path):
    binary = writing(tmp_path, f"cat '{BAKERY}' > \"$out\"")
    options: dict[str, Any] = {
        "chrome_path": None,
        "render": "never",
        "max_pages": 6,
        "timeout_seconds": 30,
        "binary": binary,
    }

    assert blacksparrow.extract("https://shop.test/", **options).ok
    assert asyncio.run(blacksparrow.extract_async("https://shop.test/", **options)).ok


def test_the_command_line_reaches_the_process(tmp_path: Path, read: Reader):
    seen = tmp_path / "args.txt"
    binary = writing(tmp_path, f"printf '%s\\n' \"$@\" > '{seen}'\ncat '{BAKERY}' > \"$out\"")

    read(binary, "http://127.0.0.1:9/", allow_private_network=True, max_output_bytes=1 << 20)

    args = seen.read_text().split("\n")
    assert args[0] == "extract" and args[-3:-1] == ["--", "http://127.0.0.1:9/"]
    assert "--allow-private-network" in args and "--render=never" in args


def test_an_unreadable_site_is_a_result_not_an_error(tmp_path: Path, read: Reader):
    binary = printing(tmp_path, {"schema_version": 1, "outcome": "bot_protected"})

    result, _ = read(binary)

    assert result.outcome == "bot_protected" and result.homepage is None


def test_a_non_zero_exit_is_an_error_with_the_stderr_tail(tmp_path: Path, read: Reader):
    binary = script(tmp_path, "echo 'boom: it broke' >&2\nexit 3")

    with pytest.raises(BlackSparrowError, match=r"exited with 3: boom: it broke"):
        read(binary)


def test_a_process_killed_by_a_signal_says_so(tmp_path: Path, read: Reader):
    binary = script(tmp_path, "kill -SEGV $$")

    with pytest.raises(BlackSparrowError, match="killed by signal 11"):
        read(binary)


def test_only_the_end_of_a_huge_stderr_is_kept(tmp_path: Path, read: Reader):
    binary = script(
        tmp_path, "head -c 3000000 /dev/zero | tr '\\0' 'x' >&2\necho THE-END >&2\nexit 2"
    )

    with pytest.raises(BlackSparrowError) as raised:
        read(binary)

    assert str(raised.value).endswith("THE-END") and len(str(raised.value)) < 700


def test_a_process_that_floods_its_stderr_is_stopped(
    tmp_path: Path, read: Reader, monkeypatch: pytest.MonkeyPatch
):
    monkeypatch.setattr(_process, "MAX_STDERR_FILE_BYTES", 100_000)
    pids = tmp_path / "pids.txt"
    binary = script(
        tmp_path, f"echo $$ > '{pids}'\nwhile :; do head -c 200000 /dev/zero >&2; sleep 0.05; done"
    )

    with pytest.raises(BlackSparrowError, match="too much to its standard error"):
        read(binary, timeout_seconds=30)

    assert ends(int(pids.read_text()))


def test_output_that_is_not_json_is_an_error(tmp_path: Path, read: Reader):
    binary = writing(tmp_path, "echo 'definitely not json' > \"$out\"")

    with pytest.raises(BlackSparrowError, match="printed no JSON"):
        read(binary)


@pytest.mark.parametrize("content", ["", "[]", "5", "null", "NaN", '{"schema_version": Infinity}'])
def test_output_that_is_not_a_document_is_an_error(tmp_path: Path, read: Reader, content: str):
    binary = writing(tmp_path, f"printf '%s' '{content}' > \"$out\"")

    with pytest.raises(BlackSparrowError):
        read(binary)


def test_output_with_invalid_utf8_is_an_error(tmp_path: Path, read: Reader):
    binary = writing(tmp_path, 'printf \'{"a": "\\377"}\' > "$out"')

    with pytest.raises(BlackSparrowError, match="printed no JSON"):
        read(binary)


def test_output_nested_too_deeply_is_an_error(tmp_path: Path, read: Reader):
    binary = writing(tmp_path, "head -c 400000 /dev/zero | tr '\\0' '[' > \"$out\"")

    with pytest.raises(BlackSparrowError, match="printed no JSON"):
        read(binary)


def test_a_process_that_writes_no_document_is_an_error(tmp_path: Path, read: Reader):
    with pytest.raises(BlackSparrowError, match="wrote no output"):
        read(script(tmp_path, "true"))


def test_a_document_of_an_unknown_version_is_an_error(tmp_path: Path, read: Reader):
    binary = printing(tmp_path, {"schema_version": 9, "outcome": "ok"})

    with pytest.raises(BlackSparrowError, match="unknown version"):
        read(binary)


def test_a_document_without_an_outcome_is_an_error(tmp_path: Path, read: Reader):
    binary = printing(tmp_path, {"schema_version": 1})

    with pytest.raises(BlackSparrowError, match="unusable"):
        read(binary)


def test_nul_characters_are_removed_from_every_string_and_key(tmp_path: Path, read: Reader):
    payload = document()
    payload["pages"][0]["title"] = "Sam\x00ple"
    payload["pages"][0]["markdown"] = "a\x00b"
    payload["design"]["extra\x00key"] = ["x\x00y", {"n\x00": "z\x00"}]
    binary = printing(tmp_path, payload)

    result, raw = read(binary)

    assert "\\u0000" not in json.dumps(raw)
    assert result.pages[0].title == "Sample" and result.pages[0].markdown == "ab"
    assert raw["design"]["extrakey"] == ["xy", {"n": "z"}]


def test_a_process_that_outlives_its_deadline_is_killed(tmp_path: Path, read: Reader):
    pids = tmp_path / "pids.txt"
    binary = script(tmp_path, f"echo $$ > '{pids}'\nexec sleep 30")

    started = time.monotonic()
    with pytest.raises(BlackSparrowTimeout, match="did not finish in time"):
        read(binary, timeout_seconds=1)

    assert time.monotonic() - started < 10 and isinstance(
        BlackSparrowTimeout("x"), BlackSparrowError
    )
    assert ends(int(pids.read_text()))


def test_the_browsers_a_killed_process_started_are_killed_with_it(tmp_path: Path, read: Reader):
    pids = tmp_path / "pids.txt"
    binary = script(tmp_path, f"sleep 30 &\necho $! > '{pids}'\nwait")

    with pytest.raises(BlackSparrowTimeout):
        read(binary, timeout_seconds=1)

    assert ends(int(pids.read_text()))


def test_the_browsers_left_by_a_process_that_exits_are_killed(tmp_path: Path, read: Reader):
    pids = tmp_path / "pids.txt"
    binary = writing(tmp_path, f"sleep 30 &\necho $! > '{pids}'\ncat '{BAKERY}' > \"$out\"")

    read(binary)

    assert ends(int(pids.read_text()))


def test_the_browsers_left_by_a_failed_process_are_killed(tmp_path: Path, read: Reader):
    pids = tmp_path / "pids.txt"
    binary = script(tmp_path, f"sleep 30 &\necho $! > '{pids}'\nexit 1")

    with pytest.raises(BlackSparrowError):
        read(binary)

    assert ends(int(pids.read_text()))


def test_a_cancelled_reading_kills_the_process_and_its_browsers(tmp_path: Path):
    pids = tmp_path / "pids.txt"
    binary = script(tmp_path, f"sleep 30 &\necho $! > '{pids}'\nwait")
    cancel = threading.Event()
    threading.Timer(0.5, cancel.set).start()
    started = time.monotonic()

    with pytest.raises(BlackSparrowCancelled, match="cancelled"):
        blacksparrow.extract(
            "https://shop.test/",
            chrome_path=None,
            render="never",
            max_pages=6,
            timeout_seconds=60,
            cancel=cancel,
            binary=binary,
        )

    assert time.monotonic() - started < 10
    assert ends(int(pids.read_text()))


def test_a_reading_cancelled_before_it_starts_runs_nothing(tmp_path: Path):
    marker = tmp_path / "ran"
    binary = script(tmp_path, f"touch '{marker}'")
    cancel = threading.Event()
    cancel.set()

    with pytest.raises(BlackSparrowCancelled):
        blacksparrow.extract(
            "https://shop.test/",
            chrome_path=None,
            render="never",
            max_pages=6,
            timeout_seconds=60,
            cancel=cancel,
            binary=binary,
        )

    assert not marker.exists()


def test_cancelling_the_task_kills_the_process_and_its_browsers(tmp_path: Path):
    pids = tmp_path / "pids.txt"
    binary = script(tmp_path, f"sleep 30 &\necho $! > '{pids}'\necho $$ >> '{pids}'\nwait")

    async def scenario() -> float:
        task = asyncio.ensure_future(
            blacksparrow.extract_async(
                "https://shop.test/",
                chrome_path=None,
                render="never",
                max_pages=6,
                timeout_seconds=60,
                binary=binary,
            )
        )
        while not pids.exists() or len(pids.read_text().split()) < 2:
            await asyncio.sleep(0.05)
        started = time.monotonic()
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
        return time.monotonic() - started

    elapsed = asyncio.run(scenario())

    assert elapsed < 10
    assert all(ends(int(pid)) for pid in pids.read_text().split())


def test_the_process_gets_a_minimal_environment_and_a_private_directory(
    tmp_path: Path, read: Reader, monkeypatch: pytest.MonkeyPatch
):
    seen = tmp_path / "env.txt"
    where = tmp_path / "where.txt"
    binary = writing(
        tmp_path,
        f"env > '{seen}'\npwd > '{where}'\ntouch \"$TMPDIR/profile\"\ncat '{BAKERY}' > \"$out\"",
    )
    monkeypatch.setenv("DATABASE_URL", "postgres://secret")
    monkeypatch.setenv("OPENROUTER_API_KEY", "secret")
    monkeypatch.setenv("HTTPS_PROXY", "http://proxy.test:3128")
    monkeypatch.setenv("LANG", "en_US.UTF-8")
    monkeypatch.delenv("LC_ALL", raising=False)

    read(binary)

    environment = dict(line.split("=", 1) for line in seen.read_text().splitlines() if "=" in line)
    assert environment["PATH"] == os.environ["PATH"] and environment["LANG"] == "en_US.UTF-8"
    assert environment["HOME"] == environment["TMPDIR"]
    assert not {"DATABASE_URL", "OPENROUTER_API_KEY", "HTTPS_PROXY", "LC_ALL"} & set(environment)
    assert Path(where.read_text().strip()) == Path(environment["TMPDIR"]).resolve()
    assert not Path(environment["TMPDIR"]).exists()


def test_extra_environment_variables_are_passed_explicitly(tmp_path: Path, read: Reader):
    seen = tmp_path / "env.txt"
    binary = writing(tmp_path, f"env > '{seen}'\ncat '{BAKERY}' > \"$out\"")

    read(binary, env={"SSL_CERT_FILE": "/etc/ssl/ca.pem", "HOME": "/custom/home"})

    environment = dict(line.split("=", 1) for line in seen.read_text().splitlines() if "=" in line)
    assert environment["SSL_CERT_FILE"] == "/etc/ssl/ca.pem"
    assert environment["HOME"] == "/custom/home"


def test_the_private_directory_is_removed_when_the_process_fails(tmp_path: Path, read: Reader):
    seen = tmp_path / "dir.txt"
    binary = script(tmp_path, f"echo \"$TMPDIR\" > '{seen}'\nexit 1")

    with pytest.raises(BlackSparrowError):
        read(binary)

    assert not Path(seen.read_text().strip()).exists()


def test_the_private_directory_is_removed_after_a_timeout(tmp_path: Path, read: Reader):
    seen = tmp_path / "dir.txt"
    binary = script(tmp_path, f"echo \"$TMPDIR\" > '{seen}'\nexec sleep 30")

    with pytest.raises(BlackSparrowTimeout):
        read(binary, timeout_seconds=1)

    assert not Path(seen.read_text().strip()).exists()


def test_the_private_directory_is_readable_by_the_owner_only(tmp_path: Path, read: Reader):
    seen = tmp_path / "mode.txt"
    binary = writing(tmp_path, f"stat -c %a \"$TMPDIR\" > '{seen}'\ncat '{BAKERY}' > \"$out\"")

    read(binary)

    assert seen.read_text().strip() == "700"


def test_output_beyond_the_limit_is_an_error_before_it_is_read(tmp_path: Path, read: Reader):
    binary = writing(tmp_path, f"cat '{BAKERY}' > \"$out\"")

    with pytest.raises(BlackSparrowError, match="too much output"):
        read(binary, max_output_bytes=1024)


def test_a_process_whose_output_grows_past_the_limit_is_stopped(tmp_path: Path, read: Reader):
    pids = tmp_path / "pids.txt"
    binary = writing(
        tmp_path,
        f"echo $$ > '{pids}'\nwhile :; do head -c 100000 /dev/zero >> \"$out\"; sleep 0.05; done",
    )

    started = time.monotonic()
    with pytest.raises(BlackSparrowError, match="too much output"):
        read(binary, max_output_bytes=250_000, timeout_seconds=30)

    assert time.monotonic() - started < 10
    assert ends(int(pids.read_text()))


def test_a_binary_that_cannot_start_is_an_error(tmp_path: Path, read: Reader):
    plain = tmp_path / "blacksparrow"
    plain.write_text("not a program")
    plain.chmod(0o755)

    with pytest.raises(BlackSparrowError, match="could not start"):
        read(plain)


def test_a_missing_binary_is_reported_before_anything_runs(tmp_path: Path, read: Reader):
    with pytest.raises(blacksparrow.BinaryNotFoundError):
        read(tmp_path / "missing")


def test_runs_do_not_share_state(tmp_path: Path):
    binary = writing(tmp_path, f"cat '{BAKERY}' > \"$out\"")

    results = []
    threads = [
        threading.Thread(target=lambda: results.append(sync_read(binary)[0].ok)) for _ in range(6)
    ]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()

    assert results == [True] * 6


def test_several_async_readings_run_at_once(tmp_path: Path):
    binary = writing(tmp_path, f"sleep 1\ncat '{BAKERY}' > \"$out\"")

    async def scenario() -> list[SiteExtract]:
        return await asyncio.gather(
            *(
                blacksparrow.extract_async(
                    "https://shop.test/",
                    chrome_path=None,
                    render="never",
                    max_pages=6,
                    timeout_seconds=30,
                    binary=binary,
                )
                for _ in range(5)
            )
        )

    started = time.monotonic()
    results = asyncio.run(scenario())

    assert all(result.ok for result in results) and time.monotonic() - started < 4
