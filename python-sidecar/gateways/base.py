"""Provider 抽象基类。

所有网关实现统一继承此类，对外暴露 connect_test() 与按能力的方法。
sidecar 用 httpx.AsyncClient 转发 OpenAI 兼容请求。
"""

from __future__ import annotations

from abc import ABC, abstractmethod

import httpx

from models import AsrRequest, Envelope, ProviderCfg


class BaseProvider(ABC):
    def __init__(self, cfg: ProviderCfg) -> None:
        self.cfg = cfg
        headers = {"Content-Type": "application/json"}
        if cfg.api_key:
            headers["Authorization"] = f"Bearer {cfg.api_key}"
        self._client = httpx.AsyncClient(
            base_url=cfg.base_url.rstrip("/"),
            timeout=httpx.Timeout(30.0, read=120.0),
            headers=headers,
        )

    async def close(self) -> None:
        await self._client.aclose()

    def _wrap(self, resp: httpx.Response) -> Envelope:
        ok = 200 <= resp.status_code < 300
        data = None
        if ok:
            try:
                data = resp.json()
            except Exception:
                data = resp.text
        return Envelope(
            ok=ok,
            code=resp.status_code,
            message="" if ok else (resp.text or "")[:300],
            data=data,
        )

    async def _safe(self, fn) -> Envelope:
        """统一捕获网络/异常，避免 sidecar 直接 500。"""
        try:
            return await fn()
        except httpx.HTTPStatusError as e:
            return Envelope(ok=False, code=e.response.status_code, message=str(e)[:300])
        except Exception as e:  # noqa: BLE001
            return Envelope(ok=False, code=-1, message=str(e)[:300])

    @abstractmethod
    async def connect_test(self) -> Envelope:
        ...

    @abstractmethod
    def capabilities(self) -> List[str]:
        ...

    async def asr(self, req: AsrRequest) -> Envelope:
        """语音识别：默认未实现（占位）。子类（如 Agnes）按需覆盖。

        M2 按冻结决策 Q1：默认返回清晰的「未就绪 / 降级」信封，
        由 Rust 端 film_import 任务捕获后降级处理，绝不阻塞导入→对齐链路。
        """
        raise NotImplementedError("该网关不支持 ASR")
