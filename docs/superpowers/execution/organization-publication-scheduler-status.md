# Organization予約公開schedulerの実装状況

Status: ACTIVE

## 2026-10-08 13:52 UTC

- branch: `feat/organization-publication-scheduler-20261008`
- 最新main基点: `7f5dd26bb96d65f1dd478e644e9480d8666ebb7d`
- 承認済み範囲: [限定接続](../specs/2026-10-08-organization-publication-scheduler.md)
- 現在: test-only段階。6主体の既存adapter一致、provider分離、executor拒否、有効期限更新、実binaryのmode選択を追加。製品sourceは未変更。
- 実施済み: 旧基点で既存scheduler Node guard 4件成功。新headのRust、実DB、DSI、全CIは未実施。cloudにcargo/mise/Docker/psqlがないため既存hosted gateを用いる。
- 次のexact action: 同一Draftへtest-onlyを保存し、既存runtime selectorがOrganization modeを拒否する実REDを確認する。その後resolverだけを接続し、実process受入へOrganizationと取消を追加、独立レビュー、同head全CIの順に進む。
- 未完: 実RED/GREEN、取消・再起動・失権のOrganization実受入、独立レビュー、PR、main統合。mergeは親担当が調整する。
- Design Freezeとの差分: 承認したruntime modeの限定接続のみ。本番認証や権限拡張は含めない。
