# Organization Finding取得失敗の診断

## 目的と境界

Organizationの合成runtimeで、再起動後のFinding取得が503になる原因を切り分ける。取得のHTTP応答、認可、5秒の開示鮮度、元の受入assertionは変更しない。待機延長、自動再送、原因修正、実環境への配備はこの変更に含めない。

Finding一覧／単体取得の失敗時だけ、閉じた操作・処理段階・依存分類・SQLエラー分類・経過時間を記録する。識別子、URL、本文、資格情報、生のエラー、SQL文、SQLエラーメッセージは記録しない。監査イベントの代替ではなく、障害切り分け用の運用診断である。

## 現在確認できたこと

- main `dba816874fe5703254b9b6d67c85e1547a8b6da2` のCI `37756243501` はDocument受入22段階が成功し、Organizationの再起動後persistenceでFinding GET503となった。
- PR108の旧失敗ログにも同じ閉じた失敗分類がある。PR111の統合前受入ではOrganization全段階が成功している。この成功は原因解決の証拠ではない。
- restartは両processの `/health/ready` 成功を待ち、persistenceはsession・task・snapshot等の取得を通過してからFindingを検証する。
- Finding取得はWork DB、agent生成元、Document根拠の現行権限、authority再確認、5秒の開示鮮度に依存する。既存503だけでは失敗した分岐を特定できない。

## 実行と解釈

Linuxの既存 `mise run organization:poc:runtime` を使う。所有する一時PostgreSQLと合成利用者だけが対象で、実データ・実アカウントは扱わない。Macは配備対象PCではなく、Linux worker／PDFiumを要求する本受入をこのMacで合格扱いしない。

ハーネスは受入が失敗した場合だけ、所有processから許可した診断を抽出する。欠落・過大・不正・未知の入力は観測不能として扱い、元の試験失敗を保つ。安全な診断を得られない場合も原因を推測して合格にしない。

backendの固定prefixは `KP_FINDING_DIAGNOSTIC`。許可するJSONは `operation`、`phase`、`dependency`、`sql_class`、`failure`、`elapsed_ms` の6項目だけで、文字列は閉じた列挙、経過時間はuint32上限を持つ整数である。tokioは既存workspace依存をdevから通常依存へ移すだけで、新しいpackageやlock更新はない。

harnessで公開するのは、元のbrowser失敗がFinding GET503だった場合だけ。現在の試験段階の開始後・生存中の所有processに限定し、期待404を除く503分類だけを抽出する。1件は `single`、複数は `ambiguous`、上限超過は `overflow`、欠落は `none` として区別する。単一記録でも要求IDで相関した証拠ではないため、観測範囲との関連を超えて原因を断定しない。

上限は6process・段階内出力64KiB・128行・診断1行512byte・診断4件。過大入力を切り捨てて都合のよい末尾だけを選ばない。成功時、他のendpointの失敗、期待404、前の試験段階の記録は公開対象外である。

診断の処理段階は失敗位置の絞り込みに使う。SQLエラー分類や期限超過を実際に観測するまでは、それを原因と断定しない。runtimeが成功しただけの場合は「今回再現せず」と記録し、原因修正済みとはしない。観測後の最小修正案は別途確認する。

実測された `agent/agent` の11ms失敗を受け、DocumentAgentSource内部の失敗時に `KP_DOCUMENT_AGENT_DIAGNOSTIC` を追加する。項目は `identity`、`phase`、`failure`、`elapsed_ms` の4つだけ。identityはrequester/provider、phaseはidentity/revision/files、failureは固定されたidentity・timeout・ApplicationError分類で、error内の本文や識別子を受け取る出力APIは持たない。既存404へ写像されるsource_mismatch/forbidden/not_found/staleはharnessの503観測から除外する。CursorStaleとStaleComparisonInputは既存503のままconflictへ分類する。

追加記録とWork記録が両方ある場合は `ambiguous` となる。連続する段階の観測として読み、同一要求の証明や最後の記録だけによる原因断定に使用しない。内部ApplicationErrorの分類が判明しても、DocumentのSQLSTATEやSQL実行箇所まで分かった証拠とは扱わない。認可の順序、結果の写像、remainingと5秒の最小値によるdeadline、成功時の無出力、元の受入assertionを保持する。

## 他担当の変更

PR112が変更中の `apps/document-web/e2e-organization/support.ts` には触れない。Directoryのbrowser-diagnostics追記とloadのDocument runtime runner変更を保持する。CI workflow、production設定、バックアップ、TLS、永続アクセスは変更しない。
