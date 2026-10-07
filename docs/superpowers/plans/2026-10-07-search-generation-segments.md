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

## 段1の計測（2026-10-07、1,000文書、Vector停止）

1件の反映 中央値58秒→26秒、worker のメモリ最大 7.5 GB→4.5 GB、1世代あたりの DB 増分 79 MB→1.5 MB。詳細は検証基盤の状態記録。

## 段2：字句索引の差分化

### 分かったこと（2026-10-07）

- 世代のディレクトリは `resources/`（Resource 索引）、`units/`（Unit 索引、1,000文書で約159 MB）、`lexical-input.json`（約0.2 MB）。
- Unit の検索文書は世代の鍵（`IndexedUnitDoc.generation`）を保存していて、seal はそれが世代の鍵と一致することを確かめる。このままでは前の世代の segment ファイルを使い回せない。
- READY の古い世代が回収されていない。検証環境では20世代・字句索引3.0 GB が残り、更新1回ごとに約160 MB 増える。`PgGenerationGc::retire_unpinned` は実装済みだが、worker から呼ばれていない。

### T7 古い世代の回収

- 公開に成功した後、現在の世代と直前の N 世代（設定、既定1）以外の READY 世代のうち、pin の無いものを `retire_unpinned` で回収する。pin のある世代は次の機会に回す。
- 試験：更新を続けても世代数とディスク量が一定に保たれる。pin 中の世代は消えない。

### T8 Unit 索引の segment を部品と対応させる

- Unit の検索文書に保存する鍵を、世代の鍵から部品の digest に替える（Unit 索引の schema 版を上げる）。seal と検索時の照合は、文書の部品 digest が世代の一覧に含まれることで行う（SD-T11 5）。
- 世代のディレクトリは、前の世代の `units/` の segment ファイルをハードリンクし、消えた部品の文書を削除の印で消し、新しい部品の文書だけを追加して作る。前の世代のファイルは書き換えない（meta.json は Tantivy が別名に書いて置き換える）。統合（merge）はこの段では止める。
- seal は、部品ごとの検証済み digest を引き継ぎ、新しく追加した文書だけを再オープンして Unit と一対一に照合する。ファイルの digest は、ハードリンクしたものは前の世代の値を引き継ぎ、新しいファイルだけを計算する。
- Resource 索引（999文書）と `lexical-input.json` は小さいので、今は全件のまま作る。

### T7・T8 の計測（2026-10-07、1,000文書、Vector停止）

1件の反映は定常で14〜15秒（段1の後は26秒）。worker の1件の処理は約8.3秒で、内訳は本文の組立て1.7秒、字句索引の作成1.4秒、Graph 0.6秒、字句の seal 1.0秒、receipt と payload の保存1.0秒、READY での再検証2.1秒。検索サーバーの読込みに約5〜6秒。字句索引のディスクは2世代で一定。

残りの処理は、どれも全 Unit をたどる（本文の組立てでの現在の Unit 一覧の複製、字句の論理 digest と unit seal の全文ハッシュ、seal の全件照合、payload の全件検証、READY での同じ検証のやり直し）。1万文書では約10倍になる。

### T10 検証を新しい部品だけにする

- 字句の論理 digest と unit seal を、部品ごとの digest の一覧から作る v2 にする（部品 digest は T1 の Unit 部品と同じ単位）。full 構築も同じ式で計算し、同一性は保つ。
- 部品ごとの検証（本文の SHA-256、Unit と字句文書の一対一）は、そのプロセスで確かめた部品の集合を覚え、新しい部品だけ行う。READY での再検証は、同じプロセスで finalize 済みの世代なら、ファイルの digest と部品一覧の一致で済ませる。
- 本文の組立てで現在の Unit 一覧を丸ごと複製せず、部品 digest で変わった項目を判定する。
- 検索サーバーの読込みも同じ部品単位の確認にする。
- 合格の目安：1件の反映が1,000文書と1万文書でほぼ同じになること。

### T10a の計測（2026-10-07）

同じプロセスで確かめた部品の検証の省略と、READY での seal の再利用を入れた後、1件の反映は10.7〜15.8秒（中央値約13秒）。

### メモリの断片化（2026-10-07）

glibc のスレッドごとの割当て領域が断片化していた。`MALLOC_ARENA_MAX=2` で worker の最大メモリは約4.0 GB→約2.5 GB、1件の反映は12〜15秒→8.9〜10.4秒。本番では配備の設定で与えるか、割当て器を選定し直す（所有者の判断）。T12 はこの値を前提に、1万文書で必要なメモリを測ってから範囲を決める。

### T11 処理済みのイベントを即座に省く

- 1回の構築はその時点の全変更を含むので、後続のイベントは「変更なし」になる。今はその判定にも全項目の組立て（1,000文書で約1.7秒）を行っている。
- イベントの文書版と、現在の世代の部品一覧（文書版×内容部分）を照合し、既に含まれていれば構築せずに完了させる。

### T12 全件の作り直しのメモリを文書数に比例させない

- 全件の作り直しは全 Unit をメモリに載せる（1,000文書で約5 GB）。1万文書では検証環境（12 GB）に収まらない。
- 文書ごとに抽出・部品の保存・字句索引への追加を進め、全 Unit を同時に保持しない。

### T9 segment の統合の条件（判断3）

- segment 数と検索の遅さを計測し、統合の条件の案を出す。統合した世代は新しいファイルになるので、それ以降の世代はそこからハードリンクする。

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
