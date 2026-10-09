# Document 1,000件の明示的な段階検証

Status: ACTIVE。利用者承認済みの公式small→1,000→1万→10万方針に基づく次段。通常CIの45分枠、安全係数、製品仕様を緩めない。

## 前提と確認済み証拠

PR110はmain 4fa12dcへ統合済み。c769b620のsmallは公式正例2登録/公開、負例1拒否、metadata/OCC、公開切替、Agentの存在秘匿404/一覧除外、実HTTP再起動保持まで成功。利用者が添付したZIPは元artifact11573699640のSHA-256 d477649d6a949efedeec9579e650e6da1329076bf64d47b8b6f562f469bea1bcと一致。receiptのcanonical SHA-256は20ceb4afbb9fd0396b52cc96531113ae3215683753041a82e3c9fb53666bc399で、厳格schemaも検証済み。これは以前のrunの証拠であり、新しいrunnerの現在容量を証明しない。

small実測6,781.434ms×500×安全係数2から時間screeningは約113分。既存45分jobでは開始しない。

## 実行境界

- 新しいmanual workflowのみ。固定1000の明示選択、同repo/public/mainに限定。PR/push/scheduleで大規模実行しない。
- 既存と同じ標準ubuntu-24.04 runner、固定toolchain/依存/Chromium/PDFium/必須sandbox/owned PostgreSQLを使用。larger runner、新credentials、永続資源、課金設定は追加しない。
- job hard cap180分。small5分、1000の作業中止/admission期限120分、chain deadline125分。期限超過で作業を中止し、進行中の既存probeやprocess cleanupはその後に完了する場合がある。runnerの強制終了上限は180分。build等には残りを確保する。
- factor2、RSS2GiB上限、disk1GiB余裕、available memory512MiB余裕を維持。実測不足、残りdeadline、DB tmpfs/原本filesystem余裕不足ならNOT_ADMITTED、途中超過はABORTED。
- 同一build/runtime/corpus/run UUID/DB/storageでfresh smallから開始し、small成功とidentity一致後だけ1000へ進む。次stageの資源は改めて実測し、既存admissionへfull previous reportを渡す。
- 両stageのdirectory/journal/Folder名を分離し、smallの文書を1000の件数へ算入しない。再起動process世代とlogも重複させない。
- 10,000/100,000の入力や自動継続は今回追加しない。

## 証拠と実装順序

1. chain制御、identity/source不一致、時間/容量停止、small失敗時に1000を開始しないことをRED→GREENで確認。
2. stageごとのFolder名・journal・再起動世代を分離。既存単一small/local planも回帰確認。
3. success-onlyで両stageの公開可能なallowlist証拠を固定JSON1ファイルに保存。最大1MiB/1日、原本/本文/raw report/log/path/env/credentials/他機能のデータは対象外。失敗はpartialとして数値と固定codeを記録し、成功artifactを作らない。
4. 独立reviewと正確headの通常CIを確認してDraft PRを統合。既存のper-PR-main承認手順を守る。
5. public標準runnerの無料条件とexact main refを再確認し、manual1000 runを1回だけ開始。全段階の終端・実測・実再起動証拠を確認する。

GitHub公式の現行案内ではpublic repositoryの標準hosted runner利用は無料、1job上限は6時間。料金や制限が変わった場合は実行前に停止して再評価する。
- https://docs.github.com/en/billing/concepts/product-billing/github-actions
- https://docs.github.com/en/actions/reference/limits

これらは検証用予算であり、製品SLOではない。1,000件を実行するまで合格とは扱わず、NOT_ADMITTEDでも全負荷依頼の完了にはしない。
