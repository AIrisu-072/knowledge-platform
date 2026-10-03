# Organization Client v0：Phase1の権限と凍結

日付：2026-10-02 UTC。状態：**PHASE1凍結済み / PHASE2は範囲内で承認済み**。

## 依頼者による権限

依頼者の元のOrganization Client要求§§0〜51は、product、UX、resource、authority、runtimeの境界を明示的に定めています。§50は次のとおりです。

> この内容を正式化するだけで、新しい重大semantic decisionを追加しない場合、この依頼文をapproval evidenceとしてDesign Approvalを作成し、そのまま次Phaseへ進んでよい。

[PR43の依頼者受入記録](https://github.com/AIrisu-072/knowledge-platform/pull/43#issuecomment-5952167525)により、2026-10-02 12:16UTCに§0の前提が満たされました。正確なsourceは`6103e4d4e3bb0d45ba03e1d2935492de7f11394a`、treeは`f2e13eee0d7e1bfa71952c1da52a72cecb65fc9e`です。元の0〜51全節を原文で受け取り、過去の要約から推測せずに読みました。[要求対応表](../execution/organization-client-v0-requirements-map.md)で番号付き全52節を追跡できます。

これは既存の限定権限の記録であり、依頼者が新しく作られたGit blobを別途レビューしたという主張ではありません。忠実に進めるための追加の通常承認は不要です。新しい重大な意味変更と元のSTOP条件は、この許可の範囲外です。

## 凍結対象と独立レビュー

- Phase1設計：[Product/UX](2026-10-02-organization-client-v0-product-ux-design.md)
- 正確な設計blob：`a2901ccb866fc85b18301db27dd66aa629791201`
- 最初の独立レビュー：候補`83bcd53fea57e1843ab04488154731ada56121ff`、tree `9298269a6810fd2e075e9a63391fd8ee953d47f5`でGO。Critical/Important指摘なし
- 編集上の修正2件で、OrganizationalUnitとTauri v2の用語を明示し、既に要求された後続Phase4の適格性確認項目を列挙
- 正確なblobの再レビュー：12:41UTCに上記凍結blobへGO。新しい意味変更やCritical/Important指摘なし。要求対応表、status、追加Active pointerもレビューし、全52行と相対リンクを確認
- 設計に元からあるレビュー待ち見出しは準備時の状態です。後のこのexact-blob記録が有効な凍結権限です。過去の承認を作ったことにするために凍結設計を書き換えてはいけません

## 範囲と次の境界

正確に2archetypeと1つの作業権限、Tasksを初期画面とする3つの主navigation、Evidence/Finding/HumanDecisionの分離、transcriptに権限を持たせない横断的なcontextual Agent、roleに基づく責任と非公開作業、不変handoff/return、local/sharedを分離する論理Workspace、DocumentとSearchの不変境界、同じReactを使うdesktop優先runtime、単一window/sidecarなし、合成identityと本番identityの延期を対象とします。

Phase2は、明示的に委任された最小rework modelの評価を含め、Domain/API/Authorizationを形式化できます。その独立凍結後にPhase3の具体source設計を行います。この承認はTauri/依存関係を検証せず、Phase4より前の実装を許可せず、Linux/backend policyを変えず、Search/Audit WIPを検証せず、PRのmerge/closeや本番操作/認証情報利用を許可しません。

新たな検証は文書のみです。正確なsource/report識別子と履歴、範囲、要求全52行、相対リンク、placeholder scan、空白を確認しています。新worktreeのAPIテストはassertion前に環境BLOCKEDのままで、新たな全runtime/hosted検証の成功は主張しません。
