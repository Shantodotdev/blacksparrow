"""The errors of the package."""

from __future__ import annotations


class BlackSparrowError(Exception):
    """Black Sparrow did not produce a usable result.

    Raised when the binary cannot start, crashes, times out, is cancelled, prints too much or
    prints a document this package cannot use. A site that cannot be read is not this error:
    that is a normal result whose ``outcome`` says why.
    """


class BinaryNotFoundError(BlackSparrowError):
    """The ``blacksparrow`` binary was not found, or the path given for it is not usable."""


class BlackSparrowTimeout(BlackSparrowError):
    """The binary did not finish within ``timeout_seconds`` plus a short grace period."""


class BlackSparrowCancelled(BlackSparrowError):
    """The reading was cancelled through the ``cancel`` event."""
