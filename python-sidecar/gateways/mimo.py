"""XiaomiMimo 网关 Provider（TTS）。

base_url: https://api.xiaomimimo.com/v1
Model   : mimo-v2.5-tts
走 Chat Completions 协议（/chat/completions）：文本放 messages 里 role=assistant 的
content，audio 对象声明格式/音色，响应在 choices[0].message.audio.data（base64 音频）。
注意：MiMo 没有 OpenAI 风格的 /audio/speech（返回 404），必须用本协议。
"""

from __future__ import annotations

import base64
from typing import List

from models import Envelope, ProviderCfg, TtsRequest
from .base import BaseProvider


class MimoProvider(BaseProvider):
    def capabilities(self) -> List[str]:
        return ["tts"]

    def _tts_body(self, text: str, voice: str | None) -> dict:
        v = voice or "mimo_default"
        return {
            "model": self.cfg.model or "mimo-v2.5-tts",
            "messages": [{"role": "assistant", "content": text}],
            "audio": {"format": "wav", "voice": v},
            "stream": False,
        }

    async def connect_test(self) -> Envelope:
        async def _call():
            if not self.cfg.api_key:
                return Envelope(ok=False, code=401, message="缺少 API Key")
            resp = await self._client.post("/chat/completions", json=self._tts_body("ping", "mimo_default"))
            env = self._wrap(resp)
            if env.ok and isinstance(env.data, dict):
                aud = env.data.get("choices", [{}])[0].get("message", {}).get("audio", {})
                if aud.get("data"):
                    env.data = {"bytes": len(base64.b64decode(aud["data"])), "content_type": "audio/wav"}
                else:
                    env.ok = False
                    env.message = "TTS 返回缺少 audio.data"
            return env

        return await self._safe(_call)

    async def tts(self, req: TtsRequest) -> Envelope:
        async def _call():
            resp = await self._client.post(
                "/chat/completions",
                json=self._tts_body(req.text, getattr(req, "voice", None) or req.voice),
            )
            env = self._wrap(resp)
            if env.ok and isinstance(env.data, dict):
                aud = env.data.get("choices", [{}])[0].get("message", {}).get("audio", {})
                if aud.get("data"):
                    env.data = {"bytes": len(base64.b64decode(aud["data"])), "content_type": "audio/wav"}
                else:
                    env.ok = False
                    env.message = "TTS 返回缺少 audio.data"
            return env

        return await self._safe(_call)
