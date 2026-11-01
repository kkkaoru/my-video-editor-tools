"""The checkout this code belongs to, also when it runs from a code snapshot.

A snapshotted step imports a read-only copy of ``replay`` under ``edits/work/code-snapshots``,
where ``Path(__file__).parents[2]`` is not the checkout. The pipeline then names the checkout in
``REPLAY_REPOSITORY_ROOT``; without it, the root is derived from this file as before.
"""

import os
from pathlib import Path

from replay.core import ReplayError

VARIABLE = "REPLAY_REPOSITORY_ROOT"


def root() -> Path:
    """The checkout: ``REPLAY_REPOSITORY_ROOT`` when set, else two levels above the package."""
    value = os.environ.get(VARIABLE)
    if not value:
        return Path(__file__).resolve().parents[2]
    path = Path(value)
    if not path.is_absolute() or not (path / "src" / "replay").is_dir():
        raise ReplayError(f"{VARIABLE} must name a checkout holding src/replay")
    return path.resolve()
