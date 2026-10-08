# 自動既読・未読戻しの最小再実装計画

仕様: [承認済み意味と限定要件](../specs/2026-10-07-document-view-read-state-design.md)。旧未公開sourceは取得不能のため新sourceで実施する。branch feat/document-view-read-state-20261007、基点main a7cf93d53。

## 共通条件

- 他担当のmain/Tauri/Org/Audit配送/Search製品を保持。Document migration集合期待3箇所と、通常hostedで実FAILを確認した台帳最大番号期待1箇所だけを追従する
- Node24.21.0/pnpm12.4.1/Rust1.98.1、固定lock/公式source。RedoclyはREDOCLY_TELEMETRY=off、REDOCLY_SUPPRESS_UPDATE_NOTICE=true
- ローカル実DB/socket/Docker/Chromiumなし。DB testsはcompileだけ。Cargo排他・task専用target
- source所有を分け、stage/commitはcontrollerが直列化。必要なAPI/DTOを固定してからGUIを並行可能にする
- 各検証単位を同じ1Draft branchへ保存する。未完成/未資格は日本語で明記。旧件数を新sourceの成功にしない

## 1 backend契約・移行・API/SDK

- [ ] 新純粋Application/API契約の欠如をREDにする。旧PUT/4field互換、UUIDv7/RFC variant、安全な整数、本人/target/digest固定、UNKNOWNを先に検査
- [ ] Application CurrentReadProjection/State、ReadStateOperationId、ReadStateMutationKind/Mutation/Result、CurrentReadStateRepository/Serviceを追加。既存read_stateは初回記録/再生の意味を維持
- [ ] Postgres current_read_state moduleとmigration0012を追加。共有query/history/read projection、HTTPの新GET/POSTと既存error mapping、OpenAPI/生成SDKを接続。説明は日本語
- [ ] 移行/旧PUTとVIEWの競合/同ID再生/異Doc同IDrollback/古いVIEW再生/別tab CAS/未読RESET/上限/権限/新版/絞り込み/Auditとreceipt失敗のDB反例を既存fixtureで追加。ローカルcompile、実行はhosted
- [ ] pure Rust、HTTP in-process、API lint/contract、SDK型/試験、scoped clippy、DB/HTTP compileを確認。独立review後、backend単位を同Draftへ保存

主な所管: crates/document-application、document-repository-postgres、document-api-httpのread-state/query/history境界、spec/apiと関連規範、packages/document-api-client、tools/api-contract。Searchの変更は既存3台帳assertionだけ。

## 2 GUI・固定回復

- [ ] 実Home→Detail/StrictMode/本番15秒QueryClientの正しいlocatorで、新機能欠如をRED確認。query reset、同tick Reset、CSS非表示祖先の既知3反例を先行
- [ ] Document専用navigation/store/hook/componentと既存facade/API wrapperを追加。main.tsxはinstaller最小接続。現在read/入場token/操作receiptを分離
- [ ] tab/refetch/remount/reload、hidden/遅着/新版/拒否、MAX、UNKNOWNの往復/固定再送、成功後read失敗、他操作/Blob/Org保持、同tick Version変更を実routeで検査
- [ ] focused→全GUI/schema/型/build、独立review。重要指摘を限定修正し同Draftへ保存

所管: apps/document-web/srcのDocument機能と関連test。共有shell、既存拒否barrier、履歴日時labelの限定差分をPRへ明記。生成SDKはbackend所有、e2e-runtimeは次Task所有。

## 3 既存受入・手順・hosted

- [ ] document-runtime.spec.ts/persistence.spec.ts/metadata-editor.spec.ts/support.tsを最小拡張。旧assertion/checkpoint/添付/private fieldを保持し、新しい状態遷移だけ明示
- [ ] 未読→通常表示→既読→RESET→同画面再取得/タブ往復→未読一覧→再入場、旧PUT/古いreceipt再生、Agent拒否、最後の閲覧後RESET保存、HTTP再起動前後の本人状態を照合
- [ ] metadataの未読snapshotは日付/属性/条件操作の後・再入場前に完全比較し、後のVIEWで誤変化を隠さない。move/read-only不変性は表示後baselineから検査
- [ ] ローカル型/既存純粋guard/収集だけ確認し独立review。日本語操作/移行互換手順を同機能へ保存。現在pin未収録と未資格を明記
- [ ] 最新mainとの差とtreeを照合、同Draft exact-headの通常全CI/既存DB/browser/HTTP再起動/cleanup/artifactを終端まで確認。未取得stdoutは既存gateの一次step/source対応と区別し、失敗を隠さない
- [ ] 最終branch reviewとrequired checks後に親へmerge-ready。親のmerge後は同じgateで統合後CIを確認

GitHubへの保存は同じ許可済みrepository/connectorのみ。明示拒否は停止し、根拠が届いたら同じcallを一度だけ再試行。uncertain writeはreadbackして重複を避ける。別経路への迂回はしない。
