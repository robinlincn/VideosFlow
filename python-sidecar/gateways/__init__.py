"""Provider 抽象与路由。

MVP 纯云：sidecar 只做轻量转发，不内置 torch/whisper，体积小。
双网关：
- Agnes  : LLM / 图像 / 视频（agnes-2.0-flash / agnes-image-2.1-flash / agnes-video-v2.0）
- Mimo   : TTS（mimo-v2.5-tts）

build_provider() 按 provider 标识或 base_url 关键字把配置路由到对应实现。
"""

from __future__ import annotations

from typing import List

from models import ProviderCfg, Envelope

from .base import BaseProvider
from .agnes import AgnesProvider
from .mimo import MimoProvider

__all__ = ["BaseProvider", "AgnesProvider", "MimoProvider", "build_provider"]


def build_provider(cfg: ProviderCfg) -> BaseProvider:
    """根据逻辑标识或 base_url 关键字选择实现。"""

    p = (cfg.provider or "").lower()
    url = (cfg.base_url or "").lower()
    if "mimo" in p or "xiaomi" in p or "xiaomimimo" in url:
        return MimoProvider(cfg)
    # 默认走 Agnes（OpenAI 兼容网关统一路由 LLM/图像/视频）
    return AgnesProvider(cfg)
