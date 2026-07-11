"""XiaomiMimo 网关 Provider（TTS）。

base_url: https://api.xiaomimimo.com/v1
Model   : mimo-v2.5-tts
OpenAI 兼容音频接口（/audio/speech），返回音频字节流。
"""

from __future__ import annotations

from typing import List

from models import Envelope, ProviderCfg, TtsRequest
from .base import BaseProvider


class MimoProvider(BaseProvider):
    def capabilities(self) -> List[str]:
        return ["tts"]

    async def connect_test(self) -> Envelope:
        async def _call():
            if not self.cfg.api_key:
                return Envelope(ok=False, code=401, message="缺少 API Key")
            resp = await self._client.post(
                "/audio/speech",
                json={
                    "model": self.cfg.model or "mimo-v2.5-tts",
                    "input": "ping",
                    "voice": "default",
                },
            )
            return self._wrap(resp)

        return await self._safe(_call)

    async def tts(self, req: TtsRequest) -> Envelope:
        async def _call():
            resp = await self._client.post(
                "/audio/speech",
                json={
                    "model": req.model or self.cfg.model or "mimo-v2.5-tts",
                    "input": req.text,
                    "voice": req.voice,
                },
            )
            env = self._wrap(resp)
            if env.ok and resp.content:
                # 前端按 data 拿到 base64 音频；这里直接透传字节长度信息
                env.data = {"bytes": len(resp.content), "content_type": resp.headers.get("content-type", "audio/mpeg")}
            return env

        return await self._safe(_call)
