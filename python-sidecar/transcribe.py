#!/usr/bin/env python
# VideosFlow 本地 ASR 推理脚本：用 faster-whisper 加载本地模型权重转写音频。
# 由 Rust 侧 transcribe_local 通过子进程调用，标准输出打印 JSON：
#   {"text": "...", "segments": [{"start": float, "end": float, "text": str}], "language": str}
# 出错时打印 {"error": "..."} 并以非 0 退出码结束。
import argparse
import json
import sys


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True, help="本地模型目录（含 config.json + model.bin）")
    ap.add_argument("--audio", required=True, help="待转写音频路径")
    ap.add_argument("--language", default="zh")
    ap.add_argument("--device", default="cpu")
    ap.add_argument("--compute_type", default="int8")
    args = ap.parse_args()

    try:
        from faster_whisper import WhisperModel
    except Exception as e:
        print(json.dumps({"error": f"无法导入 faster_whisper，请先 pip install faster-whisper：{e}"}, ensure_ascii=False))
        sys.exit(1)

    try:
        model = WhisperModel(args.model, device=args.device, compute_type=args.compute_type)
    except Exception as e:
        print(json.dumps({"error": f"加载本地模型失败：{e}"}, ensure_ascii=False))
        sys.exit(1)

    try:
        segments, info = model.transcribe(args.audio, language=args.language, beam_size=5)
        out = {
            "text": "",
            "segments": [],
            "language": getattr(info, "language", args.language),
        }
        for seg in segments:
            out["segments"].append({"start": float(seg.start), "end": float(seg.end), "text": seg.text})
            out["text"] += seg.text
        print(json.dumps(out, ensure_ascii=False))
    except Exception as e:
        print(json.dumps({"error": f"转写失败：{e}"}, ensure_ascii=False))
        sys.exit(1)


if __name__ == "__main__":
    main()
