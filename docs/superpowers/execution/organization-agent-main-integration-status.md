# Organization合成Agent — main統合候補

2026-10-04 17:08 UTC。状態: **候補準備、統合headのCIは未取得**。

## 受入済み基点

- Agent: [PR60](https://github.com/AIrisu-072/knowledge-platform/pull/60) `48ae1bfd915119ae8625705474577f093d5d303c`、tree `a0a1a2e12b52c3f228b4f77a249fc38403b1abdd`
- [通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37218125682)、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37218125679)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37218125701) は全て成功。Rust878件PASS/7skip、GUI159件/19 suites、既存mock browser6件、artifact0件、D2は対象外skip
- [実runtime](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37218125682/job/111482735453) は実DB/transaction/initialize、2名の合成候補→人間判断→選択提出/private、両HTTP server再起動/保存結果復元/shutdown/cleanupまで成功。PostgreSQL自体の再起動、実LLM、本番Identity、Tauri資格とは区別する
- main: `9c90f383f2f88f5312d541aecb8eef8766bb1ffc`、共通祖先: `d383baccddd5081687b500f064f6fce195a24816`。Document/Organizationの過去受入文書を保持する

## 統合範囲

- 両基点をparentに残す。製品source・依存lock・migration・workflowはAgent受入treeのbytesと一致させる。main側の既存文書追加・訂正を保持する
- content conflictはactive.mdの先頭追記1件だけ。双方の原文履歴を残し、その上に最新の再開先を追加する。operations文書は自動統合を確認する
- Searchの進行中変更、別branchのcomplete実装、未資格Tauri、外部モデル/認証設定を追加しない
- 旧headの成功を新しい統合headの成功とは扱わない。新Draftの通常CIを完了まで確認する

次のexact action: 最終treeと製品差分ゼロを独立レビューし、main-base Draftへ公開。全CI後、親へexact head/副作用を返す。main ref更新・実サーバー反映はここで行わない。競合相手のmainが進んだ場合は再度正確なdiffを確認する。
