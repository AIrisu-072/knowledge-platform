# Document HTTP/OpenAPI Transport v0 — 横断受入証拠

## 判定境界

承認済み設計 `2026-09-29-document-http-openapi-transport-v0-design.md`（blob `88f7046a5d14a77f4091df0c92691f6634dd57d7`）とProduction Implementation Plan（blob `111914181143672d3fec901dae75fc2af0256b15`）のHAPI-01〜12を対象とする。設計意味の変更、新しいDB migration、本番identity接続、本番deployは行っていない。

Delivery UnitはA→B→C→Dのstacked Draft PRで読む。既存のexact-head hosted gateは次のとおり。

| Unit | head | Draft PR | 標準CI | Sandbox | DSI PoC |
|---|---|---|---|---|---|
| A: contract / foundation | `cea5f75b19ec0c68d41f3e7e782a6a44cb21f8a9` | #29 | `36587061407` SUCCESS | `36587061863` SUCCESS | `36587061414` SUCCESS |
| B: read / management | `241c5d7e97ca747eea72dca4e7cd58dcfa9c15a3` | #30 | `36651316701` SUCCESS | `36651316802` SUCCESS | `36651316757` SUCCESS |
| C: create / version / lifecycle / file | `4cbca3957ebee4d086aa1601aa1f0d104de35dd3` | #31 | `36656005136` SUCCESS | `36656005182` SUCCESS | `36656005133` SUCCESS |
| D: Diff / hardening / acceptance | `423194cb8dec6c7c384df4d92c4e7ccfbde9c518` | #32 | `36661472741` SUCCESS | `36661472033` SUCCESS | `36661472021` SUCCESS |

Unit Dのreview済みcode headは `e0c4842bdf348fd7f807aad0d3cb548b1a7c9f22`、受入記録込みの初回hosted GREEN headは `423194cb8dec6c7c384df4d92c4e7ccfbde9c518`。このevidence追記commitを最終exact headとしてもう一度3 gateで確認する。merge・deployは別の明示指示を要する。

## RED → GREEN

| Task | RED | GREEN / review head | 主な証拠 |
|---|---|---|---|
| HAPI-01 | `c5b2ba90b10ce27fe4c0eddecbbd1ba971e66848` | `7ca6d5cf39b52df527c0e6708c7686ae75e61e87` | OpenAPI 3.2.1、Error Registry、codegen qualification |
| HAPI-02 | `44af801fdfc39f8cadf9d7c59762f05279d55357` | `f22880c885c8cf0d2c9cf2f555bfabb6bba4bc31` | policy read / root discovery / create recovery / typed error |
| HAPI-03 | `5e13115388a29c4c943487708e372a4b61b3a6ac`、`202e4cd1d5f146c491e721fbf55f4d18fc0909c1` | `bd73d36ae17e0a7e00bca3f1c029df47e58680aa` | identity / Problem / trace / architecture boundary |
| HAPI-04 | `110807a3bb75bb99076f0d2ee4949ca93b45bfcf` | `654b04ce189b146f0840e01e1fc3fddc7f6fe45f` | authorized read、cursor、purpose、policy |
| HAPI-05 | `b5c0f9e0ad1325a204ada356cbfc8bf252ecbb4f` | `241c5d7e97ca747eea72dca4e7cd58dcfa9c15a3` | management / read state / replay / typed conflict |
| HAPI-06 | `31057fda33d51424c721283d7694618666ff2dc5` | `feb6459b33731a329a5dc238f822b02fcebe83a1` | bounded multipart create / commit-unknown recovery |
| HAPI-07 | `de27f408d483801709fbfcc4f0f9df6303f5a575` | `cb8c500a0a246a03ba89ccdf633f0a90d786e132` | Version create / update / rebase / manifest binding |
| HAPI-08 | `a3ae4f92746bb777fdfa726f2812535bf75192ab` | `ecbe3557bf065cfdb73c6cd52763de0fa5300fa0` | publish / withdrawal / schedule / cancel / T10 |
| HAPI-09 | `61eeda1bd98f4b528a7aa6fb1c380b81348b918b` | `4cbca3957ebee4d086aa1601aa1f0d104de35dd3` | audited streaming、Range拒否、safe headers |
| HAPI-10 | `3ef7e4159d0b5951ff8060985f926b386da5451a` | `8b6689d3b20b50d35ff3b7d0df1c1c6e41e93fd9` | Diff verdict / coverage / source evidenceのlossless projection |
| HAPI-11 | `ae5b14a406c782b3da44561c85925c060c94c43e` | `2215eb4bfa79adaa08608cc62233bdb4c3a073d7` | finite limits / timeout / cancellation / browser / trace |
| HAPI-12 | `c3da21a7a2d1c9efa4171a1138af6ba2fcc38607` | `d1ddfa264ded0024c3850f0087c4e62adc2c2e1d` | 30 operation dispatch、実PostgreSQL / FS / DSI / Diff縦断 |
| HAPI-12 review | `6336ac51968f2420c961b3dd219077a5f2d50eff` | `e0c4842bdf348fd7f807aad0d3cb548b1a7c9f22` | 全operationのrequest/response example必須化とRedocly schema整合 |

## 横断受入

- `compose_document_api` は30 operationをread / management / create / versioning / publication / file / diffへ一意にdispatchする。各route familyは独自のtrusted identity、timeout、security middlewareを維持する。
- 実PostgreSQL 18.6、`FileSystemStorage`、test identity adapter、production DSI/Diff executorを使い、root discovery → Folder作成 → Document作成 → authoring list → Version作成/更新 → publish → published read → mark read → metadata/move/policy → history/file → Diff → policy revoke →再開示拒否をHTTPから縦断した。
- macOSの横断試験はproduction worker shellを直接呼ぶ。Linux標準CIでは `DSI_WORKER_BIN` / `DIFF_WORKER_BIN` をbuildし、sandboxed production runnerを通す。production allow-all identity binaryは存在しない。
- replayはFolder作成、Version作成、publish、既読、metadataで同じ結果を返す。異なるtarget Folder IDを同じoperation IDで使うと `OPERATION_CONFLICT`。終了時のDocument行数は1で、重複作成はない。
- create commit-unknown/recovery、cursor invalid/stale、T10、expired identity、identity unavailable、Audit failure時0 byte、Diff Partial/Unknown、cache hit再認可は各focused HTTP/Application/PostgreSQL試験で確認する。
- file downloadは認可とAudit commit後にStorageをopenし、64 KiB chunkで送る。Rangeは416で、Audit/Storage失敗時にfile byteを返さない。Storage locatorはresponse/logへ公開しない。
- OpenAPI contract testは全30 operationに成功schema、request body schema、media type example、実行可能HTTP test evidenceがあることを照合する。Redocly `no-invalid-media-type-examples` をerrorに固定し、exampleを宣言schemaに対して検証する。
- architecture checkとnegative smokeはHTTP crateからDB/Storage具象への直結、Domain/ApplicationからAxumへの依存、CI bypassを拒否する。

## Acceptance criteria対応

| Design AC | 証拠 |
|---|---|
| 1–7 API contract | OpenAPI 3.2.1の30 operation、actor field禁止、RFC 9457 code、replay/revision/cursor、Diff lossless DTO、file locator非公開 |
| 8–12 認可・Audit | adapter必須startup、全familyの認証middleware、architecture negative、file Audit先行、HumanInteractive既読、剥奪後のread/file/Diff拒否 |
| 13–18 信頼性 | exact replay、409のtyped conflict、create recovery、caller-fixed target IDs、有限body/timeout/cancel、cursor invalid/stale分離 |
| 19–22 tooling / client | Redocly + JSON Schema + example + operation coverage、codegen PoC gate、thin typed-client境界、GUIなしHTTP縦断 |
| 23–25 management readiness | administer付きpolicy GET、root discovery、machine-readable management error |

## 検証と資源実測

- HAPI-12 focused: E2E 1/1、dispatch 1/1、HTTP crate 40/40、strict Clippy、fmt、`mise run api:check` 10/10、`mise run arch:check`、`mise run arch:negative-smoke` がPASS。
- workspace test: `cargo nextest run --workspace` 相当の `mise run test:rust` は **686/686 PASS、既定skip 5**。
- Linux hosted standard CIはproduction DSI/Diff workerをbuild後、`document-api-http::e2e postgres_filesystem_and_workers_complete_the_document_http_journey` をPASSし、workspace **702/702 PASS、既定skip 6**。
- `mise run verify` のsecurity、fmt、workspace check、strict Clippy、architecture、APIはPASS。最初のtest linkだけがhostの空き容量0で失敗したため、再生成可能なcurrent-worktree targetを `cargo clean` で9.2 GiB整理し、未完了だった `mise run test:rust` だけを同じsource headで再実行して上記686/686を得た。source/test failureではない。
- HAPI-11ローカル実測: small upload 0.45秒 / max RSS 124,616,704 bytes、1 MiB + 1 byte upload 0.15秒 / max RSS 125,337,600 bytes、差約0.7 MiB。macOSローカル候補の妥当性証拠でありSLOではない。

## Codegen / GUI readinessと残る境界

- `openapi-typescript 7.13.0` は全30 operation type、policy discriminator、Diff union、nullable policy IDを生成したが、multipart binaryを `string` / `string[]` に縮退した。
- `json-schema-to-typescript 16.0.0` もbinaryを同様に縮退し、`oneOf` の排他意味をTypeScriptへ完全には移せない。`typify 0.7.0` は実contract fixtureでcompile failure。
- したがって3候補ともproductionへpromoteしていない。GUIはOpenAPIと手書きthin boundaryから開始できるが、Blobを保つproduction typed client generatorは未選定である。これはAPI contract完成と分離したclient tooling gapである。
- 本番Identity Adapter、Windows/AD等のidentity resolver、HTTP server composition/deployment、GUI/CLI/Agent Toolは未接続。Draft PRのmergeと本番deployも未実施。
