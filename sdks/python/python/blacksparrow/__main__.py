"""``python -m blacksparrow ...`` runs the bundled binary with the given arguments."""

from __future__ import annotations

import subprocess
import sys

from ._binary import binary_path
from ._errors import BlackSparrowError


def main() -> int:
    """Run the binary with the command-line arguments and return its exit status."""
    try:
        executable = binary_path()
    except BlackSparrowError as error:
        print(f"blacksparrow: {error}", file=sys.stderr)
        return 2
    try:
        return subprocess.call([str(executable), *sys.argv[1:]])  # noqa: S603
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())
