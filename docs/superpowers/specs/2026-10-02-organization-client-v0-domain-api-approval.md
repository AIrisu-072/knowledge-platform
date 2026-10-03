# Organization Client v0：Phase2の権限と凍結

日付：2026-10-02 UTC。状態：**PHASE2凍結済み / PHASE3は範囲内で承認済み**。

## 権限と固定入力

依頼者の元の要求§§0〜51、特に§§21,42,44,45,49〜51は、忠実な形式化と、最小限で非破壊のattempt model評価を許可しています。[Phase1権限記録](2026-10-02-organization-client-v0-product-ux-approval.md)は§50を引用し、受入済みPR43 H2に結び付けています。その凍結設計blob `a2901ccb866fc85b18301db27dd66aa629791201`は不変です。

これは、範囲限定の既存権限と独立技術レビューの記録です。それ以前には存在しなかったartifactを依頼者が新たにレビューしたと主張したり、元のSTOP条件を免除したりするものではありません。Phase3を続けるための新たな通常承認は不要です。

## 凍結対象

- 設計：[Domain/API/Authorization](2026-10-02-organization-client-v0-domain-api-design.md)
- 正確なblob：`9f68bf19eb9986eb1c78082704a5d38ff35e35af`
- 最終レビューcommit：`245b17ddf39a7ef88a29ce698d57340d79bb7ad6`
- 最終レビューtree：`d056e70a8be34c56d062f6f43138d70f8390d392`
- 12:59UTCの独立最終判定：**GO**、Critical/Important指摘なし

初回レビューのImportant指摘4件と、提案/最終修正は[レビュー記録](../execution/organization-client-v0-design-review.md)に保持しています。最終の全体再レビューでは、非公開artifact隔離/provider直接回避、context/progressの限定された可視性、generationに結び付いたnative read、明示的なrequester/executor/provider権限を検証しました。最後の限定レビューでは、引き継いだMCP起動preflightを、新しい利用ごとの検証として誤記していないことを確認しました。

設計のレビュー待ち見出しは過去の準備状態で、後のこのexact-blob記録が有効です。レビュー担当は実装や、その後のstatus/approval記録作業を、製品動作として検証してはいません。

## 選んだ最小構成と維持する制限

- 安定したWorkItemと不変の完了済みWorkAttempt、return時に因果関係を持つ新attempt、atomicで不変のhandoff membershipとnext-ready状態
- 現在のrole/assignment/delegation/provider検査、queue/context/private-detailの可視性分離、明示的operation/OCC回復
- 新しい非公開draft bytesは、認可されたgeneration storageの背後でWork所有を維持。共有Document参照は入力のまま。Work labelはDocument ACLを変えない。範囲限定のWork artifact-provider実装には独自の適格性確認がなお必要
- Evidence/Finding/HumanDecisionを分離。Agentは帰属を明示した範囲限定支援と構造化結果を持ち、Human判断をなりすまさない
- 最小要件を満たす全API設計、Human/Agent共通業務contract、論理Workspace、範囲限定broker/read handle。OS固有の証明は後続で実施

Phase3で、正確に2つのsource設計と、その状態/interaction/keyboard/accessibility対応を完了します。Tauri/現在の依存関係の適格性確認はPhase4、製品codeはPhase5、実合成受入はPhase6です。merge/close/deploy、本番migration/identity/data/認証情報、Search WIP変更、未検証Audit-storeへの依存は許可しません。

新たな検査：一意な51method/pathとoperationId設計、元の要求全52行、新しい相対リンクとplaceholder scan、正確なobject、空白、不変Phase1と引き継いだproduct/lock/workflow範囲。これらの文書検査では、新runtimeやhosted CIのPASSを主張しません。
