# Organization Browser PoC — 最終事務タスクの完了

目的: 最終事務タスクを人間が明示的に完了し、保存済みの提出・根拠・判断・実行結果を現在権限の読取り専用履歴として確認できるようにする。

基点は[PR60](https://github.com/AIrisu-072/knowledge-platform/pull/60) `48ae1bfd915119ae8625705474577f093d5d303c`（local `3e7a0fd`、同tree `a0a1a2e12b52c3f228b4f77a249fc38403b1abdd`）。独立branch `feat/organization-complete-slice` に分離する。Frozen [Domain/API §4・13](../specs/2026-10-02-organization-client-v0-domain-api-design.md) の既定契約だけを実装する。親による継続範囲指定は2026-10-04 17:03 UTC。

## 最小範囲

- 次担当へのforward handoffが不要な最終事務stepだけ、現在責任・定義action ID・expected attempt/revisionを検証してactive→completedにする。営業は従来どおりsubmitを使う
- POST tasks/{id}/actionsのclosed complete入力、既存operation ledger/OCC/transaction/stagingを再利用する。hold/resumeは今回実装しない。完了は提出snapshotや過去attemptを改変せず、新担当を作らない
- serverが返す可否/action IDをUIで使い、確認→完了→読取り専用表示へ進む。結果不明時は同operationの既存回復手順を使い、状態文字列だけで操作権限を推測しない
- 完了後も現在のprivate/source認可を保持し、履歴の閲覧を共有拡大にしない。新identity/外部送信/モデル/DB設定/依存を加えない

## TDDと受入

1. Work domain/application/repository/HTTPの既存試験へREDを追加。現在責任・wrong action/attempt/OCC・同operation replay・失敗rollback・過去snapshot不変を最小確認しGREENにする
2. 生成API/共通UIへ確認取消・完了・readonly・競合/不明結果回復の純粋回帰を追加する
3. 同じ2名journeyの末尾に事務完了を追加し、既存両HTTP server再起動後に状態・履歴・operation replay・private分離・cleanupを確認する。画像/trace/videoや新runnerは追加しない
4. 限定独立レビュー、exact headで通常CI。ローカルはpure/型/build/collection-onlyに限り、DB/listener/browserは起動しない

旧Agent sourceを変更せず、全CI完了と統合は別途親が調整する。実装者が必要な小判断を行う場合は理由/影響を日本語statusに残し、所有者の個別事前承認とは記録しない。
