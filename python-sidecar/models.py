"""VideosFlow Python sidecar — 统一数据模型与信封。

设计要点：
- ProviderCfg 字段与前端 src/data/mock.ts 的 ProviderCfg 完全对齐
  （name / provider / baseUrl / apiKey / model / enabled / test），
  保证前后端不出现字段错位。
- Envelope 是所有接口的统一返回信封，前端 IPC 层按 ok/code/message/data 解包。
- 密钥策略：api_key 仅由调用方（Rust/前端）在请求体里传入，sidecar 不落盘、不缓存到文件。

导入说明：本项目以可安装包形式运行（pyproject.toml 声明为 python_sidecar），
所有内部引用均使用绝对导入 python_sidecar.xxx，避免与 Python 3.13 同名顶层
模块（如标准库/第三方 providers）冲突。
"""

from __future__ import annotations

from typing import Any, Literal, Optional

from pydantic import BaseModel


class ProviderCfg(BaseModel):
    """单个能力网关配置。与前端 ProviderCfg 对齐。"""

    name: str = ""
    provider: str = ""            # 逻辑标识：agnes / mimo / openai ...
    base_url: str = ""            # 网关 base_url
    api_key: str = ""             # 运行时由调用方传入，sidecar 不持久化
    model: str = ""
    enabled: bool = True
    test: str = "idle"            # idle / ok / fail / local


class Envelope(BaseModel):
    """统一返回信封。"""

    ok: bool = True
    code: int = 0
    message: str = ""
    data: Any = None


class ChatRequest(BaseModel):
    prompt: str
    model: Optional[str] = None
    max_tokens: int = 512
    temperature: float = 0.7


class ImageRequest(BaseModel):
    prompt: str
    model: Optional[str] = None
    n: int = 1
    size: str = "1024x1024"


class VideoRequest(BaseModel):
    prompt: str
    model: Optional[str] = None


class TtsRequest(BaseModel):
    text: str
    model: Optional[str] = None
    voice: str = "default"


# 能力维度
Capability = Literal["llm", "image", "video", "tts", "asr"]
