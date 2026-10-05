"""Test-only numeric oracle for P2-02 (not a runtime candidate).

Loads each pinned model with the reference `transformers` implementation from
the hash-checked local assets and the pinned metadata, and writes token IDs,
the attention-masked mean pool and its L2-normalized copy for fixed inputs.

    python reference_vectors.py <poc-root> <output-json> [model ...]
"""

import json
import pathlib
import shutil
import sys
import tempfile

import torch
from transformers import AutoModel, AutoTokenizer

MODELS = {
    "e5": {"max_tokens": 512, "prefix": {"query": "query: ", "passage": "passage: "}},
    "minilm": {"max_tokens": 128, "prefix": {"query": "", "passage": ""}},
}

LONG = "検索結果は現在の権限で確認してから開示する。" * 60

# Synthetic inputs only: Japanese, English, mixed, width variants, a single
# character and a text longer than both token limits.
CASES = [
    ("query", "監査 証跡"),
    ("query", "incident response"),
    ("query", "保守 承認"),
    ("passage", "保守 承認 本文 合成記録"),
    ("passage", "Search APIの検索結果をLLMとagentが同じ規則で読む。"),
    ("passage", "ＡＢＣ　テスト　ｶﾀｶﾅ"),
    ("passage", "周辺情報 7"),
    ("passage", "a"),
    ("passage", LONG),
]

# A padded batch must give each member its unpadded result.
BATCH = [("passage", "a"), ("passage", LONG), ("passage", "保守 承認 本文 合成記録")]


def materialize(root: pathlib.Path, name: str, target: pathlib.Path) -> None:
    for item in (root / "metadata" / name).rglob("*"):
        if item.is_file():
            destination = target / item.relative_to(root / "metadata" / name)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(item, destination)
    for asset in ("model.safetensors", "tokenizer.json"):
        (target / asset).symlink_to(root / "assets" / name / asset)


def encode(tokenizer, model, texts, max_tokens):
    batch = tokenizer(
        texts,
        padding=True,
        truncation=True,
        max_length=max_tokens,
        return_tensors="pt",
    )
    with torch.no_grad():
        hidden = model(**batch).last_hidden_state
    mask = batch["attention_mask"].unsqueeze(-1).to(hidden.dtype)
    raw = (hidden * mask).sum(1) / mask.sum(1)
    normalized = torch.nn.functional.normalize(raw, p=2, dim=1)
    ids = [
        [int(token) for token, keep in zip(row, keep_row) if keep]
        for row, keep_row in zip(batch["input_ids"].tolist(), batch["attention_mask"].tolist())
    ]
    return ids, raw.tolist(), normalized.tolist()


def main() -> None:
    root = pathlib.Path(sys.argv[1]).resolve()
    output = pathlib.Path(sys.argv[2])
    selected = sys.argv[3:] or list(MODELS)
    out = (
        json.loads(output.read_text())
        if output.exists()
        else {"oracle": "transformers AutoModel last_hidden_state, attention-masked mean", "models": {}}
    )
    torch.set_num_threads(1)
    for name, spec in MODELS.items():
        if name not in selected:
            continue
        with tempfile.TemporaryDirectory() as directory:
            target = pathlib.Path(directory)
            materialize(root, name, target)
            tokenizer = AutoTokenizer.from_pretrained(target)
            model = AutoModel.from_pretrained(target)
            model.eval()

            def run(cases):
                texts = [spec["prefix"][kind] + text for kind, text in cases]
                return encode(tokenizer, model, texts, spec["max_tokens"])

            singles = []
            for kind, text in CASES:
                ids, raw, normalized = run([(kind, text)])
                singles.append(
                    {"kind": kind, "text": text, "token_ids": ids[0], "raw": raw[0], "normalized": normalized[0]}
                )
            ids, raw, normalized = run(BATCH)
            batch = [
                {"kind": kind, "text": text, "token_ids": i, "raw": r, "normalized": n}
                for (kind, text), i, r, n in zip(BATCH, ids, raw, normalized)
            ]
            out["models"][name] = {
                "torch": torch.__version__,
                "transformers": __import__("transformers").__version__,
                "cases": singles,
                "batch": batch,
            }
    output.write_text(json.dumps(out, ensure_ascii=False))


if __name__ == "__main__":
    main()
