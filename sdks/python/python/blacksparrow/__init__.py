"""Black Sparrow: read a business website into one typed document.

``extract`` runs the bundled ``blacksparrow`` binary and returns a ``SiteExtract``: the homepage
and the few pages that matter as Markdown with their links and contacts, the site's colours,
fonts and logo candidates, and schema.org business facts. The binary ships inside the platform
wheel, so nothing is built or downloaded at install or run time.

>>> import blacksparrow
>>> site = blacksparrow.extract(
...     "https://example.com", chrome_path=None, render="never", max_pages=6, timeout_seconds=60
... )
>>> site.outcome
'ok'
"""

from __future__ import annotations

from importlib import metadata

from ._api import extract, extract_async, extract_raw, extract_raw_async
from ._binary import ENVIRONMENT_VARIABLE, binary_path, version
from ._errors import (
    BinaryNotFoundError,
    BlackSparrowCancelled,
    BlackSparrowError,
    BlackSparrowTimeout,
)
from ._models import (
    SCHEMA_VERSION,
    Color,
    ColorEvidence,
    Contacts,
    Design,
    Font,
    Logo,
    Outcome,
    Page,
    PageError,
    PageLink,
    SiteExtract,
    Stats,
    Structured,
    Stylesheet,
)
from ._process import RenderMode

try:
    __version__ = metadata.version("blacksparrow")
except metadata.PackageNotFoundError:  # running from a source tree that is not installed
    __version__ = "0+unknown"

__all__ = [
    "ENVIRONMENT_VARIABLE",
    "SCHEMA_VERSION",
    "BinaryNotFoundError",
    "BlackSparrowCancelled",
    "BlackSparrowError",
    "BlackSparrowTimeout",
    "Color",
    "ColorEvidence",
    "Contacts",
    "Design",
    "Font",
    "Logo",
    "Outcome",
    "Page",
    "PageError",
    "PageLink",
    "RenderMode",
    "SiteExtract",
    "Stats",
    "Structured",
    "Stylesheet",
    "__version__",
    "binary_path",
    "extract",
    "extract_async",
    "extract_raw",
    "extract_raw_async",
    "version",
]
