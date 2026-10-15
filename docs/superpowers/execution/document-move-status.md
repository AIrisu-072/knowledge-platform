# 文書移動GUIの実行状況

## 2026-10-06 11:01 UTC

- PR87統合main `3448c51de9ceac4f74531c5ca841c0842d09d725` / tree `88e5ad3ac94d04aec9ab85a6390ed123edca2fc7` を基点とし、`feat/document-move-gui-20261006` で[小計画](../plans/2026-10-06-document-move.md)を進める。通常readで見える文書/移動先の既存API GUI化に限る。
- 前Folder移動はPR87最終 `8b4e2055` の全適用CI/実受入/DB/cleanup/artifact0確認後にmain統合。main自身の[CI `37447848469`](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37447848469)もattempt1の全13jobs/checks・Document18+5・Agent9・Organization操作/HTTP再起動/所有コンテナ削除・指定DB36/36・新衝突2件と既存拡張case・終端artifact0を確認した。ref/tree/両parentsも終端後に照合済み。
- workspaceの旧worktree/toolchain/ローカルログが見えなくなったため、公開mainのSHA/tree/両parentsをGitHubと照合し、公式hash一致のNode24.21.0/pnpm12.4.1と固定lock依存で独立worktreeを復旧した。旧ローカル証拠の復元や原因特定は主張しない。
- 文書移動GUI `b6b0444` は実RED後にAPI/固定要求/通常詳細と一覧回復を実装。独立reviewで移動先名/IDの型不正を採用するI1を外部DOM3反例から再現した。`6bb0b26` の2files限定補修は新反例22件のREDを閉じ、focused127件/3 suites・全GUI1089件/45 suites・schema/型・修正後build成功。独立再reviewはspec/品質GO。実runtime資格は未取得。
- 本人が読めない元Folder、T10終了文書、読めない移動先は今回のGUI入口の対象外。URLからID/権限を補完せず、backendの最終認可・OCCを維持する。機能docs/testsを同じPRにまとめ、結果だけのPRを作らない。

- 既存metadata2caseへの受入追加 `d4a6bc5` は、同権限Shared→Sandboxの移動1回とHTTP再起動後の元要求replayを扱う。送信前に開始・body完了したGETだけを、POST時に固定した有限記録から照合する。正式改訂/Version/全原本hash/本人・Agent readStateを保持し、既存metadata/未読/日時検査を維持。純粋66件・runtime型・MCP compile・収集18+5成功。次は組合せ独立レビュー、日本語Draft、同headの既存hosted実受入。
