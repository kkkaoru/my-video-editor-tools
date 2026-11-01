"""The checkout root, from a snapshot too."""

from pathlib import Path

import pytest

from replay import repository
from replay.core import ReplayError

CHECKOUT = Path(__file__).resolve().parents[1]


def test_without_the_variable_the_root_is_the_checkout_holding_the_package(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv("REPLAY_REPOSITORY_ROOT", raising=False)
    assert repository.root() == CHECKOUT


def test_the_variable_names_the_checkout_for_a_snapshotted_step(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    (tmp_path / "checkout/src/replay").mkdir(parents=True)
    monkeypatch.setenv("REPLAY_REPOSITORY_ROOT", str(tmp_path / "checkout"))
    assert repository.root() == (tmp_path / "checkout").resolve()


def test_a_relative_root_is_refused(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("REPLAY_REPOSITORY_ROOT", "checkout")
    with pytest.raises(ReplayError, match="must name a checkout holding src/replay"):
        repository.root()


def test_a_directory_without_the_package_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("REPLAY_REPOSITORY_ROOT", str(tmp_path))
    with pytest.raises(ReplayError, match="must name a checkout holding src/replay"):
        repository.root()
