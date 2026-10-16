#!/usr/bin/env python3
"""Embed exported Units with the pinned multilingual-e5-small on a GPU.

Reads `vector_seed export` lines ({"d": cache digest, "t": Unit text}) and
writes one little-endian float32 row per line, in input order, for
`vector_seed import`. The model contract follows search-vector-adapter:
"passage: " prefix, truncation to 512 tokens, attention-masked mean of the
last hidden state, L2 normalization. Weights are the files the worker loads
(config.json, tokenizer.json, model.safetensors), computed in float32.

Usage: embed_units_gpu.py --model DIR --units IN.jsonl --out OUT.f32
       embed_units_gpu.py --model DIR --units IN.jsonl --check REF.f32 --rows N
"""

import argparse
import json
import sys
import time

import numpy as np
import torch
from transformers import AutoModel, PreTrainedTokenizerFast

BATCH = 256


def load(model_dir):
    # XLM-R pads with <pad>, the id its position embeddings skip.
    tokenizer = PreTrainedTokenizerFast(
        tokenizer_file=f"{model_dir}/tokenizer.json", pad_token="<pad>"
    )
    model = AutoModel.from_pretrained(model_dir, torch_dtype=torch.float32).cuda().eval()
    return tokenizer, model


def embed(tokenizer, model, texts):
    out = np.zeros((len(texts), model.config.hidden_size), dtype="<f4")
    order = sorted(range(len(texts)), key=lambda i: len(texts[i]))
    with torch.inference_mode():
        for start in range(0, len(order), BATCH):
            rows = order[start:start + BATCH]
            batch = tokenizer(
                ["passage: " + texts[i] for i in rows],
                padding=True,
                truncation=True,
                max_length=512,
                return_tensors="pt",
            ).to("cuda")
            hidden = model(input_ids=batch["input_ids"], attention_mask=batch["attention_mask"]).last_hidden_state
            mask = batch["attention_mask"].unsqueeze(-1).float()
            mean = (hidden * mask).sum(1) / mask.sum(1)
            mean = torch.nn.functional.normalize(mean, dim=-1)
            out[rows] = mean.float().cpu().numpy()
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--units", required=True)
    parser.add_argument("--out")
    parser.add_argument("--check", help="compare the first --rows rows with these vectors")
    parser.add_argument("--rows", type=int, default=0)
    args = parser.parse_args()
    texts = [json.loads(line)["t"] for line in open(args.units, encoding="utf-8")]
    if args.check:
        texts = texts[: args.rows]
    tokenizer, model = load(args.model)
    started = time.time()
    vectors = embed(tokenizer, model, texts)
    elapsed = time.time() - started
    if args.check:
        reference = np.fromfile(args.check, dtype="<f4").reshape(-1, vectors.shape[1])[: len(texts)]
        cosine = (vectors * reference).sum(1)
        print(json.dumps({"rows": len(texts), "min_cosine": float(cosine.min()),
                          "mean_cosine": float(cosine.mean())}))
        return
    vectors.tofile(args.out)
    print(json.dumps({"rows": len(texts), "seconds": round(elapsed, 1),
                      "units_per_second": round(len(texts) / max(elapsed, 1e-9))}), file=sys.stderr)


if __name__ == "__main__":
    main()
