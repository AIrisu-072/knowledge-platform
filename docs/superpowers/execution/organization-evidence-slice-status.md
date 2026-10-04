# Organization Browser PoC — 根拠・候補・人間判断の状況

## 2026-10-04 09:55 UTC — 初回実DB成功・browser失敗の局所修正

[PR57](https://github.com/AIrisu-072/knowledge-platform/pull/57) source `5d65b3b1a2cb8a2e660757b4c0b540ea95ff2b9c` の[初回通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37192481161)はOrganization browser journeyで失敗した。build・実PostgreSQL接続・拡張transaction試験・初期化は成功、再起動/persistence/正常shutdown段階は未到達。失敗後cleanupは実装上finallyで試みるが、このrunの公開logにはその結果が無く、確認済みとしない。Document実runtime回帰、DSI、Sandboxと他の通常jobは成功（Rust856 PASS/7 skip、GUI127件、既存mock browser6件）。

初回の詳細な失敗行は一時workspaceだけに残り、公開artifactは無い。source調査では、selectを内包する3labelに選択肢文字列が混ざり、実Playwrightのexact label照合で全3件が不一致になる欠陥を純粋DOMで再現した。表示labelと同じ明示aria-labelを3属性だけ追加し、lock済Playwright1.63.0の実getElementLabels/createTextMatcherによる恒久試験をRED→GREENにした。E2E条件は緩めていない。この再現欠陥と、初回hostedの実際の停止行を確認したという主張は区別する。

標準Playwright JSONは同じprivate run directoryにだけ保存し、失敗時は有限phase/test/source名・座標・status/category/matcherだけをstdoutへ出す。本文・実値・URL・資格情報・raw message/stack・画像/trace/添付は出さず、診断不能でも元の失敗を維持する。既存cleanupの有限結果だけも最終stdoutへ出す。新workflow/依存・権限・実行先は追加していない。

局所修正のGUI128件/19 suites、純粋Organization runner11件、application/runtime型、schema/build、差分確認PASS（既存Webpack advisory3件）。sourceレビューでCritical/Important所見なし、exact sourceの最終照合後に同じhosted条件で再確認する。ローカルDB/listener/browserは実行していない。

---

2026-10-04 09:05 UTC。状態: **最小実装・ローカル確認PASS・限定独立レビューGO／新hosted実証待ち**。

- 基点: [PR56](https://github.com/AIrisu-072/knowledge-platform/pull/56) remote `cf28175d9b2467afd7225fa4f92f1d7a801d4002`、tree `59a1d299c45d6ffedf2692355c2ebf3f50d4cedf`。独立branch `feat/organization-evidence-slice` で作業し、受入headは保持する
- 前headの[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37187521089)、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37187521117)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37187521128)は成功。Rust836 PASS/7 skip、GUI100件、既存mock browser6件、Document実runtimeとOrganizationの差戻/再提出・旧履歴不変・両HTTP server再起動後復元・一時DB/container cleanupが成功した。PostgreSQL process再起動やTauri/native資格は含まない
- 今回は[短い計画](../plans/2026-10-04-organization-evidence-slice.md)に従い、人間がDocumentの改訂/原本参照を登録し、候補と採用/修正/却下を別recordとして保存する。固定2名・同じ使い捨てDB/Chromium・画像非公開を維持する
- Work transaction/ledgerとDocumentの現在認可、既存Context Surfaceを再利用する。source checkとWork transactionをcross-provider atomicと称さない。実Agent/model外部送信・新しい認可方式・汎用frameworkは追加しない

## 実装とローカル確認

- 人間起点のreference-only Evidence、immutable Finding、採用/修正/却下のappend-only HumanDecision、凍結8 API、明示選択のhandoff membershipを実装。Document Revision/History serviceの小さいadapterを通し、正確な改訂/版/原本と現在の認可を確認する
- 営業型・事務型の既存「根拠」moduleに登録・候補・判断・原本確認を接続した。判断はworkflow操作を自動実行しない。未選択recordと新attemptの作業はprivateのまま
- Work35＋Organization17の純粋試験、計52 PASS。既存実PostgreSQL試験1件を拡張してcompile済み、ローカルでは明示ignoredで未実行。source/provider checkはWork lock外、同operation重複排除・unknown回復・private非開示を確認した
- GUI19 suites/127 PASS。application/runtime TypeScript、validator/generated schema freshness、production build、OpenAPI lint、architecture、fmt/対象Clippy、差分確認PASS。既存Webpack advisory3件は維持。Playwrightは既存journey1件＋persistence1件の収集のみ確認
- 限定レビューで、可視record増加後に一覧が取得不能となる点と、source/根拠503後の再読込不足を修正。各可視collectionは16件まで、実JSON byte上限を保存前に確認する。default50/limit16–100、cursor未対応の完全集合PoC profileを明示し、記録を切り捨てない。再読込は同じattempt/revisionの根拠・Document source queryも再取得する
- 現在attemptと実担当/責任が残るCompletedでは本人readonly閲覧を継続。次attemptへ置換された旧privateは非開示、過去snapshotの明示選択recordは現在の責任とsource権限で閲覧する

依存・lock・既存Document source・migration0001/0002・既存runtime harness/workflowは変更していない。旧record/ledgerを読め、旧command digestを保持する。新schemaで旧snapshotを返す際に空の参照配列が付くことはあり、旧workflow JSONのbyte-for-byte一致を主張しない。既存本文・ID・revision・membership値は書き換えない。

限定独立レビューはstaged tree `5d1c1dcfa07cf0bb2ed1bd85b41f7eaca190b777` に対してGO、Critical/Importantの残りなし。上記2件を修正して再確認した。限定staged差分のGitleaksも所見なし。これはsourceレビューであり、実runtime受入ではない。

次のexact action: exact commit/treeを親へ渡し、別Draftの既存通常CIとOrganization専用実DB/2名browserで登録→判断→選択提出→差戻後の分離→両HTTP server再起動後復元→cleanupを確認する。今回の追加経路はまだ実runtime未実行。ローカルDB/listener/browser拒否は再試行していない。
