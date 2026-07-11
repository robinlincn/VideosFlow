"""Agnes 网关 Provider（LLM / 图像 / 视频）。

OpenAI 兼容风格，单 base_url 按 ModelID 路由不同能力。
连接测试：发一个 max_tokens=1 的最小 chat 请求验证鉴权与可达性。
"""

from __future__ import annotations

from typing import List

from models import ChatRequest, Envelope, ImageRequest, ProviderCfg, VideoRequest
from .base import BaseProvider


class AgnesProvider(BaseProvider):
    def capabilities(self) -> List[str]:
        return ["llm", "image", "video"]

    async def connect_test(self) -> Envelope:
        async def _call():
            if not self.cfg.api_key:
                return Envelope(ok=False, code=401, message="缺少 API Key")
            resp = await self._client.post(
                "/chat/completions",
                json={
                    "model": self.cfg.model or "agnes-2.0-flash",
                    "messages": [{"role": "user", "content": "ping"}],
                    "max_tokens": 1,
                },
            )
            return self._wrap(resp)

        return await self._safe(_call)

    async def chat(self, req: ChatRequest) -> Envelope:
        async def _call():
            resp = await self._client.post(
                "/chat/completions",
                json={
                    "model": req.model or self.cfg.model or "agnes-2.0-flash",
                    "messages": [{"role": "user", "content": req.prompt}],
                    "max_tokens": req.max_tokens,
                    "temperature": req.temperature,
                },
            )
            return self._wrap(resp)

        return await self._safe(_call)

    async def image(self, req: ImageRequest) -> Envelope:
        async def _call():
            resp = await self._client.post(
                "/images/generations",
                json={
                    "model": req.model or self.cfg.model or "agnes-image-2.1-flash",
                    "prompt": req.prompt,
                    "n": req.n,
                    "size": req.size,
                },
            )
            return self._wrap(resp)

        return await self._safe(_call)

    async def video(self, req: VideoRequest) -> Envelope:
        async def _call():
            resp = await self._client.post(
                "/videos/generations",
                json={
                    "model": req.model or self.cfg.model or "agnes-video-v2.0",
                    "prompt": req.prompt,
                },
            )
            return self._wrap(resp)

        return await self._safe(_call)
