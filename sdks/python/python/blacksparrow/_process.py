"""What the synchronous and asynchronous runners share: a validated request, the command line, the
private working directory, the minimal environment, the limits and the process-tree kill."""

from __future__ import annotations

import contextlib
import os
import shutil
import signal
import subprocess
import sys
import tempfile
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Literal, get_args
from urllib.parse import urlsplit

from ._errors import BlackSparrowError
from ._models import MAX_PAGES, web_url

RenderMode = Literal["auto", "always", "never"]
PathLike = str | os.PathLike[str]

DEFAULT_MAX_OUTPUT_BYTES = 16 * 1024 * 1024
"""The largest document read by default; six pages of Markdown with links stay far below it."""

MIN_TIMEOUT_SECONDS = 5
MAX_TIMEOUT_SECONDS = 3600
KILL_GRACE_SECONDS = 30
"""Seconds beyond the binary's own deadline before the process is killed."""
POLL_SECONDS = 0.2
"""Seconds between two looks at a running process (its end, a cancel, a limit)."""
STDERR_TAIL_BYTES = 4096
MAX_STDERR_FILE_BYTES = 8 * 1024 * 1024
REAP_SECONDS = 10
_MAX_ENV_ENTRIES = 200

_PASSED_ENVIRONMENT = ("PATH", "LANG", "LC_ALL")
_PASSED_WINDOWS_ENVIRONMENT = ("SYSTEMROOT", "SYSTEMDRIVE", "WINDIR", "COMSPEC", "PATHEXT")
_PRIVATE_ENVIRONMENT = ("HOME", "TMPDIR", "TMP", "TEMP", "USERPROFILE", "APPDATA", "LOCALAPPDATA")


@dataclass(frozen=True)
class Request:
    """The validated arguments of one extraction."""

    url: str
    render: RenderMode
    chrome_path: Path | None
    max_pages: int
    timeout_seconds: int
    allow_private_network: bool
    proxy: str | None
    user_agent: str | None
    chrome_no_sandbox: bool
    pages: tuple[str, ...]
    env: Mapping[str, str]
    max_output_bytes: int

    def command(self, executable: Path, output: Path) -> list[str]:
        """The command line that reads the site and writes the JSON document to ``output``.

        Values that come from the caller use the ``--option=value`` form, so one that starts with
        a dash cannot be read as another option; the address follows ``--`` for the same reason.
        """
        command = [
            str(executable),
            "extract",
            f"--max-pages={self.max_pages}",
            f"--render={self.render}",
            f"--timeout-secs={self.timeout_seconds}",
            f"--output={output}",
        ]
        if self.chrome_path is not None:
            command.append(f"--chrome-path={self.chrome_path}")
        if self.chrome_no_sandbox:
            command.append("--chrome-no-sandbox")
        if self.allow_private_network:
            command.append("--allow-private-network")
        if self.proxy is not None:
            command.append(f"--proxy={self.proxy}")
        if self.user_agent is not None:
            command.append(f"--user-agent={self.user_agent}")
        command.extend(f"--page={page}" for page in self.pages)
        return [*command, "--", self.url]


def build_request(
    url: str,
    *,
    chrome_path: PathLike | None,
    render: RenderMode,
    max_pages: int,
    timeout_seconds: int,
    allow_private_network: bool,
    proxy: str | None,
    user_agent: str | None,
    chrome_no_sandbox: bool,
    pages: Sequence[str],
    env: Mapping[str, str] | None,
    max_output_bytes: int,
) -> Request:
    """Validate the arguments of an extraction.

    ``url`` is only checked to be text; whether it is a readable address is a result of the run,
    not an argument error.

    Raises:
        TypeError: An argument has the wrong type.
        ValueError: An argument has an unusable value.
    """
    if not isinstance(url, str):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise TypeError("url must be a string")
    if render not in get_args(RenderMode):
        raise ValueError(f"render must be 'auto', 'always' or 'never', not {render!r}")
    return Request(
        url=url,
        render=render,
        chrome_path=_chrome_path(chrome_path, render),
        max_pages=_bounded_int("max_pages", max_pages, 1, MAX_PAGES),
        timeout_seconds=_bounded_int(
            "timeout_seconds", timeout_seconds, MIN_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS
        ),
        allow_private_network=_flag("allow_private_network", allow_private_network),
        proxy=_proxy(proxy),
        user_agent=_user_agent(user_agent),
        chrome_no_sandbox=_flag("chrome_no_sandbox", chrome_no_sandbox),
        pages=_pages(pages),
        env=_extra_environment(env),
        max_output_bytes=_bounded_int("max_output_bytes", max_output_bytes, 1024, 1 << 30),
    )


def _chrome_path(chrome_path: PathLike | None, render: str) -> Path | None:
    if render == "never":
        return None
    if chrome_path is None:
        raise ValueError(f"chrome_path is required when render is {render!r}")
    path = Path(os.path.abspath(os.fspath(chrome_path)))
    if not path.is_file() or not os.access(path, os.X_OK):
        raise ValueError(f"chrome_path is not an executable file: {path}")
    return path


def _bounded_int(name: str, value: int, low: int, high: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise TypeError(f"{name} must be an integer")
    if not low <= value <= high:
        raise ValueError(f"{name} must be between {low} and {high}, not {value}")
    return value


def _flag(name: str, value: bool) -> bool:
    if not isinstance(value, bool):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise TypeError(f"{name} must be a bool")
    return value


def _proxy(proxy: str | None) -> str | None:
    if proxy is None:
        return None
    if not isinstance(proxy, str):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise TypeError("proxy must be a string")
    try:
        parts = urlsplit(proxy)
        parts.port  # noqa: B018 - raises ValueError for a malformed port
    except ValueError as error:
        raise ValueError("proxy is not a valid URL") from error
    if (
        parts.scheme.lower() != "http"
        or not parts.hostname
        or any(char.isspace() or ord(char) < 0x20 for char in proxy)
    ):
        raise ValueError("proxy must be an http://[user:pass@]host:port URL")
    return proxy


def _user_agent(user_agent: str | None) -> str | None:
    if user_agent is None:
        return None
    if not isinstance(user_agent, str):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise TypeError("user_agent must be a string")
    if not user_agent.strip() or any(ord(char) < 0x20 or ord(char) == 0x7F for char in user_agent):
        raise ValueError("user_agent must be non-empty text without control characters")
    return user_agent


def _pages(pages: Sequence[str]) -> tuple[str, ...]:
    if isinstance(pages, str) or not isinstance(pages, Sequence):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise TypeError("pages must be a sequence of URLs, not a single string")
    if len(pages) > MAX_PAGES:
        raise ValueError(f"pages can name at most {MAX_PAGES} URLs")
    for page in pages:
        if web_url(page) is None:
            raise ValueError(f"pages must be absolute http(s) URLs, got {page!r}")
    return tuple(pages)


def _extra_environment(env: Mapping[str, str] | None) -> Mapping[str, str]:
    if env is None:
        return {}
    if len(env) > _MAX_ENV_ENTRIES:
        raise ValueError(f"env can hold at most {_MAX_ENV_ENTRIES} variables")
    for name, value in env.items():
        if (
            not isinstance(name, str)  # pyright: ignore[reportUnnecessaryIsInstance]
            or not isinstance(value, str)  # pyright: ignore[reportUnnecessaryIsInstance]
            or not name
            or "=" in name
            or "\x00" in name
            or "\x00" in value
        ):
            raise ValueError(f"env has an unusable variable: {name!r}")
    return dict(env)


@dataclass(frozen=True)
class Workspace:
    """A private temporary directory for one run: the process's home and temp directory (the
    browser's profile lives there), its document and its standard error."""

    directory: Path

    @classmethod
    def create(cls) -> Workspace:
        """Make the directory; it is readable by the current user only."""
        return cls(Path(tempfile.mkdtemp(prefix="blacksparrow-")))

    @property
    def output(self) -> Path:
        """Where the binary writes its JSON document."""
        return self.directory / "output.json"

    @property
    def errors(self) -> Path:
        """Where the binary's standard error goes."""
        return self.directory / "stderr.log"

    def remove(self) -> None:
        """Delete the directory and everything the run left in it."""
        shutil.rmtree(self.directory, ignore_errors=True)


def minimal_environment(directory: Path, extra: Mapping[str, str] | None) -> dict[str, str]:
    """The environment of the process: no inherited secrets.

    Only ``PATH``, the locale variables (and the few variables Windows needs to start any
    program) are inherited; ``HOME``, ``TMPDIR`` and their Windows counterparts point at the
    private ``directory``. ``extra`` is added last and may override anything.
    """
    environment: dict[str, str] = {}
    names = _PASSED_ENVIRONMENT + (_PASSED_WINDOWS_ENVIRONMENT if sys.platform == "win32" else ())
    for name in names:
        value = os.environ.get(name)
        if value:
            environment[name] = value
    environment.update(dict.fromkeys(_PRIVATE_ENVIRONMENT, str(directory)))
    if extra:
        environment.update(extra)
    return environment


# The options that make the process lead its own process group (Windows) or session (POSIX).
if sys.platform == "win32":
    NEW_PROCESS_GROUP_FLAGS = subprocess.CREATE_NEW_PROCESS_GROUP | subprocess.CREATE_NO_WINDOW
    START_NEW_SESSION = False
else:
    NEW_PROCESS_GROUP_FLAGS = 0
    START_NEW_SESSION = True


def kill_tree(pid: int, *, running: bool) -> None:
    """Kill the process and every process it started.

    On POSIX the process leads its own session, so its group is killed; browsers it left behind
    share the group and die with it, also after the process itself has exited. On Windows
    ``taskkill /T`` ends the tree of a process that still runs; once the parent has exited its
    children can no longer be found.
    """
    if sys.platform == "win32":
        if running:
            root = os.environ.get("SYSTEMROOT", r"C:\Windows")
            taskkill = os.path.join(root, "System32", "taskkill.exe")
            with contextlib.suppress(OSError, subprocess.TimeoutExpired):
                subprocess.run(  # noqa: S603
                    [taskkill, "/F", "/T", "/PID", str(pid)],
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    timeout=REAP_SECONDS,
                    check=False,
                )
        return
    with contextlib.suppress(ProcessLookupError, PermissionError):
        os.killpg(pid, signal.SIGKILL)


def limit_exceeded(workspace: Workspace, request: Request) -> str | None:
    """A message when the document or the standard error file has grown past its limit."""
    if _size(workspace.output) > request.max_output_bytes:
        return "blacksparrow printed too much output"
    if _size(workspace.errors) > MAX_STDERR_FILE_BYTES:
        return "blacksparrow wrote too much to its standard error"
    return None


def _size(path: Path) -> int:
    try:
        return path.stat().st_size
    except OSError:
        return 0


def read_document(workspace: Workspace, request: Request, code: int) -> bytes:
    """The bytes of the document a finished process wrote.

    Raises:
        BlackSparrowError: The process failed, wrote nothing, or wrote more than the limit.
    """
    if code != 0:
        tail = _tail(workspace.errors, STDERR_TAIL_BYTES).decode("utf-8", errors="replace")
        how = f"was killed by signal {-code}" if code < 0 else f"exited with {code}"
        detail = f": {tail[-500:].strip()}" if tail.strip() else ""
        raise BlackSparrowError(f"blacksparrow {how}{detail}")
    try:
        size = workspace.output.stat().st_size
    except OSError as error:
        raise BlackSparrowError("blacksparrow wrote no output") from error
    if size > request.max_output_bytes:
        raise BlackSparrowError("blacksparrow printed too much output")
    with workspace.output.open("rb") as file:
        # One byte more than allowed: a file that grew after the check is still caught.
        raw = file.read(request.max_output_bytes + 1)
    if len(raw) > request.max_output_bytes:
        raise BlackSparrowError("blacksparrow printed too much output")
    return raw


def _tail(path: Path, size: int) -> bytes:
    """The last ``size`` bytes of the file; empty when it cannot be read."""
    try:
        with path.open("rb") as file:
            file.seek(0, os.SEEK_END)
            file.seek(max(file.tell() - size, 0))
            return file.read()
    except OSError:
        return b""
