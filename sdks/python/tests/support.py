"""Helpers shared by the tests: the recorded document and fake binaries."""

from __future__ import annotations

import json
import stat
import sys
import time
from pathlib import Path
from typing import Any

DATA = Path(__file__).resolve().parent / "data"
BAKERY = DATA / "bakery_extract.json"
ON_WINDOWS = sys.platform == "win32"


def document() -> dict[str, Any]:
    """A real ``blacksparrow extract`` document, recorded from the bakery fixture site."""
    return json.loads(BAKERY.read_text(encoding="utf-8"))


def script(directory: Path, body: str, name: str = "blacksparrow") -> Path:
    """An executable shell script standing in for the binary."""
    path = directory / name
    path.write_text(f"#!/bin/sh\n{body}\n")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return path


# Finds the file the document goes to (``--output=FILE``) in the arguments, as ``$out``.
FIND_OUTPUT = 'out=; for a in "$@"; do case "$a" in --output=*) out="${a#--output=}";; esac; done\n'


def writing(directory: Path, body: str, name: str = "blacksparrow") -> Path:
    """A fake binary whose ``body`` can write the document to ``$out``."""
    return script(directory, FIND_OUTPUT + body, name)


def printing(directory: Path, payload: object, name: str = "blacksparrow") -> Path:
    """A fake binary that writes ``payload`` as the document."""
    answer = directory / f"{name}-answer.json"
    answer.write_text(json.dumps(payload))
    return writing(directory, f"cat '{answer}' > \"$out\"", name)


def alive(pid: int) -> bool:
    """Whether the process runs (a zombie waiting to be reaped does not)."""
    try:
        state = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0]
    except (FileNotFoundError, ProcessLookupError):
        return False
    return state != "Z"


def ends(pid: int, seconds: float = 5) -> bool:
    """Whether the process ends within a few seconds (a killed process takes a moment to die)."""
    deadline = time.monotonic() + seconds
    while alive(pid):
        if time.monotonic() > deadline:
            return False
        time.sleep(0.05)
    return True


def executable_file(directory: Path, name: str = "chrome") -> Path:
    """An executable file, for arguments that name one."""
    path = directory / name
    path.write_text("#!/bin/sh\n")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return path
