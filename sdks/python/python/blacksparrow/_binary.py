"""Finding the ``blacksparrow`` binary.

The order is fixed and never consults ``PATH``:

1. an explicit ``binary`` argument,
2. the ``BLACKSPARROW_BINARY`` environment variable,
3. the binary the wheel installed next to the package (found the way ruff finds its own).
"""

from __future__ import annotations

import os
import subprocess
import sys
import sysconfig
from pathlib import Path

from ._errors import BinaryNotFoundError, BlackSparrowError
from ._process import Workspace, minimal_environment

ENVIRONMENT_VARIABLE = "BLACKSPARROW_BINARY"
_EXECUTABLE_NAME = "blacksparrow"
_VERSION_TIMEOUT_SECONDS = 15
_VERSION_PREFIX = "blacksparrow "


def binary_path(binary: str | os.PathLike[str] | None = None) -> Path:
    """The ``blacksparrow`` binary that will run.

    Args:
        binary: An absolute path to use instead of the environment variable and the bundled
            binary.

    Returns:
        The absolute path of an executable file.

    Raises:
        BinaryNotFoundError: The given path or the one in ``BLACKSPARROW_BINARY`` is not an
            absolute path to an executable file, or no bundled binary was found.
    """
    if binary is not None:
        return _checked(binary, "the binary argument")
    override = os.environ.get(ENVIRONMENT_VARIABLE)
    if override:
        return _checked(override, ENVIRONMENT_VARIABLE)
    bundled = _find_bundled()
    if bundled is None:
        raise BinaryNotFoundError(
            "the blacksparrow binary was not found next to this package; reinstall it from a "
            f"platform wheel, or set {ENVIRONMENT_VARIABLE} to the absolute path of a binary"
        )
    return bundled


def version(binary: str | os.PathLike[str] | None = None) -> str:
    """The version of the ``blacksparrow`` binary, such as ``"0.1.0"``.

    Args:
        binary: See ``binary_path``.

    Raises:
        BinaryNotFoundError: See ``binary_path``.
        BlackSparrowError: The binary did not answer ``--version`` with a version.
    """
    executable = binary_path(binary)
    workspace = Workspace.create()
    try:
        completed = subprocess.run(  # noqa: S603 - the executable is a checked absolute path
            [str(executable), "--version"],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            env=minimal_environment(workspace.directory, None),
            cwd=workspace.directory,
            timeout=_VERSION_TIMEOUT_SECONDS,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise BlackSparrowError(f"blacksparrow --version failed: {error}") from error
    finally:
        workspace.remove()
    output = completed.stdout.decode("utf-8", errors="replace").strip()
    if completed.returncode != 0 or not output.startswith(_VERSION_PREFIX):
        raise BlackSparrowError(f"blacksparrow --version printed no version: {output[:200]!r}")
    return output.removeprefix(_VERSION_PREFIX).strip()


def _checked(candidate: str | os.PathLike[str], source: str) -> Path:
    """``candidate`` as a path, when it is an absolute path to an executable file."""
    path = Path(os.fspath(candidate))
    if not path.is_absolute():
        raise BinaryNotFoundError(f"{source} must be an absolute path, got {str(path)!r}")
    if not path.is_file():
        raise BinaryNotFoundError(f"{source} is not a file: {path}")
    if not os.access(path, os.X_OK):
        raise BinaryNotFoundError(f"{source} is not executable: {path}")
    return path


def _find_bundled() -> Path | None:
    """The binary the wheel installed, or None.

    A wheel puts the binary in the scripts directory of the environment it is installed into.
    That is searched in the same places ruff searches for its own binary: the scripts directory
    of the running environment and of its base Python, above or beside the installed package
    (``pip install --prefix`` and ``--target``), and the user scheme's scripts directory.
    """
    executable = _EXECUTABLE_NAME + (sysconfig.get_config_var("EXE") or "")
    seen: set[str] = set()
    for directory in _script_directories():
        if directory is None or str(directory) in seen:
            continue
        seen.add(str(directory))
        candidate = directory / executable
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate
    return None


def _script_directories() -> list[Path | None]:
    package = Path(__file__).resolve().parent
    return [
        _scripts_path(),
        _scripts_path(vars={"base": sys.base_prefix}),
        _above_package(package),
        package.parent / "bin",
        _scripts_path(scheme=_user_scheme()),
    ]


def _scripts_path(**options: object) -> Path | None:
    try:
        path = sysconfig.get_path("scripts", **options)  # type: ignore[arg-type]
    except (KeyError, ValueError):
        return None
    return Path(path) if path else None


def _above_package(package: Path) -> Path | None:
    """The ``bin`` (``Scripts``) directory above ``lib/pythonX.Y/site-packages/blacksparrow``."""
    parents = package.parents
    if sys.platform == "win32":
        # <prefix>\Lib\site-packages\blacksparrow
        if len(parents) >= 3 and parents[0].name.lower() == "site-packages":
            return parents[1].parent / "Scripts"
        return None
    # <prefix>/lib/pythonX.Y/site-packages/blacksparrow
    if len(parents) >= 4 and parents[0].name == "site-packages":
        return parents[2].parent / "bin"
    return None


def _user_scheme() -> str:
    """The name of the user installation scheme of this platform."""
    if sys.platform == "win32":
        return "nt_user"
    if sys.platform == "darwin" and getattr(sys, "_framework", None):
        return "osx_framework_user"
    return "posix_user"
