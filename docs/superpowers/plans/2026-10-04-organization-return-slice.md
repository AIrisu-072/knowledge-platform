# Organization Browser PoC — 差戻・再提出の最小slice

## 既存設計と承認

所有者の2026-10-04 07:21:34 UTC「続けてください」に基づき、実証済みPR54 source `44e1b41219a77809f82fe22045cb4fceaf0c1ed8`（local `357e1e2a`）を保持して別branchで進める。凍結[Domain/API§4・6・7・14](../specs/2026-10-02-organization-client-v0-domain-api-design.md)、[UI§5–7](../specs/2026-10-02-organization-client-v0-ui-design.md)、[Product/UX§7](../specs/2026-10-02-organization-client-v0-product-ux-design.md)の差戻契約を実装する。新しいworkflow/認可方式は設計しない。

## 最小操作

1. 事務が受領した内容を確認し、理由を入力して営業へ差戻す。確認cancelは変更しない
2. 事務の現在attemptを閉じ、immutable ReturnInstructionと既存submissionへの因果を保存。同じ営業WorkItemへ次のready attemptを作る。旧attempt/snapshotを再開・上書きしない
3. 営業が担当を引き受け、理由と過去提出を読み、新しいprivate文案を保存・再提出する。同じ事務WorkItemへ新attemptを作る
4. 事務は再提出されたsnapshotを受領する。営業の差戻後private文案を、提出前の一覧・既知ID・回復操作から取得できない

理由は空白のみ不可・UTF-8で8KiB以下。固定sales/office、既存Work transaction・OCC・operation ledger/recovery、Document参照・React shellを再利用する。返却理由をAudit本文へコピーしない。新しい添付・role管理・Agent・Tauriは対象外。

## 実装と確認

- 既存migration0001と使用済みdefinition versionを変更しない。0002と新しい合成definition versionをfresh seedへ追加し、旧JSON/ledgerの読込とforward-only資格を保持する。既存DBをreset・定義付替えしない
- backend: 完了attemptを不変の履歴として保持し、stable WorkItemのcurrent attempt/OCCを進める。ReturnInstruction、return API/read API、再claim/再submitを追加
- frontend: 既存2画面に差戻確認、immutable理由/過去提出、新attempt private編集を追加。元画面の所有者を勝手に切り替えない
- 最小TDD: 旧snapshot/attempt不変、private非開示、stale revision/attempt・不正target拒否、同operation重複排除・変更payload拒否
- 既存実DB/2名browserの後半へ差戻1往復を追加し、同じGitHub Ubuntu、一時PostgreSQL、固定Chromium、画像非公開、cleanupで確認する。実行先・データ種類・外部送信・破壊操作は拡張しない
- 限定独立レビューと既存回帰、通常CIを通す。ローカルDB/listener/browser拒否は再試行しない。新しい検証監督frameworkは作らない
