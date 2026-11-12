"""The binary comes from an explicit path, the environment variable or the wheel; never PATH."""

from __future__ import annotations

import sys
import sysconfig
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

import blacksparrow
from blacksparrow import BinaryNotFoundError, BlackSparrowError, _binary, binary_path
from support import ON_WINDOWS, script

pytestmark = pytest.mark.skipif(ON_WINDOWS, reason="uses POSIX shell scripts")


@pytest.fixture(autouse=True)
def clean_environment(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.delenv("BLACKSPARROW_BINARY", raising=False)


def real_path(
    real: Callable[..., str], name: str, scheme: str | None, variables: dict[str, str] | None
) -> str:
    """The answer of the real ``sysconfig.get_path`` for the same question."""
    options: dict[str, Any] = {}
    if scheme is not None:
        options["scheme"] = scheme
    if variables is not None:
        options["vars"] = variables
    return real(name, **options)


def bundled(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> Path:
    """Install a fake binary where the wheel puts it, and make that the scripts directory."""
    scripts = tmp_path / "venv" / "bin"
    scripts.mkdir(parents=True)
    binary = script(scripts, "echo 'blacksparrow 9.9.9'")
    real = sysconfig.get_path

    def get_path(name: str, scheme: str | None = None, vars: dict[str, str] | None = None) -> str:  # noqa: A002
        if name == "scripts" and scheme is None and vars is None:
            return str(scripts)
        return real_path(real, name, scheme, vars)

    monkeypatch.setattr(sysconfig, "get_path", get_path)
    return binary


def test_the_environment_variable_names_the_binary(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    binary = script(tmp_path, "true")
    monkeypatch.setenv("BLACKSPARROW_BINARY", str(binary))

    assert binary_path() == binary


def test_an_explicit_path_wins_over_the_environment_and_the_wheel(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    installed = bundled(monkeypatch, tmp_path)
    other = script(tmp_path, "true", "other")
    monkeypatch.setenv("BLACKSPARROW_BINARY", str(installed))

    assert binary_path(other) == other
    assert binary_path(str(other)) == other


def test_the_environment_variable_wins_over_the_wheel(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    bundled(monkeypatch, tmp_path)
    other = script(tmp_path, "true", "other")
    monkeypatch.setenv("BLACKSPARROW_BINARY", str(other))

    assert binary_path() == other


def test_without_an_override_the_binary_of_the_wheel_is_used(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    installed = bundled(monkeypatch, tmp_path)

    assert binary_path() == installed
    assert blacksparrow.version() == "9.9.9"


@pytest.mark.parametrize("value", ["blacksparrow", "bin/blacksparrow", "./blacksparrow"])
def test_an_override_must_be_an_absolute_path(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, value: str
):
    script(tmp_path, "true")
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv("BLACKSPARROW_BINARY", value)

    with pytest.raises(BinaryNotFoundError, match="absolute"):
        binary_path()


def test_an_override_that_is_not_an_executable_file_is_an_error(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    plain = tmp_path / "plain"
    plain.write_text("x")
    monkeypatch.setenv("BLACKSPARROW_BINARY", str(tmp_path / "missing"))
    with pytest.raises(BinaryNotFoundError, match="not a file"):
        binary_path()

    monkeypatch.setenv("BLACKSPARROW_BINARY", str(tmp_path))
    with pytest.raises(BinaryNotFoundError, match="not a file"):
        binary_path()

    monkeypatch.setenv("BLACKSPARROW_BINARY", str(plain))
    with pytest.raises(BinaryNotFoundError, match="not executable"):
        binary_path()


def test_a_bad_override_is_never_replaced_by_the_wheels_binary(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    bundled(monkeypatch, tmp_path)
    monkeypatch.setenv("BLACKSPARROW_BINARY", str(tmp_path / "missing"))

    with pytest.raises(BinaryNotFoundError):
        binary_path()


def test_the_path_is_never_searched(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    script(tmp_path, "true")
    monkeypatch.setenv("PATH", str(tmp_path))
    monkeypatch.setattr(_binary, "_script_directories", lambda: [])

    with pytest.raises(BinaryNotFoundError, match="not found next to this package"):
        binary_path()


def test_a_missing_binary_says_how_to_fix_it(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setattr(_binary, "_script_directories", lambda: [])

    with pytest.raises(BinaryNotFoundError, match="BLACKSPARROW_BINARY") as raised:
        binary_path()

    assert isinstance(raised.value, BlackSparrowError)


def test_the_directories_searched_follow_ruffs_order(monkeypatch: pytest.MonkeyPatch):
    directories = _binary._script_directories()

    assert len(directories) == 5
    assert directories[0] == Path(sysconfig.get_path("scripts"))
    assert directories[3] == Path(_binary.__file__).resolve().parent.parent / "bin"


def test_a_binary_beside_the_package_is_found_for_pip_target_installs(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    target = tmp_path / "target"
    (target / "bin").mkdir(parents=True)
    installed = script(target / "bin", "true")
    monkeypatch.setattr(_binary, "__file__", str(target / "blacksparrow" / "_binary.py"))
    (target / "blacksparrow").mkdir()
    monkeypatch.setattr(sysconfig, "get_path", lambda *_a, **_k: None)

    assert binary_path() == installed


def test_a_binary_above_the_package_is_found_for_prefix_installs(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    prefix = tmp_path / "prefix"
    package = prefix / "lib" / "python3.12" / "site-packages" / "blacksparrow"
    package.mkdir(parents=True)
    (prefix / "bin").mkdir()
    installed = script(prefix / "bin", "true")
    monkeypatch.setattr(_binary, "__file__", str(package / "_binary.py"))
    monkeypatch.setattr(sysconfig, "get_path", lambda *_a, **_k: None)

    assert binary_path() == installed


def test_the_user_scheme_is_searched_last(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    user_scripts = tmp_path / "user" / "bin"
    user_scripts.mkdir(parents=True)
    installed = script(user_scripts, "true")
    real = sysconfig.get_path

    def get_path(name: str, scheme: str | None = None, vars: dict[str, str] | None = None) -> str:  # noqa: A002
        if scheme is not None and scheme.endswith("_user"):
            return str(user_scripts)
        if scheme is None and vars is None:
            return str(tmp_path / "empty")
        return real_path(real, name, scheme, vars)

    monkeypatch.setattr(sysconfig, "get_path", get_path)
    monkeypatch.setattr(_binary, "__file__", str(tmp_path / "nowhere" / "_binary.py"))

    assert binary_path() == installed


def test_version_reads_the_binarys_answer(tmp_path: Path):
    assert blacksparrow.version(script(tmp_path, "echo 'blacksparrow 1.2.3-rc.1'")) == "1.2.3-rc.1"


@pytest.mark.parametrize(
    "body", ["echo hello", "exit 3", "echo 'blacksparrow 1' >&2; exit 1", "true"]
)
def test_version_rejects_an_answer_that_is_not_a_version(tmp_path: Path, body: str):
    with pytest.raises(BlackSparrowError, match="--version"):
        blacksparrow.version(script(tmp_path, body))


def test_version_of_a_missing_binary_is_an_error(tmp_path: Path):
    with pytest.raises(BinaryNotFoundError):
        blacksparrow.version(tmp_path / "missing")


def test_version_runs_with_a_minimal_environment(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("DATABASE_URL", "postgres://secret")
    binary = script(
        tmp_path, 'if [ -n "$DATABASE_URL" ]; then echo "leak"; else echo "blacksparrow 1"; fi'
    )

    assert blacksparrow.version(binary) == "1"


def test_python_m_blacksparrow_runs_the_binary(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    import subprocess

    binary = script(tmp_path, 'echo "args: $*"')
    monkeypatch.setenv("BLACKSPARROW_BINARY", str(binary))
    monkeypatch.setenv("PYTHONPATH", str(Path(blacksparrow.__file__).parent.parent))

    completed = subprocess.run(
        [sys.executable, "-m", "blacksparrow", "extract", "--help"],
        capture_output=True,
        text=True,
        check=False,
    )

    assert completed.returncode == 0 and completed.stdout.strip() == "args: extract --help"
