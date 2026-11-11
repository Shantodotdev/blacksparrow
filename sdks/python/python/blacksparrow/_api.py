"""The public functions: validate the arguments, find the binary, run it."""

from __future__ import annotations

import threading
from collections.abc import Mapping, Sequence
from typing import Any

from . import _async, _sync
from ._binary import binary_path
from ._document import unsupported_url
from ._models import SiteExtract, web_url
from ._process import DEFAULT_MAX_OUTPUT_BYTES, PathLike, RenderMode, build_request


def extract_raw(
    url: str,
    *,
    chrome_path: PathLike | None,
    render: RenderMode = "auto",
    max_pages: int,
    timeout_seconds: int,
    allow_private_network: bool = False,
    proxy: str | None = None,
    user_agent: str | None = None,
    chrome_no_sandbox: bool = False,
    pages: Sequence[str] = (),
    env: Mapping[str, str] | None = None,
    max_output_bytes: int = DEFAULT_MAX_OUTPUT_BYTES,
    cancel: threading.Event | None = None,
    binary: PathLike | None = None,
) -> tuple[SiteExtract, dict[str, Any]]:
    """Read the website at ``url``, returning the result and the raw document.

    The raw document is the binary's JSON as decoded, with NUL characters removed. Applications
    that keep their own record of a run store it, because it carries everything the models drop
    (fields of a newer binary, values the models reject).

    Args:
        url: The website address. One that is not an absolute http(s) address is not an error:
            the result has outcome ``"unsupported_url"`` and the binary is not run.
        chrome_path: Absolute path of a Chromium or chrome-headless-shell executable, used to
            render pages that need JavaScript. Required unless ``render`` is ``"never"``; ``None``
            is accepted only then.
        render: When to render in Chromium: ``"auto"`` (only JavaScript shells and bot
            challenges), ``"always"`` or ``"never"``.
        max_pages: Pages read in total, homepage included (1 to 100).
        timeout_seconds: The binary's own deadline for the whole site, at least 5. The process
            is killed 30 seconds after it if it still runs.
        allow_private_network: Allow loopback and private addresses. Off by default: the binary
            refuses them for every request, also inside the browser. Turn it on only for sites
            you run yourself.
        proxy: An ``http://[user:pass@]host:port`` egress proxy for all traffic. The proxy
            resolves names, so it must enforce its own egress rules. Credentials in the URL are
            visible to local users in the process list.
        user_agent: The User-Agent of plain fetches; the binary's own when None.
        chrome_no_sandbox: Run Chromium without its sandbox. Needed in containers without user
            namespaces; the browser only loads pages the address guard allows.
        pages: Pages to read after the homepage instead of choosing them by rules.
        env: Extra environment variables for the process, added to the minimal environment (it
            inherits only ``PATH``, the locale variables and, on Windows, the few variables
            needed to start a program). Use it for settings the binary needs, such as
            ``SSL_CERT_FILE``.
        max_output_bytes: The largest document accepted; the process is stopped when its output
            file grows past it.
        cancel: Set from another thread to stop the reading; the process group is then killed
            and ``BlackSparrowCancelled`` is raised.
        binary: Absolute path of the ``blacksparrow`` binary to run. By default the
            ``BLACKSPARROW_BINARY`` environment variable, then the binary of the installed wheel.

    Returns:
        The validated result and the raw document.

    Raises:
        TypeError: An argument has the wrong type.
        ValueError: An argument has an unusable value.
        BinaryNotFoundError: No usable binary (see ``binary_path``).
        BlackSparrowCancelled: ``cancel`` was set.
        BlackSparrowTimeout: The process outlived its deadline.
        BlackSparrowError: The process could not start, crashed, printed too much, or printed no
            usable document.
    """
    request = build_request(
        url,
        chrome_path=chrome_path,
        render=render,
        max_pages=max_pages,
        timeout_seconds=timeout_seconds,
        allow_private_network=allow_private_network,
        proxy=proxy,
        user_agent=user_agent,
        chrome_no_sandbox=chrome_no_sandbox,
        pages=pages,
        env=env,
        max_output_bytes=max_output_bytes,
    )
    if web_url(url) is None:
        return unsupported_url(url)
    return _sync.run(request, binary_path(binary), cancel)


def extract(
    url: str,
    *,
    chrome_path: PathLike | None,
    render: RenderMode = "auto",
    max_pages: int,
    timeout_seconds: int,
    allow_private_network: bool = False,
    proxy: str | None = None,
    user_agent: str | None = None,
    chrome_no_sandbox: bool = False,
    pages: Sequence[str] = (),
    env: Mapping[str, str] | None = None,
    max_output_bytes: int = DEFAULT_MAX_OUTPUT_BYTES,
    cancel: threading.Event | None = None,
    binary: PathLike | None = None,
) -> SiteExtract:
    """Read the website at ``url``.

    Takes the same arguments and raises the same errors as ``extract_raw``; see there.

    Returns:
        The validated result. A site that could not be read is a result with an ``outcome`` other
        than ``"ok"``, not an exception.
    """
    return extract_raw(
        url,
        chrome_path=chrome_path,
        render=render,
        max_pages=max_pages,
        timeout_seconds=timeout_seconds,
        allow_private_network=allow_private_network,
        proxy=proxy,
        user_agent=user_agent,
        chrome_no_sandbox=chrome_no_sandbox,
        pages=pages,
        env=env,
        max_output_bytes=max_output_bytes,
        cancel=cancel,
        binary=binary,
    )[0]


async def extract_raw_async(
    url: str,
    *,
    chrome_path: PathLike | None,
    render: RenderMode = "auto",
    max_pages: int,
    timeout_seconds: int,
    allow_private_network: bool = False,
    proxy: str | None = None,
    user_agent: str | None = None,
    chrome_no_sandbox: bool = False,
    pages: Sequence[str] = (),
    env: Mapping[str, str] | None = None,
    max_output_bytes: int = DEFAULT_MAX_OUTPUT_BYTES,
    binary: PathLike | None = None,
) -> tuple[SiteExtract, dict[str, Any]]:
    """Read the website at ``url`` without blocking the event loop.

    Takes the same arguments as ``extract_raw`` except ``cancel``: cancel the awaiting task
    instead. The process group (and so the browsers) is killed and its temporary directory
    removed before the ``asyncio.CancelledError`` propagates.
    """
    request = build_request(
        url,
        chrome_path=chrome_path,
        render=render,
        max_pages=max_pages,
        timeout_seconds=timeout_seconds,
        allow_private_network=allow_private_network,
        proxy=proxy,
        user_agent=user_agent,
        chrome_no_sandbox=chrome_no_sandbox,
        pages=pages,
        env=env,
        max_output_bytes=max_output_bytes,
    )
    if web_url(url) is None:
        return unsupported_url(url)
    return await _async.run(request, binary_path(binary))


async def extract_async(
    url: str,
    *,
    chrome_path: PathLike | None,
    render: RenderMode = "auto",
    max_pages: int,
    timeout_seconds: int,
    allow_private_network: bool = False,
    proxy: str | None = None,
    user_agent: str | None = None,
    chrome_no_sandbox: bool = False,
    pages: Sequence[str] = (),
    env: Mapping[str, str] | None = None,
    max_output_bytes: int = DEFAULT_MAX_OUTPUT_BYTES,
    binary: PathLike | None = None,
) -> SiteExtract:
    """Read the website at ``url`` without blocking the event loop.

    Takes the same arguments and raises the same errors as ``extract_raw_async``; see there.
    """
    result, _ = await extract_raw_async(
        url,
        chrome_path=chrome_path,
        render=render,
        max_pages=max_pages,
        timeout_seconds=timeout_seconds,
        allow_private_network=allow_private_network,
        proxy=proxy,
        user_agent=user_agent,
        chrome_no_sandbox=chrome_no_sandbox,
        pages=pages,
        env=env,
        max_output_bytes=max_output_bytes,
        binary=binary,
    )
    return result


__all__ = ["binary_path", "extract", "extract_async", "extract_raw", "extract_raw_async"]
