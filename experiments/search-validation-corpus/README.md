# 検索精度・大量文書管理の検証基盤

[計画](../../docs/superpowers/programs/search-validation-corpus/plan.md)と[状態記録](../../docs/superpowers/programs/search-validation-corpus/status.md)を参照。PoC専用で、production codeではない。

## 置き場所

| 内容 | 場所 | repository |
|---|---|---|
| 原本（ダンプ、法令API応答） | `~/kp-validation-data/raw/` | 入れない |
| 検索入力・来歴・正解候補 | `~/kp-validation-data/corpus/<名前>/` | 入れない |
| 自動生成した評価質問 | `~/kp-validation-data/eval/` | 入れない（生成手順だけ） |
| LLMが書いた評価質問（未確認） | `questions/` | 入れる（短い引用だけ） |
| 実行環境（DB、原本保存、索引、秘密） | OrbStack Linux machine `kpval` の `~/kpval/` | 入れない |
| 手順と集計結果 | `scripts/`、`host/`、状態記録 | 入れる |

秘密値（DB password、合成利用者のtoken）は `~/kpval/secrets/` と `~/kpval/config/actors.json`（0600）にだけ置き、表示しない。

## 再現手順

```sh
# 1. 原本（版を固定）
curl -sfO https://dumps.wikimedia.org/jawiki/20261001/jawiki-20261001-pages-articles-multistream1.xml-p1p114794.bz2
# SHA-1 670ca0da4015e54d60221741c38d4c8fada1edf3（jawiki-20261001-sha1sums.txt と照合）

# 2. 検索入力と来歴（Mac、venvに mwparserfromhell==0.7.2）
python scripts/jawiki_corpus.py --dump <part1.bz2> --out ~/kp-validation-data/corpus/jawiki-970 --limit 970
python scripts/egov_laws.py --laws sources/laws.json --out ~/kp-validation-data/corpus/laws-20261006

# 3. Linux machine（Rust 1.98.1、PostgreSQL 18.6）でrelease build
cargo build --release --locked -p document-server -p document-semantic-inspection-worker \
  -p document-diff-worker -p search-extraction-worker
cargo build --release --locked -p search-runtime --bin search_outbox_worker
(cd host && cargo build --release)

# 4. 使い捨ての実行環境
scripts/linux_runtime.sh init     # DB、migration、bootstrap、role
scripts/linux_runtime.sh start    # document-server、Search worker、検証用Search API host

# 5. 投入（Common Document API）と監視
python3 scripts/ingest.py --corpus <corpus> --manifest ~/kpval/ingest/<name>.jsonl
scripts/watch_progress.sh 30

# 6. 評価
python scripts/build_questions.py --corpus <jawiki> <laws> --links <jawiki>/truth/links.jsonl \
  --absent-titles <titles> --out ~/kp-validation-data/eval/stage1-auto.jsonl
python3 scripts/evaluate.py --questions <auto> questions/stage1-llm.jsonl \
  --corpus <jawiki> <laws> --ingest ~/kpval/ingest/*.jsonl --out results.jsonl
```

## 出典と帰属

- 日本語版ウィキペディアの記事（2026年10月1日のダンプ）。CC BY-SA 4.0 / GFDL。著作者はウィキペディアの執筆者。リンク先・カテゴリ・テンプレート・表を除いた本文に変換して使い、本文は再配布しない。`questions/` の短い引用も同じ条件に従う。
- e-Gov法令検索（デジタル庁）の法令データ（法令API Version 2）。出典：e-Gov法令検索（https://laws.e-gov.go.jp/）。本文テキストへ変換して使う。
- 金融庁ウェブサイトの資料（公共データ利用規約 第1.0版）。初期段階では未取得（PDF経路の資格がこの環境に無いため、状態記録を参照）。
