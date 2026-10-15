# 文書アクセス設定の結果不明・再読取回復を補修する

## 目的と範囲

PR97統合main `bba1d6dd45d93c5ad52e4a69debc9a3e77e5a8ab` / tree `84b0b0d48d3dfdc8b0a5b66ec53f9cc070aa6420` を基点に、既に通常詳細「アクセス」から使えるDocument ACLの回復品質をFolder ACLと同じ水準へ揃える。新操作の追加よりこの補修を優先する限定方針が承認済み。既存GET/PUT、既存主体と5権限、個別/継承の意味、文書内容は変えない。

現AccessTabはoperationIdと変更案をcomponent内だけで持ち、背景policy再取得・入力変更・画面往復でIDを失い得る。再試行時に現在のpolicyRevision/変更案からbodyを作り直し、成功後はpolicy queryだけを無効化する。この静的所見を実routeのREDで確定してから直す。

## 制約

- 新backend/OAS/生成SDK・新主体directory・Root操作・既読・原本構成編集・旧版取下げは追加しない。Tauri/新Workspace Runtime/共有Shell/Organization/Audit/Searchの機能は別担当で変更しない。
- 通常詳細で正規readできる対象Documentとserver manageAccess、正確なpolicy target/local revisionを使う。表示名や別URLから権限や主体を推測しない。新しい主体の追加機能、権限規則、継承の原子的保証を足さない。
- 操作ID・Document ID・mode・grants・expectedPolicyRevision・reasonを初回送信前に深く固定する。UNKNOWNではその要求だけを再送し、背景read・入力・往復で書き換えない。現在認可を失った後の403/404/競合で初回の失敗を確定しない。
- 成功receiptを現在policy読取と区別する。policy変更が同値なら既存no-op/replayとして扱う。serverの最終認可/OCCが正本であり、GETの一致から操作成功を作らない。
- Folder側の既存operation store/policy正規化/fresh read/失効パターンを必要最小限で再利用する。新しい汎用Runtime基盤を作らない。背景policyの変化を未送信draftの無言置換や新ID送信へ変えず、見直しを明示する。
- UNKNOWNや成功結果は同じアプリ内の画面往復後も確認できるようにし、自己の閲覧/管理権限喪失で通常の「アクセス」tabが消えた場合も固定要求の確認導線を保つ。ページ再読み込み・タブ終了を跨ぐ永続化は追加しない。
- 成功時は既存readだけを同期失効→cancel/resetし、古いpolicy・文書・比較・原本結果を再利用しない。別の固定操作/アップロードBlob/Organization選択providerを消さない。関連Folder/Document移動やACLの未確定要求を破棄しない。
- 実データのACLは変更せず、合成fixtureだけを使う。既存runner/画像/skip/timeoutを変えない。ローカルDB/socket/browser/Cargoは実行しない。PRは日本語で機能単位、mainへの統合は管理担当、実サーバー反映は所有者手動。

## 小さい実装と検証

1. 実DocumentDetail routeの現在AccessTabで、最初のPUT応答喪失→背景再取得または入力/往復→再送時にoperationId/bodyが変わる反例を観測する。fixture/mockingの不足による例外と、実際の契約違反REDを区別する。
2. QueryClient単位の固定storeと必要な回復表示を追加し、入力・現在readと固定要求を分離する。既存AccessTabのnormal GET/PUTと権限の意味を維持し、UNKNOWN後は同じbody/IDのみ再送できるようにする。必要なら専用componentへ抽出するが、transportをpresentationへ持ち込まない。
3. 保存中の編集・同tick二重送信、背景read、未送信のpolicy競合、同値no-op、確定拒否とUNKNOWN後拒否、不正receipt、別tab/別Document/一覧往復、自己失権後の回復、遅い結果・Blobと関連read失効、既存Folder要求/Blob/Organization保持をTDDで確認する。
4. 既存合成runtimeのDocument policy正常保存区間を最小拡張し、固定payload/receipt、正規read、閉じて戻る操作とHTTP再起動後の状態を確認する。実の応答喪失/自己失権を今回hostedで再現しなければ純粋反例との差を明示し、新proxyや検証基盤を加えない。
5. focused→全GUI/schema/型/build、既存runtime純粋/型/collection、独立reviewと限定再reviewを経て、同じ機能PRへ日本語手順/試験をまとめる。最新mainを保存前/統合前に確認し、remote exact head/treeと通常hosted/必須CIを終端まで検証する。

## 資格の扱い

PR97の合格と新mainのpostmerge資格、今回の補修資格は別に扱う。初期段階では本補修の実browser/画像資格はない。画像/macOS golden、本番Identity/TLS/対象PC、backup/restore、PostgreSQLプロセス再起動、HTTPに存在しない管理operation照会による回復は今回追加しない。
