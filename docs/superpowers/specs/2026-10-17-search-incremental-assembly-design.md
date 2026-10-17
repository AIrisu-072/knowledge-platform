# 検索世代の差分組立て — 1件の更新を変わった文書の分だけで作り、確かめる（設計案）

2026-10-17。状態：**提案（所有者の判断待ち、§8）**。前の設計：[検索世代の持ち方 — 不変の部品と一覧](2026-10-07-search-generation-segments-design.md)（2026-10-07 承認、仕様 SD-T11 5）。根拠の計測は検証基盤の状態記録（`docs/superpowers/programs/search-validation-corpus/status.md` の「workerのメモリ」）。

## 1. 何が問題か（1万文書・169万 Unit、検証VM）

2026-10-07 の設計で、Unit・字句のUnit索引・Vector・Graph の行は内容 digest の部品（segment）と世代ごとの一覧になった。それでも1件の更新で、次の処理は世代の**全件**について行っている。

| 処理 | 場所 | 全件である理由 |
|---|---|---|
| Sourceの読取りと射影の組立て | `crates/search-source-document/src/outbox.rs` の `records` のループ | 全文書の記録を翻訳し、全文書の `CompiledResourceProjection` を作る（Unit の抽出だけは変わらない項目を再利用している） |
| 射影の digest | `search-projection-memory::generation_digest` | 全 Resource の正規形 JSON を順に1つの hash へ入れる定義のため、全件が要る |
| 射影の保存 | `PgPayloadStore::store` の `projection` 行 | 全文書の射影を1つの JSON（1万文書で約41 MB）として毎回書く |
| Resource 単位の字句索引 | `search-tantivy` `build_generation_at` の `RESOURCES_DIR` | 全文書の `LexicalDocument` から作り直す |
| Graph の組立てと digest | `graph_mapping::document_graph_records`、`canonical_graph_digest`、`segments::partition` | 全文書の射影から全レコードを作り、全件の正規形バイト列から digest を計算し、全 segment を組み立ててから、新しいものだけを書く |
| READY の照合 | `ready.rs`、`search_graph::store::validate_on` → `segments::load_generation` | 保存した Graph の全 segment を読んで組み立て直し、全件から digest を計算し直す。射影も全件を復元して照合する |

計測（2026-10-17、main `e9764975`、1文書の更新を4回）：

- workerの最大RSSは2.4〜2.6 GB。2026-10-07 の合格条件「1万文書で2 GB以内」に届いていない。
- 工程ごとの確保量は、ふだんの水準が1.55〜1.8 GB。そこから約+0.5 GBの山が、Graph の書込み、bundle の保存、READY の照合の3か所で、ほぼ同じ高さで出る。
- 3か所のうち1か所だけを減らす修正を2つ試した（射影と Graph を写さずに移す、変わった Graph segment だけ JSON を作る）。どちらも最大RSSは変わらなかった（同じ環境でmainと続けて計り比べ、ぶれは回ごとに±0.15 GB）。

量は文書数に比例する。10万文書では、射影の JSON だけで約0.4 GB を毎回書き、READY で読み直すことになり、成り立たない。

## 2. 方針

**1件の更新で作る量・読む量・確かめる量を、変わった文書の量に比例させる。**

- 世代の部品を、すべて「文書を単位にした内容 digest の部品」と「世代ごとの順序付き一覧」に揃える。2026-10-07 の設計で、Unit・字句のUnit索引・Vector・Graph は済んでいる。残りの射影と Resource 単位の字句索引を同じ形にする。
- 全体の digest は、部品の digest を決まった順に畳み込んだ値と定義し直す。全件の作り直しも同じ部品分けと同じ畳み込みで計算するので、全件と差分の論理的な同一性（SD-T1 5、SD-T11 4）は保たれる。
- READY・切替・pin の照合は SD-T11 5 のとおり、一覧の順序・件数・合成 digest と、その世代で新しく作った部品だけを計算し直す。既存の部品は、一覧と検証 receipt の digest で照合する。これを射影と Graph にも適用する。

```text
文書 X の変更イベント
  ↓
Source の軽い列挙：全文書の (document_id, document revision, access revision, Live Version)
  ↓  前の世代の一覧と比べて、変わった文書の集合 D を決める（X 以外も、権限やフォルダーの変更で入りうる）
D の文書だけ：記録を読む → 翻訳 → 射影・Unit・Resource 字句文書・Graph レコード
  ↓
D の新しい部品を書く（書いたときに全件を確かめ、検証 receipt を付ける）
世代の一覧 = 前の一覧から D を差し替えたもの、合成 digest は一覧から計算する
  ↓
READY：一覧と D の部品だけを計算し直す → 公開（pointer の CAS）
```

## 3. 部品の単位と保存

### 3.1 射影の部品（新規）

- 単位：1つの文書。その文書の Live Version Resource と、その文書が所有する補助 Resource（`auxiliary_projections`）をまとめる。所有は今の `ownership`（Resource → Document）と同じ。
- 表 `search_projection_segment`（主キーは digest）に、その文書の `CompiledResourceProjection` 群を正規形で保存する。世代の一覧 `search_projection_generation_segment`（世代×順序 → digest）を足す。
- `SemanticRegistrySnapshot` は文書をまたぐので、一覧の見出し（header）に置き、その digest を合成 digest に入れる。
- 今の `search_generation_payload` の `projection` 行（1つの JSON）は使わなくする。

### 3.2 Resource 単位の字句索引（新規）

- Unit 索引と同じく、Tantivy の segment ファイルを前の世代からハードリンクし、D の文書だけの新しい segment と削除の印を足す（2026-10-07 §3.2 と同じ仕組み）。
- seal の digest は、ファイルごとの digest の一覧から作る。読み直して照合するのは新しいファイルだけにする。

### 3.3 Graph（作り方の差分化）

- 保存はすでに文書ごとの segment と、文書をまたぐ関係の「残り」segment（remainder）になっている。変えるのは作り方。D の文書のレコードだけを作り、それ以外の segment は前の世代の一覧から引き継ぐ。
- 残り segment は、文書をまたぐ関係（フォルダーの配置など）を集めたもので、今は1つ。これが大きいと、更新のたびに丸ごと作り直すことになる。最初の段で1万文書での件数と大きさを計り、大きければ関係の「起点の文書」ごとに分ける案を出す（§8 判断5）。
- Graph の内容 digest も、segment digest の畳み込みとして定義し直す（§4）。

### 3.4 変えないもの

Unit 部品、字句の Unit 索引、Vector の参照、segment の回収（参照する非 DELETING 世代が無くなるまで消さない）は、2026-10-07 の設計のまま使う。

## 4. digest の定義（仕様の改訂を含む）

今の射影の `manifest.digest` と Graph の内容 digest は、全件の正規形バイト列を順に1つの hash へ入れる定義で、全件がそろわないと計算できない。これを次の形に改める。

```text
segment_digest   = SHA-256(部品の版 ‖ source_id ‖ 部品の正規形バイト列)            … 今と同じ考え方
composite_digest = SHA-256(合成の版 ‖ source_id ‖ header の digest ‖ 件数 ‖
                           順序どおりの (文書キー, segment_digest, Resource 件数) の列)
```

- 文書キーの順序は document_id の順とし、全件・差分どちらの組立てでも同じ列になる。
- 全件の作り直しも同じ部品分けで同じ値を計算するので、「同じ Source snapshot から全件と差分が同じ論理 digest になる」ことは保たれる。全件・差分の両方を作って digest を比べる試験を、各段に置く。
- 改訂する仕様の文言（案）：
  - SD-T11 3「既存 projection-only `manifest.digest` …は変更せず」を「projection の `manifest.digest` は、文書単位の projection segment digest を document_id 順に畳み込んだ合成 digest とする（2026-10-17 改訂）」に改める。
  - SD-T11 5 の列挙「Unit/coverage、lexical、Graph、Vector」に「projection と Resource 単位の lexical」を足す。照合の規則（新しい部品だけ計算し直す、既存は receipt の digest、初めて読むときに遅延検証）は5のまま適用する。
- 公開前（pre-release）なので、旧形式の世代との互換は持たない。移行は全件の作り直し1回で行う（segment DTO v2 を入れたときと同じ扱い）。

## 5. Source の読取り（変わった文書の決め方）

SD-T11 1 は、1つの `REPEATABLE READ, READ ONLY` snapshot で、現行 Live Version・T10・document/access revision・全 AUTHORITATIVE 項目とその順序を固定することを求めている。これは変えない。変えるのは、固定したものをどう読むかである。

1. 同じ snapshot の中で、全文書の軽い列挙 `(document_id, Live Version, T10, document revision, access revision)` を読む。1万文書でも数 MB の行。
2. 前の世代の一覧が持つ同じ組と比べ、違う文書（追加・削除・版・T10・権限・フォルダーの変更）を D とする。
3. D の文書だけ、記録（Part・representation・FileObject の束縛を含む）を同じ snapshot で読む。D 以外の文書は、前の世代の部品をそのまま使う。
4. 公開直前の Source の読み直し（SD-T11 1）も、軽い列挙の再読と D の束縛の再照合で行う。D 以外の文書の束縛は revision が同じことで照合する。

この読み替えは、「全文書の現行状態を同じ snapshot で固定する」ことを revision の列挙で満たし、内容は変わった文書だけ読む、というものである。revision が変わらずに内容が変わる経路が Document 側に無いことを、最初の段で Document の仕様と実装の両方で確かめる（無ければ判断3の前提が崩れるので、案を出し直す）。

## 6. worker のメモリ

- worker が更新の間に持つのは、世代の一覧、D の文書の部品、部品 digest ごとのキャッシュ（上限を設定で決める）だけにする。全文書の射影・Graph レコードを一度にメモリに並べない。
- READY は、一覧と D の部品を読んで照合する。既存の部品は receipt の digest で照合するので、本文を読み直さない。
- これで、更新1回の確保量は「待機中の量 + D の量」になり、文書数に比例する山が無くなる見込み。待機中の量（今は約1.3 GB）は、部品キャッシュの上限で抑える。

## 7. 実装の順番と、事前に決める合格条件

0. 確かめること：残り Graph segment の件数と大きさ（§3.3）、revision が変わらずに内容が変わる経路が無いこと（§5）。
1. 射影の部品と合成 digest（§3.1、§4）、全件と差分の digest 一致の試験。
2. Source の軽い列挙と D の決定（§5）、D だけの射影・Unit・Graph レコードの組立て。
3. Graph の作り方の差分化と、READY の差分照合（§3.3、SD-T11 5 の射影と Graph への適用）。
4. Resource 単位の字句索引の差分化（§3.2）。
5. 1万文書での計測、その後10万文書の段階へ。

合格条件（1万文書、計測の前に決める。数値は案）：

| 項目 | 条件 |
|---|---|
| workerの最大RSS（1件の更新、2〜4回目） | 2 GB以内（2026-10-07 の条件のまま。今は2.4〜2.6 GB） |
| 1件の更新の反映（イベントから公開まで） | p95 10秒以内 |
| 1件の更新で読む・書く量 | 変わった文書の量に比例し、文書数に比例しない（射影・Graph・READY で全件を読まない） |
| 全件の作り直しとの論理的な同一性 | 射影・Graph・Unit の合成 digest が一致 |
| 10万文書 | 同じ条件を、計測の前に改めて決める |

## 8. 所有者に判断をお願いしたいこと

1. **digest の定義を部品の畳み込みに改める**（§4、SD-T11 3 の改訂）。全件と差分の同一性は保つが、digest の値そのものは今と変わる。
2. **射影の部品の単位を文書にする**（補助 Resource を含む）。
3. **Source は revision の軽い列挙で全文書を固定し、内容は変わった文書だけ読む**（§5、SD-T11 1 の読み方の明確化）。
4. **旧形式の世代とは互換を持たない**（公開前なので、全件の作り直し1回で移行）。
5. 残り Graph segment が大きい場合に分けるかどうかは、§7 の0で計ってから案を出す。

## 9. やらないこと

- 公開 API、権限の確認、検索の順位付けの変更。
- Graph の保存先の変更（PostgreSQL のまま）。
- 字句の解析方式（bigram）の変更。
- Document 側の transaction と outbox の意味の変更。
