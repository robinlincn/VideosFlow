"""能力转发路由。

请求体统一为 { cfg: ProviderCfg, req: XxxRequest } 单 JSON body：
- cfg 携带网关配置（含运行时传入的 api_key），sidecar 不持久化
- req 为具体能力参数
由 build_provider(cfg) 路由到 Agnes / Mimo 实现。
"""

from __future__ import annotations

from fastapi import APIRouter
from pydantic import BaseModel

from models import (
    AsrRequest,
    ChatRequest,
    Envelope,
    ImageRequest,
    ProviderCfg,
    TtsRequest,
    VideoRequest,
)
from gateways import build_provider

router = APIRouter(prefix="/v1")


class ChatCall(BaseModel):
    cfg: ProviderCfg
    req: ChatRequest


class ImageCall(BaseModel):
    cfg: ProviderCfg
    req: ImageRequest


class VideoCall(BaseModel):
    cfg: ProviderCfg
    req: VideoRequest


class TtsCall(BaseModel):
    cfg: ProviderCfg
    req: TtsRequest


class AsrCall(BaseModel):
    cfg: ProviderCfg
    req: AsrRequest


class TestCall(BaseModel):
    cfg: ProviderCfg


@router.post("/chat", response_model=Envelope)
async def chat(call: ChatCall) -> Envelope:
    p = build_provider(call.cfg)
    try:
        return await p.chat(call.req)
    finally:
        await p.close()


@router.post("/image", response_model=Envelope)
async def image(call: ImageCall) -> Envelope:
    p = build_provider(call.cfg)
    try:
        return await p.image(call.req)
    finally:
        await p.close()


@router.post("/video", response_model=Envelope)
async def video(call: VideoCall) -> Envelope:
    p = build_provider(call.cfg)
    try:
        return await p.video(call.req)
    finally:
        await p.close()


@router.post("/tts", response_model=Envelope)
async def tts(call: TtsCall) -> Envelope:
    p = build_provider(call.cfg)
    try:
        return await p.tts(call.req)
    finally:
        await p.close()


@router.post("/asr", response_model=Envelope)
async def asr(call: AsrCall) -> Envelope:
    """语音识别（M2 新增）。默认占位端点，返回清晰降级信封；真实转发由 VF_ASR_REAL=1 开启。"""
    p = build_provider(call.cfg)
    try:
        return await p.asr(call.req)
    finally:
        await p.close()


@router.post("/test", response_model=Envelope)
async def test(call: TestCall) -> Envelope:
    """连接测试：验证 base_url 可达 + api_key 鉴权通过。"""
    p = build_provider(call.cfg)
    try:
        return await p.connect_test()
    finally:
        await p.close()
