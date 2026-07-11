"""健康检查与存活探针。Rust 侧用它做 sidecar 守护健康检查。"""

from __future__ import annotations

from fastapi import APIRouter

router = APIRouter()


@router.get("/ping")
async def ping():
    return {"ok": True, "service": "videosflow-sidecar", "version": "0.1.0"}


@router.get("/health")
async def health():
    return {"ok": True, "status": "healthy"}
