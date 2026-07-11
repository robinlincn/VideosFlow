"""VideosFlow Python sidecar 入口。

职责（MVP 阶段）：
- 暴露 /ping、/health 供 Rust 守护健康检查
- 暴露 /v1/chat、/v1/image、/v1/video、/v1/tts、/v1/test 做双网关轻量转发
- 密钥仅由请求体传入，不落盘

启动：uvicorn main:app --host 127.0.0.1 --port 8731
（Tauri 生产环境由 Rust 以 sidecar 方式拉起，端口随机以避免冲突）
"""

from __future__ import annotations

import os
import sys

# 把项目根目录插到 sys.path 最前，确保内部模块（providers / models / routers）
# 优先于 Python 3.13 同名顶层模块被导入，避免遮蔽。
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from dotenv import load_dotenv
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from routers import ping, providers

load_dotenv()

app = FastAPI(title="VideosFlow Sidecar", version="0.1.0")

# 本机回环通信，放开 CORS 方便 Rust 本地调试；生产仅限 127.0.0.1
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)

app.include_router(ping.router)
app.include_router(providers.router)


if __name__ == "__main__":
    import uvicorn

    port = int(os.getenv("SIDECAR_PORT", "8731"))
    uvicorn.run(app, host="127.0.0.1", port=port, log_level="info")
