"""CUDA validation. The production worker intentionally has no CPU fallback."""

from __future__ import annotations

from dataclasses import asdict, dataclass
from typing import Any

from .errors import CudaRequiredError


@dataclass(frozen=True)
class CudaInfo:
    available: bool
    torch_version: str
    torch_cuda_version: str | None
    device_count: int
    device_name: str | None
    capability: tuple[int, int] | None

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


def inspect_cuda(torch_module: Any | None = None) -> CudaInfo:
    """Return CUDA details without allowing an implicit CPU fallback."""
    if torch_module is None:
        try:
            import torch as torch_module  # type: ignore[no-redef]
        except ImportError as exc:
            raise CudaRequiredError(
                "PyTorch is not installed. Run scripts/setup-worker.ps1 first."
            ) from exc

    available = bool(torch_module.cuda.is_available())
    count = int(torch_module.cuda.device_count()) if available else 0
    name = str(torch_module.cuda.get_device_name(0)) if count else None
    capability = tuple(torch_module.cuda.get_device_capability(0)) if count else None
    return CudaInfo(
        available=available,
        torch_version=str(torch_module.__version__),
        torch_cuda_version=getattr(torch_module.version, "cuda", None),
        device_count=count,
        device_name=name,
        capability=capability,  # type: ignore[arg-type]
    )


def require_cuda(torch_module: Any | None = None) -> CudaInfo:
    """Reject machines where PyTorch cannot access an NVIDIA CUDA GPU."""
    info = inspect_cuda(torch_module)
    if not info.available or info.device_count < 1:
        raise CudaRequiredError(
            "Uvox is CUDA-only: torch.cuda.is_available() returned False. "
            "Install a supported NVIDIA driver and a CUDA-enabled PyTorch wheel, then rerun "
            "`uvox-worker doctor`. CPU fallback is intentionally disabled."
        )
    return info
