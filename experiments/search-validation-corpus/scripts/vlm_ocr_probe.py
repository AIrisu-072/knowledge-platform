#!/usr/bin/env python3
"""Probe page OCR by a vision-language model behind an OpenAI-compatible API.

Sends one rendered page image, asks for the page text in reading order, and
compares it with a reference text (character error rate over the text with
whitespace removed). The reference is a human-checked transcription or, for
pages whose native text is in reading order, the native text.

Usage: vlm_ocr_probe.py --endpoint URL --model NAME --image PAGE.png
                        [--reference REF.txt] --out RESULT.json
"""

import argparse
import base64
import json
import re
import time
import urllib.request
from pathlib import Path

PROMPT = (
    "この画像は日本の官公庁文書の1ページです。ページに書かれている文字を、読む順番どおりにすべて書き起こしてください。"
    "縦書きは右の列から左の列へ読みます。表は行ごとに、セルを「|」で区切ってください。"
    "新旧対照表は「改正後」と「改正前」を分けて書き、傍線が付いた部分は【】で囲んでください。"
    "読めない文字は推測せず〓にしてください。説明や要約は書かず、書き起こしだけを出力してください。"
)


def distance(a: str, b: str) -> int:
    previous = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        current = [i]
        for j, cb in enumerate(b, 1):
            current.append(min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + (ca != cb)))
        previous = current
    return previous[-1]


def normalize(text: str) -> str:
    return re.sub(r"[\s|【】]", "", text)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--image", required=True, type=Path)
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--max-tokens", type=int, default=8192)
    parser.add_argument("--no-thinking", action="store_true", help="disable the reasoning phase (Qwen chat template)")
    args = parser.parse_args()
    image = base64.b64encode(args.image.read_bytes()).decode()
    payload = {"model": args.model, "max_tokens": args.max_tokens, "temperature": 0,
               "messages": [{"role": "user", "content": [
                   {"type": "image_url", "image_url": {"url": f"data:image/png;base64,{image}"}},
                   {"type": "text", "text": PROMPT}]}]}
    if args.no_thinking:
        payload["chat_template_kwargs"] = {"enable_thinking": False}
    request = urllib.request.Request(f"{args.endpoint}/v1/chat/completions", data=json.dumps(payload).encode(),
                                     method="POST", headers={"Content-Type": "application/json"})
    started = time.perf_counter()
    with urllib.request.urlopen(request, timeout=900) as response:
        body = json.loads(response.read())
    seconds = round(time.perf_counter() - started, 1)
    text = body["choices"][0]["message"].get("content") or ""
    result = {"model": args.model, "image": args.image.name, "thinking": not args.no_thinking, "seconds": seconds, "usage": body.get("usage"),
              "chars": len(normalize(text)), "text": text}
    if args.reference:
        reference = normalize(args.reference.read_text())
        result["reference_chars"] = len(reference)
        result["cer"] = round(distance(normalize(text), reference) / max(1, len(reference)), 4)
    args.out.write_text(json.dumps(result, ensure_ascii=False, indent=1))
    print(json.dumps({k: v for k, v in result.items() if k != "text"}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
