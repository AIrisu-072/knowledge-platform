# Search PoCのSegmenter直接利用と実測

2026-10-14承認済みの限定保守。既存Search本体とPR85/86/99を統合しない。`lindera-analysis` のフィルターを一切使わないPoC経路を、同版 `Segmenter::segment(Cow::Borrowed(text))` へ接続した。[上流Tokenizer](https://github.com/lindera/lindera/blob/0482efcc46b5ec518b7238efae55e9fed9bba0c0/lindera-analysis/src/tokenizer.rs#L303)と[Segmenter](https://github.com/lindera/lindera/blob/0482efcc46b5ec518b7238efae55e9fed9bba0c0/lindera/src/segmenter.rs#L654)で、無filter/空offset補正時には同じdefault Latticeで分割することを確認した。

旧main373941fbのPoC lockはPR123と同一。`unsafe-libyaml0.2.11` は[RUSTSEC-2024-0449](https://rustsec.org/advisories/RUSTSEC-2024-0449.html)のinformational unmaintainedで修正版がなく、不要なanalysis経路を除去する。例外・scan除外は追加しない。

## 依存候補7件

Tantivy0.26.2、Lindera6.2.0、IPADIC6.2.0、lru0.18.2、Tokio Postgres0.7.18、testcontainers0.28.0、PostgreSQL18.6-bookworm。辞書・既存fixtures・残る依存の版/源/checksumは不変。lockは新規追加/更新なし、analysisと専用transitiveのkanaria/serde_yaml_ng/unicode-blocks/unicode-segmentation/unsafe-libyamlだけを削除した。

## 旧新の同等性

旧main373941fbから取得した[基準](../../../experiments/search-discovery-poc/tests/data/tokenizer-6.2.0-golden.json)は全resource title/summary/alias・全query・空/英数/記号/日英混在/改行tabの59入力、257tokenを含む。新実検索経路とsurface/順序/byte開始終了/position/position_lengthが完全一致。両analyzerの13ケースすべてのretrieved/qualified IDs、missing、recall/MRR/nDCGのIEEE754 bits、index bytes5035/5939も一致。

JSON小数の読取りで1ULP変化する境界は、許容誤差を追加せず、別保存した旧値のu64 bitsと新値を正確比較する。JSON報告の小数だけは同じserialize/parse経路で比較する。旧新各5回・計10評価の安定値もすべて一致した。

## 新しい実measure-lexical

[旧新のraw実測](../../../experiments/search-discovery-poc/measurements/2026-10-14-segmenter-lexical.json)。固定Rust1.98.1、macOS arm64、unoptimized+debuginfo。両候補を交互5回、各評価でquery13件を5回。以下は新実測の中央値で、旧2026-09-29のtimingsを付け替えていない。

|候補|Recall@10|MRR|nDCG@10|Index bytes|Build ms|Query p50 ms|Query p95 ms|
|---|---:|---:|---:|---:|---:|---:|---:|
|Tantivy default|0.846154|0.807692|0.817764|5035|80.378875|0.085292|0.186334|
|Lindera IPADIC|0.923077|0.823077|0.847522|5939|93.281708|0.198375|0.259208|

他のcompileが同じMacで競合する可能性があり、性能改善・本番SLO・Ubuntu性能の合格を主張しない。qualityとindex容量だけを互換性判定へ使用した。qualification-reportは候補表とlexical新timingsのみ更新し、graph/fusionの過去測定は更新・再測定扱いにしない。辞書assetのproduction packaging/license未資格という従来の境界も保持する。

## 試験と未実施

候補7件と新helper不在のRED確認後に実装。新golden/harness9件成功、verify成功、cargo fmt/diff成功。PoC cargo-deny0.20.2はadvisories/bans/licenses/sources成功、root OSV2.5.1の原本再帰scanはexit0/No issues found（既存5除外を保持、追加なし）。

全PoC試験は27成功・1失敗。失敗は `postgres_incidence_matches_reference_paths_and_reports_feasibility` のDocker socket不在で、試験DB作成前。旧新の互換差ではない。Ubuntuへの既存SSHも接続timeoutで実DB確認は未実施。Docker/既存serviceを勝手に起動せず、未成功を隠さない。実DB環境で同じ最終sourceを確認する必要がある。独立レビュー・正確head全CI・main統合はこれから。

時間予測PR123の128.9分>120分未達は維持する。新長pipeline・デモの起動は行っていない。
