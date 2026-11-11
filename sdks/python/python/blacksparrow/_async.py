"""Running the binary from asyncio code."""

from __future__ import annotations

import asyncio
import contextlib
import subprocess
import time
from pathlib import Path
from typing import Any

from . import _process  # pyright: ignore[reportPrivateUsage]
from ._document import parse_document
from ._errors import BlackSparrowError, BlackSparrowTimeout
from ._models import SiteExtract
from ._process import Request, Workspace


async def run(request: Request, executable: Path) -> tuple[SiteExtract, dict[str, Any]]:
    """Run ``executable`` for ``request`` and return the validated result and the raw document.

    Cancelling the awaiting task kills the process group (and so the browsers) before the
    cancellation propagates. See ``blacksparrow._sync.run`` for the rest of the behaviour.

    Raises:
        asyncio.CancelledError: The awaiting task was cancelled.
        BlackSparrowTimeout: The process outlived ``timeout_seconds`` plus the grace period.
        BlackSparrowError: The process could not start, failed, or printed no usable document.
    """
    workspace = Workspace.create()
    try:
        process = await _start(request, executable, workspace)
        try:
            code = await _wait(process, request, workspace)
        finally:
            _process.kill_tree(process.pid, running=process.returncode is None)
            with contextlib.suppress(asyncio.TimeoutError):
                await asyncio.wait_for(process.wait(), _process.REAP_SECONDS)
        # Reading and validating a large document is blocking work.
        return await asyncio.to_thread(_finish, workspace, request, code)
    finally:
        # The thread finishes the removal even when this task is cancelled while awaiting it.
        await asyncio.to_thread(workspace.remove)


def _finish(
    workspace: Workspace, request: Request, code: int
) -> tuple[SiteExtract, dict[str, Any]]:
    return parse_document(_process.read_document(workspace, request, code))


async def _start(
    request: Request, executable: Path, workspace: Workspace
) -> asyncio.subprocess.Process:
    try:
        with workspace.errors.open("wb") as stderr:
            return await asyncio.create_subprocess_exec(
                *request.command(executable, workspace.output),
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


async def _wait(process: asyncio.subprocess.Process, request: Request, workspace: Workspace) -> int:
    """Wait for the process to end; raise when it must be stopped."""
    deadline = time.monotonic() + request.timeout_seconds + _process.KILL_GRACE_SECONDS
    waiter = asyncio.ensure_future(process.wait())
    try:
        while True:
            done, _ = await asyncio.wait({waiter}, timeout=_process.POLL_SECONDS)
            if done:
                return waiter.result()
            if time.monotonic() >= deadline:
                raise BlackSparrowTimeout("blacksparrow did not finish in time")
            problem = _process.limit_exceeded(workspace, request)
            if problem is not None:
                raise BlackSparrowError(problem)
    finally:
        waiter.cancel()
