"""Agnes 网关 Provider（LLM / 图像 / 视频 / ASR）。

OpenAI 兼容风格，单 base_url 按 ModelID 路由不同能力。
连接测试：发一个 max_tokens=1 的最小 chat 请求验证鉴权与可达性。

ASR（M2）：按冻结决策 Q1 采用「占位端点 + 降级」策略。
- 默认返回清晰的「未就绪 / 降级」信封（agnes-asr-1.0 尚未验证真实可达）。
- 当环境变量 VF_ASR_REAL=1 且网关真实支持 whisper 兼容 /audio/transcriptions 时，
  走真实转发（代码已就绪，仅需确认 ModelID 后开启）。
无论哪种情况，Rust 端 film_import 均按 ok 判定降级，绝不阻塞后续流程。
"""

from __future__ import annotations

import os
from typing import List

from models import AsrRequest, ChatRequest, Envelope, ImageRequest, ProviderCfg, VideoRequest
from .base import BaseProvider


class AgnesProvider(BaseProvider):
    def capabilities(self) -> List[str]:
        return ["llm", "image", "video", "asr"]

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

    async def asr(self, req: AsrRequest) -> Envelope:
        # 冻结决策 Q1：默认占位端点，返回清晰的降级信封。
        if os.getenv("VF_ASR_REAL") != "1":
            return Envelope(
                ok=False,
                code=501,
                message="ASR 未就绪：占位端点（agnes-asr-1.0 尚未验证真实可达，真实 ASR 留待 M1.5 补齐）",
                data=None,
            )

        # 真实转发路径（whisper 兼容 /audio/transcriptions），仅当 VF_ASR_REAL=1 时启用。
        async def _call():
            audio_path = req.audio_path
            if not os.path.exists(audio_path):
                return Envelope(ok=False, code=404, message=f"音频文件不存在: {audio_path}")
            with open(audio_path, "rb") as f:
                resp = await self._client.post(
                    "/audio/transcriptions",
                    files={"file": (os.path.basename(audio_path), f, "audio/wav")},
                    data={
                        "model": self.cfg.model or "agnes-asr-1.0",
                        "language": req.language or "zh",
                        "response_format": "verbose_json",
                    },
                )
            if not 200 <= resp.status_code < 300:
                return self._wrap(resp)
            raw = resp.json()
            segs = []
            for s in raw.get("segments", []):
                segs.append(
                    {
                        "start": float(s.get("start", 0.0)),
                        "end": float(s.get("end", 0.0)),
                        "text": s.get("text", ""),
                        "confidence": float(s.get("avg_logprob", 0.0)),
                    }
                )
            if not segs and raw.get("text"):
                segs.append(
                    {
                        "start": 0.0,
                        "end": float(raw.get("duration", 0.0)),
                        "text": raw.get("text", ""),
                        "confidence": 1.0,
                    }
                )
            data = {
                "segments": segs,
                "language": raw.get("language", req.language or "zh"),
                "duration": float(raw.get("duration", 0.0)),
            }
            return Envelope(ok=True, code=200, message="ok", data=data)

        return await self._safe(_call)
