# 検索世代の部品化 — 実装計画

2026-10-07。設計：[検索世代の持ち方](../specs/2026-10-07-search-generation-segments-design.md)（所有者承認 2026-10-07）。仕様：`spec/data/transaction-consistency-requirements-v0.md` SD-T11 5、`spec/data/logical-data-model-v0.md` P1 bundle の追記。branch `feat/search-generation-segments-20261007`（#85 の上）。

## 所有者の判断（2026-10-07）

1. 部品は書込み時に一度だけ全体を検証し、検証 receipt を付ける。切替・pin では一覧と新しい部品だけを再計算する。既存の部品は各プロセスが初めて読むときに再計算する。
2. 部品の単位は親 Version × Part × profile/parser build（今の `BodyItemEntry` 1件）。
3. 字句索引のsegment統合の条件は、実装中に計測してから案を出す。

## 段1：Unit部品と一覧、読込みの差分化

### T1 digest v2（`search-source-document`）

- `BodyItemEntry` 1件の canonical encoding から部品の digest（domain `document-body-segment:v1`）を作る `segment_digest(entry)`。
- `unit_manifest_receipt` を v2 にする：digest = SHA-256(domain `document-unit-manifest:v2` ‖ 部品数 ‖ 部品 digest を順に)。count は今と同じ Unit 数。
- coverage receipt も同じ形（部品ごとの coverage digest の一覧）にする。
- composite の domain を `document-generation-bundle:v2` にする。v1 の receipt を持つ世代は読込みで拒否し、移行時に全件の作り直しを1回行う（`linux_runtime.sh rebuild` と同じ経路）。
- 試験：同じ入力の full 構築と、1件だけ変えた差分構築で digest が一致する。1件の本文を変えると、その部品の digest と一覧の digest だけが変わる。

### T2 migration 0011（`search-runtime`）

- `search_unit_segment(segment_digest PK, dto_version, payload JSONB, unit_count, coverage_digest, verified_build, created_at)`：内容で決まる鍵、更新禁止（trigger）。
- `search_generation_segment(source_id, generation_id, ordinal, segment_digest FK, PK(source_id, generation_id, ordinal))`：今の generation child と同じ guard（BUILDING の親と生きている guard のときだけ書込み、DELETING の親と GC role のときだけ削除）。
- 1部品の JSON が 256 MiB を超える場合は今の chunk 方式を部品に適用する。

### T3 payload の保存と復元（`payload.rs`）

- `unit_manifest` を部品ごとに `INSERT … ON CONFLICT (segment_digest) DO NOTHING` で保存し、一覧を `search_generation_segment` に書く。既にある部品は、保存済み行の digest と件数の一致だけを確かめる（判断1）。
- 新しく書いた部品は、書込み前に全 field と本文を検証し、`verified_build` を記録する。
- `restore` は一覧を読み、部品を digest ごとのプロセス内キャッシュから取る。キャッシュに無い部品は DB から読み、その場で部品の digest を再計算して照合する（遅延検証）。不一致は `BundleError::Digest` とし、その世代の本文評価を止める。
- `projection` と `body_coverage` の行は今のまま（大きさを計測し、必要なら段1の後に部品化する）。

### T4 読込み（`durable_read.rs`）

- 世代の切替では、一覧の digest と件数を再計算し、新しい部品だけを読む。
- 部品のキャッシュは世代をまたいで共有し、件数か bytes の上限で古いものを捨てる（設定項目を追加）。
- `vector_units` の対応表も部品単位で作り、共有する。

### T5 GC（`gc.rs`）

- 世代の削除時に `search_generation_segment` の行を消す。どの非 DELETING 世代からも参照されない部品を、同じ GC の実行で消す。

### T6 検証

- 単体：T1 の digest の同一性、T3 の遅延検証で改ざんを見つけること、T5 で参照中の部品を消さないこと。
- DB 結合（`search-runtime/tests`）：full 構築 → 1件更新 → 部品の行が1件増え、一覧が差し替わり、復元した manifest が full 構築のものと一致する。
- 実環境（kpval、1,000文書）：1件の更新の反映時間、worker のメモリ、1世代あたりの DB 増分、新しい世代の最初の検索の時間を、変更前と比べる。

## 段2：字句索引の差分化（概要）

- 世代のディレクトリを、変わらない segment ファイルのハードリンクと、変わった文書の新しい segment と削除の印で作る。
- seal はファイルごとの digest の一覧から作り、ハードリンクしたファイルは前の世代の検証済み digest を引き継ぐ。新しいファイルだけを再オープンして Unit との一対一を照合する。
- segment 数と検索の遅さを計測し、統合の条件の案を出す（判断3）。

## 段3：Vector の参照化（概要）

- ベクトルの値を `EmbeddingCacheKey` ごとに1回だけ保存し、stage は部品単位の参照の一覧にする。
- 1世代の構築を変わった部品の分だけにし、更新が続いても CAS で公開できるようにする。

## 段4：Graph の部品化（概要）

- 文書ごとのレコードを部品にし、世代は参照と文書をまたぐ関係の差分を持つ。

## 1万文書の段階の合格条件（計測前に固定）

| 項目 | 条件 |
|---|---|
| 1件の更新の反映（イベントから公開まで） | p95 10秒以内 |
| 新しい世代の最初の検索 | 2秒以内 |
| worker のメモリ | 2 GB以内 |
| 1世代あたりの DB・ディスクの増分 | 変わった文書の量に比例し、全体量に比例しない |
| full 構築との論理的な同一性 | digest が一致 |

段1だけでは字句索引と Vector が全件のままなので、反映時間とメモリの条件は段3までで満たす。段ごとに計測値を状態記録へ残す。
