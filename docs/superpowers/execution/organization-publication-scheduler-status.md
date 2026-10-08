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

## 2026-10-08 14:00 UTC — test-only保存とDraft作成の確認待ち

- identity test-onlyをremote `b806644ca47534a82681c8b249bca660593b4e90` / tree `ca484c8763ce22e74c57c36d0338a372aab6c2b7` へ保存し、fetch後にlocal treeとの一致を確認した。
- Draft PR作成は承認確認で停止している。コード保存は成功したが、PR未作成のため通常CIは未開始。別の起動経路へ迂回しない。親担当がこのDraftの明示的な作成確認を依頼中。
- 実process受入のtest-only追補を準備した。既存PoCを残し、同じscenarioをOrganizationでも実行する。fixtureを本人に束縛したRepositoryで作成・予約・取消し、取消再送の監査1件、WORKINGと公開pointer null、再起動後の非公開を追加する。既存の競合・失権・不正主体・停止中非実行・監査帰属は両modeに適用する。
- 独立静的レビューで不要なtrait importを除き、5分TTLの境界と取消監査の本人・版・公開操作ID・executor不在の照合、再起動後CANCELLED保持を追加した。実API/SQLとの不一致は静的には見つかっていない。この追補のRust compile/実行は未確認。Node guard 4件とdiff検査は成功。製品sourceは引き続き未変更。
- 次のexact action: test-onlyの独立静的レビューを確認し同branchへ保存する。Draft作成の明示承認後にPRを作り、通常hostedの実REDを確認する。
