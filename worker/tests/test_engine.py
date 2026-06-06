from types import SimpleNamespace

from uvox_worker.engine import LiveEngine


class FakeRecognizer:
    def __init__(self):
        self.config = SimpleNamespace(chunk_samples=2)
        self.reset_count = 0
        self.outputs = iter(["hello ", "hello world ", "hello world from ", "hello world from rust "])

    def reset(self):
        self.reset_count += 1

    def transcribe_chunk(self, pcm16_bytes):
        assert len(pcm16_bytes) == 4
        return next(self.outputs)


def messages_of_type(events, kind):
    return [event for event in events if event["type"] == kind]


def test_engine_ignores_wrong_session_and_cancels_tail():
    events = []
    recognizer = FakeRecognizer()
    engine = LiveEngine(recognizer, events.append, stability_observations=2)
    engine.start(7)
    engine.push_pcm16(99, b"\0" * 8)
    assert not messages_of_type(events, "partial")

    engine.push_pcm16(7, b"\0" * 4)
    engine.push_pcm16(7, b"\0" * 4)
    commits = messages_of_type(events, "commit")
    assert commits[-1]["text"] == "hello "

    engine.cancel(7)
    engine.push_pcm16(7, b"\0" * 4)
    assert messages_of_type(events, "session_cancelled")[-1]["session_id"] == 7
    assert len(messages_of_type(events, "partial")) == 2
