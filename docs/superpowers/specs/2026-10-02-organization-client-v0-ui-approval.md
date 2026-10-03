# Organization Client v0：Phase3の権限と凍結適格性確認

日付：2026-10-02 UTC。状態：**PHASE3凍結済み、証拠PACKETのレビュー/公開待ち**。

## 権限と順序に沿った入力

依頼者の元の要求§§0〜51、特に§§38〜42,49〜51は、正確に2つのSource Designを承認し、通常承認を取り直さずに忠実な形式化を進めることを許可しています。§50は次のとおりです。

> この内容を正式化するだけで、新しい重大semantic decisionを追加しない場合、この依頼文をapproval evidenceとしてDesign Approvalを作成し、そのまま次Phaseへ進んでよい。

[Phase1権限記録](2026-10-02-organization-client-v0-product-ux-approval.md)は、元の要求と受入済みPR43を前提として結び付けます。そのProduct/UX blob `a2901ccb866fc85b18301db27dd66aa629791201`と、[Phase2凍結Domain/API/Auth](2026-10-02-organization-client-v0-domain-api-approval.md)のblob `9f68bf19eb9986eb1c78082704a5d38ff35e35af`は不変です。Phase1/2の凍結と独立レビューは具体source作業に先行し、Phase3設計をbackend実装で代用していません。

これは既存の依頼者権限と独立技術検証の記録であり、依頼者が後の画像を自らレビューしたという新たな主張ではありません。新しい重大な業務上の意味判断は追加していません。元のSTOP条件は有効で、file命名、内部refactoring、test構造を追加gateにはしません。

## 正確なソースと範囲

- [具体UI意味仕様](2026-10-02-organization-client-v0-ui-design.md)：撮影/凍結した意味snapshot blob `3097853a51cf32b20f4e37c3fae4048d97e19d65`
- [PR48](https://github.com/AIrisu-072/knowledge-platform/pull/48)の撮影対象remote `e6bf24d8afa76a4aa7c66546bd963e4e1a90ffc8`、同等local `ef7f610f07b2bad4f37e42a9716d7988855770df`、tree `204a412ba40211ca052d81cdf79f2b8701c148bc`
- 正確に2つの入口`sales.html`と`office.html`で、source/CSS/10状態fixtureを共有。[最終証拠記録](../execution/organization-d2-visual-review-v2.md)は、runtime-snapshot入力8blob、README、修正後の全画像を固定
- Source/control独立GO：Critical/Importantの残存指摘なし。328/328検査と、独立した近接16caseがPASS
- 調整担当と独立レビュー担当による実原画像20枚のレビュー：**GO**、Important指摘2件を解消し、新しい必須regressionなし

意味仕様の保留見出しと、以前のsource/statusの説明は過去の準備状態です。このexact-blob権限/適格性記録は、ここで特定した正確な検証済みsourceに対して有効です。撮影対象source、初回の画面失敗判定、source設計の範囲を書き換えるものではありません。

凍結設計は、context中心の営業とqueue中心の事務、事務profileとしてのreview、workflow/rework状態としてのreturn、共通Context ModuleとしてのEvidence/Agentを形式化します。backend/private-draftと不変handoff contract、Finding/Evidence/HumanDecision分離、Human/Agent共通業務API、Documentのauthoritativeな意味と機能再利用、論理Workspace、local/shared resource境界、Search Platformの既存名称/architecture、単一window/合成identity/runtime-contract境界、本番延期を維持します。

## 最終の正確なソースの適格性確認

正確なheadの通常CI、D2、DSI、Sandboxと修正後captureはSUCCESSです。修正後の原画像20枚すべてをレビューしました。capture起動に伴うCI37035368187は**SUCCESS**で、16:56UTCに確認しました。正確なsourceはPhase3凍結の適格性を満たしました。この別文書packetには、Phase4開始前に独立した正確性レビューと、親担当による公開/適格性確認がなお必要です。新たな依頼者の通常受入要件は導入しません。

画面証拠は幅1440のfull-pageです。1280 geometry、keyboard/focus/hover/font、reduced motionは別のbrowser assertionです。Search本文、decision outcome、開いたdialog、動的blocked→Returnは撮影されていません。実Tauri、production React/業務/backend/identity/永続化、実Agent/Search受入は主張しません。正確な制限は最終証拠記録を参照してください。

## 次の作業順序と維持するSTOP条件

この証拠packetを独立レビューし、公開・適格性確認した後、元の承認に従いPhase4 Tauri v2 Runtime Qualificationへ進みます。現行の公式stable version/license/推移license/security、Windows10 Pro/WebView2、React/Webpack互換性を調査し、runtime実装やinstallの前に範囲限定の適格性確認design/planを作成/レビューします。既存Linux/backend/native-Windows policyを維持し、desktop固有の範囲改訂は明示してレビューし、global checkの回避にはしません。元§46の実runtime caseには固有の証拠が必要です。本番実装はPhase5、合成統合評価はPhase6です。

この証拠branchは撮影source tree上の文書のみの変更で、親担当はlocal parentを実remote `e6bf24d8afa76a4aa7c66546bd963e4e1a90ffc8`へ対応付けます。後に作成されるcommit/PRは証拠対象であり、撮影済み製品基準を置き換えません。新しい画像export、upload許可、保持延長、恒常的Action-license例外は作りません。PR43の受入済みH2は凍結したままで、C0の過去G9を遡って完了にはしません。Search PR40と独立した未検証Audit store/source作業は依存先でも、新たに適格となったものでもありません。この記録からmerge/close/deploy、本番migration/data/認証情報/identity接続、広範な依存installは許可されません。
