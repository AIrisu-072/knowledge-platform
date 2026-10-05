# SearchとmainのDomain migration統合判断

## 判断の範囲

2026年10月4日の統合候補。受入済みDocument/Organizationを含むmain `9c90f383f2f88f5312d541aecb8eef8766bb1ffc` と、Searchのレビュー済みlocal `3738e286e6f859021d94c6b0066fe0a64ea45d52` を履歴ごと保つ。Searchの公開PR40 head `1571ee49af66defa6a0e2b738c81f299a2c0ef4f` は同じtree `ca7ecabe4fe8732b12cdf80d9d43df2645a00ada` を持つ。

これは親担当から委任された統合上の工学判断であり、所有者による新たな凍結設計の個別再承認を主張しない。既存の設計・承認原文hash、SQLの意味、配送権限、Documentの業務境界を変更しない。公開、mainへのmerge、deploy、既存実DBへの接続・実行は別操作である。Search全体の完成・本番適格性も主張しない。

規範は `spec/`、既存の[追加型Outbox設計](../superpowers/programs/search-platform-completion/p6-outbox-design-revision-1.md)と[P6計画](../superpowers/programs/search-platform-completion/p6-outbox-plan.md)。元の「Domain 0009」はその凍結時点のSearch枝の識別子として残す。本統合の配置先だけを以下で明示する。一般的なmigration互換framework、下りmigration、台帳書換機能は追加しない。

## 一つのDomain台帳での配置

| version | 統合後のファイル | 保持する根拠 |
| --- | --- | --- |
| 1〜8 | 既存Document migration | 両側の既存bytesをそのまま保持 |
| 9 | `0009_document_revisions_v0.sql` | main blob `26ac29eb80991a6aea2b46bd66e3543ae3cd8dd6` |
| 10 | `0010_document_version_updated_at.sql` | main blob `a533a89e445e26676711ee258ab1c44640b5c4c5` |
| 11 | `0011_outbox_delivery_v0.sql` | Search旧 `0009_outbox_delivery_v0.sql` のSQL bytesをそのまま移動 |

Outbox SQLのSHA-256は `6059108aa478df2b5c9af5417e650ce9d661f74e77236fa0e2b8e02e626cf7bb`、SQLxが使うSHA-384は `df0d246c1b3ab0c344a18c13936630a8d618fad2ae21649c9a13a16ac7d77cca75ca201b00f2b7acc956f93549b84783`。ファイル名の番号だけを変え、末尾改行を含むSQL本文は不変である。既存Outbox試験のpathと最大version期待値だけを追随させる。

Document9のSHA-256は `cb0bb8030e012e3777a939935bf147f59c0a72359829e783a81f3201f7966255`、Document10は `9e1478d0d42cd7d3239d5cd067531d479c81fbd30af5a600a02f89170092cc76`。Search runtimeとWorkは従来の独立した台帳のままである。

## 履歴ごとの境界

| 開始時の履歴 | 候補の扱い | 合成試験で確認すること |
| --- | --- | --- |
| 新規の空DB | 1〜11を適用 | 全version/checksum、両機能のschema、再実行の不変性 |
| 正規1〜8 | Document9/10、Outbox11の順 | 旧Document/Version、Domain/Auditイベント、旧台帳の保持、Revision backfill、更新時刻の根拠 |
| 正規Document1〜10 | Outbox11だけ追加 | 既存Revision/Version更新時刻、イベント、台帳の保持 |
| 旧Search1〜8＋Outbox9 | **停止** | compatibility checkのchecksum不一致、SQLx `VersionMismatch(9)`、Document9/10の未適用、元データ・台帳の不変性 |
| 欠落・失敗・未知version・別checksum・台帳なしの既存schema | **停止して個別調査** | 上の対応済み履歴へ読み替えない |

旧Search9試験は、同じSQLをversion9としてSQLxで実際に適用し、その履歴を作る。台帳のversion/checksumをUPDATEして作り替える試験ではない。製品の既存 `migrate` とSQLxのchecksum検証をそのまま使用する。既存のread-only起動互換検査も同じ埋込migrationを使う。旧Search9に対しDocument9/10を適用したり、Outbox11を再実行したりする移行経路は提供しない。

**所有者の実環境に旧Search9適用済みDBが存在しないことは未証明。** 合成DBの結果、branchの状態、mainへのmergeだけでは証明できない。実環境の分類・backup・復元試験と停止判断は[手動更新のSTOP手順](../operations/search-main-migration-stop.md)に従う。

## 共有ファイルの競合解決

12ファイルに競合があった。既存のDocument/OrganizationとSearchの内容を加算的に保持する。

- CIは両側のjobとrequired-checkの全依存を保持する。既存Search合成PostgreSQL jobに新しい履歴試験を追加する。permissions、公式PostgreSQL pin、既存画像公開条件は変えない
- Rustの公開export、architecture default、依存境界、規範追補、ignoreは双方の独立した項目を残す。Activeの過去checkpointも双方を残し、現在状態だけを先頭へ追加する
- `.gitleaksignore` の非comment指紋集合は両側とも同じ35件。mainのファイルbytesを採用し、新しい例外、広いルール、指紋削除はない
- Cargo lockは両側の既存603 package identityの和集合。同じname/version/source/checksumを保ち、既存featureの依存edgeを統合する。unionに複数versionが存在する依存名は既存の正確なversionへ明示化する。新dependencyの採用・version更新・downloadはしない

## OpenAPIの包装

両側は同じ `spec/api/openapi.yaml` に別契約を持っていた。Search側は4routesだけで、Documentのpathを含まず、その53componentすべてが4routesから到達する。`Problem`、`FieldError`、`Cursor` は両側で別の意味を持つ。Documentの34operation SDKと明示schema dialectも維持する必要がある。

mainの `spec/api/openapi.yaml` はbyte不変で保持し、Search原本を `spec/api/search-openapi.yaml` にbyte不変で置く。Searchのwire field、status、header、security、schema dialect、内部 `$ref` は変更しない。Search用 `Problem.trace_id` をDocumentの `traceId` へ変換しない。旧凍結文書内のpath/hashは当時の原本参照として保存し、今後のSearch契約の参照先はこの別fileとする。

現在の規範入口である `spec/api/README.md` と `spec/data/logical-data-model-v0.md` のSearch契約への参照先もこの配置に揃える。後者はfile名・linkだけを変え、論理契約の本文は維持する。

- Document原本SHA-256：`aa1b073dcaee33b7be77c7877cc362d8fb51ef29c1a322c28497cf052f98bb1f`
- Search原本SHA-256：`803db0d6051860dc92cb3fc1571aeddae3ba36be376dab5e8ef69edda217d144`
- 既存 `api:lint` は両fileを必ず検査する。既存 `api:contract` はDocument12件に加えてSearchの4route、閉じた入力、独立Problem、全responseのprivate headerを3件で確認する

同じbytesと解析済み構造を照合するため、これはwire意味の再選択でもfreezeの再承認でもない。Search runtime HTTPの実装・資格が新たに得られたという意味でもない。

## 現在の検証と残り

- Rust1.98.1、offline/locked、jobs2/debug0/incremental0：新履歴試験5件と既存Outbox試験4件をcompile。SQLとversionを固定する通信なし1件PASS、対象strict Clippy PASS
- architecture policy22件と実repository check PASS。両OpenAPI lint、contract15件、Document SDK34operation一致1件PASS。Search原本のlocalhost placeholderに既存のlint warning1件あり
- ローカルDB/Docker/socket/listenerは実行していない。新履歴DB4件、統合した全sourceの全体CI、実環境inventoryは未検証。初回test compileのhex表示エラーは修正済みで、意味的DB REDとは扱わない
- 独立ソースレビューは候補tree `8d13a27c188ed5ca563a38974fa2c79ce88d88dc` に対しGO、Critical/Important残件なし。検証用symlinkの誤stage、Search側既存試験2件のDomain件数9期待、現在規範の参照先2箇所を修正して再確認した。symlinkはindexと作業木の双方から除外済み
- Searchの既存 `coordination_migration` / `source_ownership_migration` はDomainの正確なversion1〜11を期待し、前後のchecksum保持assertを維持する。この2targetも修正後にcompileとstrict Clippy PASS。Cargo lockの依存edgeも既存1,971件の和集合に一致。両parentのCI job構造・全11 predecessorの必須化・permissions保持、共有文書の両側変更内容、root fmt、新規変更のdiffも確認した。全体diffの空白警告8箇所は保持したSearch/vendor原本に限る
- レビュー以後は本検証記録とActive/Statusだけを更新。次は親担当による公開、同一headの公式PostgreSQL合成履歴4件・既存Outbox4件・既存Search台帳分離と全適用CI。結果を元mainやSearch旧headへ遡及しない
