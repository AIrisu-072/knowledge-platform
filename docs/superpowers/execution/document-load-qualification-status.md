# Document 段階負荷検証の状況

Status: ACTIVE / 実 API 資格は未取得

## 対象と承認

2026-10-08 の承認済み方針は、公式 PDF の少量試験から始めて 1,000 → 1万 → 10万件へ段階的に増やすこと。製品の性能 SLO や本番資格は設定しない。基点 main `8d4b94a912f9ac2e8079fc1c13b4efa4ce6d7538`、branch `test/document-load-qualification-20261008`。

[実行手順・検証範囲](../../../tools/document-load-qualification/README.md) と `tools/document-load-qualification/official-sources.json` が再開入口。共有 active pointer・導入手順・製品 source は変更しない。

## 既存作業との分離

- 既存 Document seed は合成 5 文書、runtime は実 API/PDF・同一 DB/storage 再起動を既に扱う。新規は公式原本来歴・段階件数・resource admission・計測に限定
- Search corpus branch `feat/search-validation-corpus-20261006` head `e508398bc11e343b76fd2c27c12fc521774c2322` の計画/状況を読取確認。既存 1,000 文書試験を複製しない
- Search 修正 Draft PR85 head `18080d442e78fc6d14c3e15abba93333740a2884` は独立作業。変更も取り込みもしていない
- 旧 Search 記録では公式 PDF は 0 件、Linux aarch64/native PDFium が障害。Document の実公開は資格済み Linux sandbox と PDFium が必要で、Mac native unit test だけで置き換えない

## 準備・ローカル検証

- 厚生労働省の 2025-03-31 通知 2 件を 2026-10-08 に公式 URL から取得し、5頁182,988B / 2頁95,621B、SHA-256 を確認。原本は repo 外。利用条件と出典を manifest/runbook に保持
- Node built-ins、既存生成 SDK / BinaryTransportBridge を利用。依存追加なし
- resource 欠測、deadline、再起動 identity、初回 create の結果不明、OCC 409、権限拒否、全ページ/原本 hash 等を TDD で契約検査
- 独立レビューで 25 万 sample の variadic stack overflow、最終 observation 中の期限超過、別 filesystem の容量増分が min 値に隠れる反例を発見し、RED→GREEN で補修
- cloud の Node は 24.19.0（repository pin 24.21.0 ではない）。Rust / Docker / psql / mise はなし。これらの local 契約結果は実 runtime 資格ではない
- root `mise run verify:fast` は mise 不在で未実行。API 契約の Redocly は telemetry 無効化を確認してから実行する必要があり、未確認の外部送信は実行しない

## 未実行と次の action

- 公式 PDF の実登録・公開・拒否・実 HTTP 再起動：NOT RUN
- 1,000 / 10,000 / 100,000：NOT RUN
- exact-head hosted / 全 CI / merge：NOT RUN
- 初回の次の action：最終独立レビュー、固定 toolchain の契約検査、Draft PRへ保存し、専用 `document-load-small` label で同一 head の通常 CI＋公式 PDF small を実行。実結果をこの記録へ追記する
- small が実成功した後、その report と host 資源から 1,000 件 plan を確定する。前段失敗・資源不足・欠測は調査/判断が残るもので、全段階の完了とはしない

## 2026-10-08 06:59 UTC — 公開前の source 資格

- 独立レビュー：real small 試行へ GO。最終の CI shell 並び替えも再レビュー GO
- 新ハーネス51件、既存 runtime/API 契約と合わせて248/248成功、skip0。生成SDK/binary試験12/12成功。型build、Node構文、diff whitespace検査成功
- 最初の広域回帰は既存 visual-integration の連続shell guard 1件が失敗（247/248）。既存 visual設定の並びを保ち、新small設定をその前に置く限定補修で、既存testを弱めず再度248/248成功
- Redocly2.52.1の実装とroot mise.tomlの正式な telemetry/update notice opt-outを確認し、REDOCLY_TELEMETRY=off / REDOCLY_SUPPRESS_UPDATE_NOTICE=trueでAPI契約を実行した。外部telemetryへの許可は追加していない
- 公開直前mainはPR108統合の6a34de3f0904949daca304b9178ef5125ea13d82へ更新。GUI/既存文書の変更をそのまま取り込み、今回のsource境界を広げない。組合せの再検証とexact-head hostedはこれから

07:01追補：main6a34de3fをfast-forwardで保持した（初回commitは作者未設定で未作成だったため不要なmerge commitなし）。親の確認ではMac空き容量5.3GiBで、大規模local runはadmitしない。現在の実行候補はhosted smallのみ。Search用text/plain固定ingest.pyを読取確認し、今回のPDF API受入clientとの非共有境界をREADMEへ明記した。

07:02追補：main6a34de3fとの組合せで生成SDK型buildと新規＋既存runtime/API契約248/248成功、skip0を新たに確認。独立レビューGOはsmall試行のsource資格のみ。Rust/実DB/公式PDF公開・large stage・exact-head CIは引き続き未実行。
