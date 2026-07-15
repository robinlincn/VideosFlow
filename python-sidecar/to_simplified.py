#!/usr/bin/env python3
# 繁体中文 -> 简体中文转换（供 Rust 端调用，对 LLM/ASR 最终文案做兜底）。
# 优先用 opencc；未安装时原样返回（不阻塞主流程）。
import sys


def main() -> None:
    data = sys.stdin.read()
    out = data
    try:
        from opencc import OpenCC

        cc = OpenCC("t2s")
        out = cc.convert(data)
    except Exception:
        # opencc 不可用：原样返回，由上层提示词约束简体
        out = data
    sys.stdout.write(out)


if __name__ == "__main__":
    main()
