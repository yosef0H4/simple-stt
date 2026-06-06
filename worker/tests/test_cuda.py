from types import SimpleNamespace

import pytest

from uvox_worker.cuda import inspect_cuda, require_cuda
from uvox_worker.errors import CudaRequiredError


class FakeCuda:
    def __init__(self, available: bool):
        self._available = available

    def is_available(self):
        return self._available

    def device_count(self):
        return 1 if self._available else 0

    def get_device_name(self, index):
        assert index == 0
        return "Fake RTX"

    def get_device_capability(self, index):
        assert index == 0
        return (8, 9)


def fake_torch(available: bool):
    return SimpleNamespace(
        cuda=FakeCuda(available),
        version=SimpleNamespace(cuda="12.8" if available else None),
        __version__="2.fake",
    )


def test_inspect_cuda_reports_gpu():
    info = inspect_cuda(fake_torch(True))
    assert info.available is True
    assert info.device_count == 1
    assert info.device_name == "Fake RTX"
    assert info.capability == (8, 9)


def test_require_cuda_rejects_cpu_only():
    with pytest.raises(CudaRequiredError, match="CUDA-only"):
        require_cuda(fake_torch(False))
