from uvox_worker.constants import DEFAULT_STABILITY_OBSERVATIONS
from uvox_worker.stabilizer import PrefixStabilizer, complete_text_boundary, longest_common_prefix


def test_longest_common_prefix():
    assert longest_common_prefix(["hello world", "hello worm", "hello work"]) == "hello wor"


def test_complete_boundary_does_not_commit_partial_word():
    assert complete_text_boundary("hello wor") == "hello "
    assert complete_text_boundary("hello ") == "hello "
    assert complete_text_boundary("hello") == ""


def test_stabilizer_requires_repeated_observations_and_emits_delta_only():
    stabilizer = PrefixStabilizer(required_observations=3)
    assert stabilizer.observe("hello wor").commit_delta == ""
    assert stabilizer.observe("hello world ").commit_delta == ""
    assert stabilizer.observe("hello world from ").commit_delta == "hello "
    update = stabilizer.observe("hello world from the ")
    assert update.commit_delta == "world "
    assert update.committed == "hello world "


def test_default_stabilizer_commits_complete_words_quickly():
    stabilizer = PrefixStabilizer(required_observations=DEFAULT_STABILITY_OBSERVATIONS)
    assert stabilizer.observe("hello ").commit_delta == "hello "


def test_force_commit_is_explicit():
    stabilizer = PrefixStabilizer(required_observations=3)
    stabilizer.observe("hello ")
    update = stabilizer.force_commit("hello world")
    assert update.commit_delta == "hello world"
