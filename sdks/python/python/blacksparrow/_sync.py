"""Running the binary from threaded or blocking code."""

from __future__ import annotations

import contextlib
import subprocess
import threading
import time
from pathlib import Path
from typing import Any

from . import _process  # pyright: ignore[reportPrivateUsage]
from ._document import parse_document
from ._errors import BlackSparrowCancelled, BlackSparrowError, BlackSparrowTimeout
from ._models import SiteExtract
from ._process import Request, Workspace


def run(
    request: Request, executable: Path, cancel: threading.Event | None
) -> tuple[SiteExtract, dict[str, Any]]:
    """Run ``executable`` for ``request`` and return the validated result and the raw document.

    The process leads its own process group and gets a private temporary directory as its home
    and temp directory. A timeout, a cancel, an exceeded limit or any other error kills the whole
    group, so the browsers it started die too, and the directory is removed.

    Raises:
        BlackSparrowCancelled: ``cancel`` was set.
        BlackSparrowTimeout: The process outlived ``timeout_seconds`` plus the grace period.
        BlackSparrowError: The process could not start, failed, or printed no usable document.
    """
    if cancel is not None and cancel.is_set():
        raise BlackSparrowCancelled("blacksparrow was cancelled before it started")
    workspace = Workspace.create()
    try:
        process = _start(request, executable, workspace)
        try:
            code = _wait(process, request, workspace, cancel)
        finally:
            # Browsers the process left behind share its group; none may outlive the run.
            _process.kill_tree(process.pid, running=process.poll() is None)
            _reap(process)
        return parse_document(_process.read_document(workspace, request, code))
    finally:
        workspace.remove()


def _start(request: Request, executable: Path, workspace: Workspace) -> subprocess.Popen[bytes]:
    try:
        with workspace.errors.open("wb") as stderr:
            return subprocess.Popen(  # noqa: S603 - the executable is a checked absolute path
                request.command(executable, workspace.output),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=stderr,
                env=_process.minimal_environment(workspace.directory, request.env),
                cwd=workspace.directory,
                creationflags=_process.NEW_PROCESS_GROUP_FLAGS,
                start_new_session=_process.START_NEW_SESSION,
            )
    except OSError as error:
        raise BlackSparrowError(f"blacksparrow could not start: {error.strerror}") from error


def _wait(
    process: subprocess.Popen[bytes],
    request: Request,
    workspace: Workspace,
    cancel: threading.Event | None,
) -> int:
    """Wait for the process to end; raise when it must be stopped."""
    deadline = time.monotonic() + request.timeout_seconds + _process.KILL_GRACE_SECONDS
    while True:
        try:
            return process.wait(timeout=_process.POLL_SECONDS)
        except subprocess.TimeoutExpired:
            pass
        if cancel is not None and cancel.is_set():
            raise BlackSparrowCancelled("blacksparrow was cancelled")
        if time.monotonic() >= deadline:
            raise BlackSparrowTimeout("blacksparrow did not finish in time")
        problem = _process.limit_exceeded(workspace, request)
        if problem is not None:
            raise BlackSparrowError(problem)


def _reap(process: subprocess.Popen[bytes]) -> None:
    """Collect the exit status of the (killed) process so it does not stay a zombie."""
    with contextlib.suppress(subprocess.TimeoutExpired):
        process.wait(timeout=_process.REAP_SECONDS)
