#!/usr/bin/env python3
"""Fine-tune the EmbeddingGemma 2 text tower for Japanese retrieval.

Training data: cl-nagoya/ruri-v3-dataset-ft (query, positive, hard negatives;
JQaRA is excluded as its card marks it unused). Loss: cached in-batch
negatives with one hard negative per query, plus a retention term (MSE to the
frozen original model on sampled documents; its share of batches follows the
sample count) that keeps document embeddings close so the shared space
with the image and audio encoders does not drift far. Only the text tower is
loaded and trained; the vision and audio encoders are untouched.

Usage: eg2_finetune.py --out DIR [--init MODEL_OR_DIR] [--max-per-config N]
                       [--epochs 1] [--retention-samples 50000]
`--init` continues from an earlier checkpoint (e.g. after extra pre-training).
"""

import argparse
import random

import torch
from datasets import Dataset, concatenate_datasets, load_dataset
from sentence_transformers import (SentenceTransformer, SentenceTransformerTrainer,
                                   SentenceTransformerTrainingArguments, losses)
from sentence_transformers.training_args import BatchSamplers

CONFIGS = ["auto-wiki-qa-nemotron", "jaquad", "jsquad", "miracl", "mkqa", "mr-tydi", "nli",
           "quiz-no-mori", "quiz-works"]
QUERY = "task: search result | query: "
DOCUMENT = "title: none | text: "


def load(model_id: str):
    return SentenceTransformer(
        model_id, device="cuda", model_kwargs={"torch_dtype": torch.bfloat16},
        config_kwargs={"vision_config": None, "audio_config": None})


def triplets(max_per_config: int, seed: int) -> Dataset:
    parts = []
    for name in CONFIGS:
        data = load_dataset("cl-nagoya/ruri-v3-dataset-ft", name, split="train")
        if max_per_config and len(data) > max_per_config:
            data = data.shuffle(seed=seed).select(range(max_per_config))
        data = data.filter(lambda row: bool(row["neg"]))
        data = data.map(lambda row: {"anchor": QUERY + row["anc"], "positive": DOCUMENT + row["pos"],
                                     "negative": DOCUMENT + row["neg"][0]},
                        remove_columns=data.column_names)
        parts.append(data)
    return concatenate_datasets(parts).shuffle(seed=seed)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True)
    parser.add_argument("--init", default="google/embeddinggemma-2")
    parser.add_argument("--max-per-config", type=int, default=0)
    parser.add_argument("--epochs", type=float, default=1.0)
    parser.add_argument("--retention-samples", type=int, default=50_000)
    parser.add_argument("--batch", type=int, default=512)
    parser.add_argument("--lr", type=float, default=2e-5)
    parser.add_argument("--seed", type=int, default=42)
    args = parser.parse_args()
    random.seed(args.seed)

    train = triplets(args.max_per_config, args.seed)
    print(f"triplets={len(train)}", flush=True)

    # Retention targets: the frozen original model's embeddings of training documents.
    teacher = load("google/embeddinggemma-2")
    sample = train.shuffle(seed=args.seed + 1).select(range(min(args.retention_samples, len(train))))
    texts = list(sample["positive"])
    targets = teacher.encode(texts, batch_size=256, normalize_embeddings=True, convert_to_numpy=True)
    retention = Dataset.from_dict({"text": texts, "label": [t.tolist() for t in targets]})
    del teacher
    torch.cuda.empty_cache()

    model = load(args.init)
    train_losses = {
        "retrieval": losses.CachedMultipleNegativesRankingLoss(model, mini_batch_size=32),
        "retention": losses.MSELoss(model),
    }
    training_args = SentenceTransformerTrainingArguments(
        output_dir=args.out, num_train_epochs=args.epochs, per_device_train_batch_size=args.batch,
        learning_rate=args.lr, warmup_ratio=0.05, bf16=True, logging_steps=20, save_steps=500,
        save_total_limit=3, batch_sampler=BatchSamplers.NO_DUPLICATES, seed=args.seed,
        multi_dataset_batch_sampler="proportional", report_to=[])
    trainer = SentenceTransformerTrainer(
        model=model, args=training_args,
        train_dataset={"retrieval": train, "retention": retention}, loss=train_losses)
    trainer.train()
    model.save(f"{args.out}/final")
    print("saved", flush=True)


main()
