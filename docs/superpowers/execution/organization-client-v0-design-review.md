# Organization Client v0：独立設計レビュー記録

## Phase1 Product/UX

最初の候補`83bcd53fea57e1843ab04488154731ada56121ff`、tree `9298269a6810fd2e075e9a63391fd8ee953d47f5`に対する独立判定は**GO**で、Critical/Important指摘はありません。レビュー担当は、引き継いだ凍結Document contract、2archetype/共有権限境界、元の範囲限定承認を確認しました。

編集上の修正でOrganizationalUnit/Tauri v2を明示し、既に必要なPhase4適格性確認checklistを再掲しました。正確なdesign blob `a2901ccb866fc85b18301db27dd66aa629791201`への独立再レビューは12:41UTCに**GO**でした。番号付きの元要求全52件、補助status、追加Active pointerも確認しました。相対リンク/空白、不変の以前のpointer内容の検査はPASSです。[Phase1権限](../specs/2026-10-02-organization-client-v0-product-ux-approval.md)に凍結を記録しています。製品/runtime/hosted testの証拠は推測しません。

## Phase2初回レビュー：NO-GO

候補`5dd56998e05fcb74620d3228d9e8f6f0eaeba638`、tree `6197c20d20e524d62ac804c649846359e48c0f9d`、design blob `d7eeb997c0f01b0cade56f79bfb5e28c68d85cd9`では、**Important**指摘4件、Critical指摘なしでした。初回Phase2凍結は**NO-GO**です。下記の位置は正確な元対象の行番号であり、後の修正済み行番号ではありません。

1. Providerに保持する非公開draft参照（188〜238,477〜478行）は、Document直接読取を防げません。既存`document_history.rs:151–215`はWork assignmentではなくDocument権限を使い、Document/folder ACL所有権は固定です。Work submitが失敗する前にpromotionすると、内容が公開され得ます。必要な対応はWork/local所有か証明済みprovider隔離、submit前の閲覧範囲拡大禁止、直接routeの拒否/reassignment/returnテストです。
2. Local read API（381〜420行）はfile識別子を返しますが、後のrangeをcontent generationへ結び付けません。in-place writeではinode同一性が残り得ます。必要な対応は、principal/device/window/contextへ結び付いた上限付きread handle/generation、安定したcapture、無効化、変更の反例です。
3. 営業の継続性（143〜194,320〜322,473〜475行）では、queueと非公開detailの間に、独立認可するcontext/progress/history projectionが不足していました。必要な対応は、別action、許可fieldの限定、count/filter/cursorのprivacyです。
4. AgentExecution（285〜305,571〜583行）は実executor/provider identityを結び付けず、requester権限との積集合も取りませんでした。既存MCP preflightは合成`agent-01`とは異なり`poc/poc-agent`を要求します。必要な対応は、帰属を追跡できる別identity、serverが確立する限定dispatch範囲、非対称権限/取消の拒否テストで、固定session検証を弱めないことです。

これらは承認済みの意味の範囲内の修正で、依頼者の通常承認gateを増やすものではありません。親担当も独立にprivate-provider回避を特定しました。

## Phase2修正候補：独立再レビュー待ち

候補`19d47c5c0fc555e13435018947d496b08585a6c7`、tree `585d30f3aaaab580b4d2106a047db9449d1a471c`、design blob `1c629ca51abae47532aaa60cf624a6c624d3c8c1`には、次の解消案を含みます。

- Work所有の非公開・不変content generation。共有sourceは明示的に入力のまま。local uploadはatomic handoffまで非公開で、共有権限のstorageと受取人への可視性を区別。Document権限は変更しない
- resource上限を持つ、generationで保護したopenRead/readFile/closeRead handle。安定したcaptureはPhase4でsame-inodeの安全性を証明するか、利用不可と報告する
- context.read/progress/history権限と限定projection。count/filter/cursorおよび機微なreason/bodyの除外を含む
- requester、executor、独立検証されたprovider identity。範囲限定dispatchには現在認可の積集合が必要。header/bodyでのprincipal選択や、固定`poc-human`/`poc-agent`への暗黙mappingは行わない

新たな文書範囲/空白検査はPASSです。再レビューは保留で、Phase2未凍結、Phase3未開始です。このレビュー期間に実装、install、browser、Rust、database、実Agent作業は行っていません。

## Phase2最終exact-blobレビュー：GO

修正済み19d47c5cの全体再レビューで、Important4件すべてを解消しました。編集上の正確性修正が1件求められました。変更していないMCPは起動時に固定Agent sessionを検証し、利用ごとの検証は新dispatch/provider adapterの責任です。最終commit `245b17ddf39a7ef88a29ce698d57340d79bb7ad6`、tree `d056e70a8be34c56d062f6f43138d70f8390d392`、design blob `9f68bf19eb9986eb1c78082704a5d38ff35e35af`の独立判定は12:59UTCに**GO**、Critical/Important指摘なしでした。exact-object/空白検査はPASSし、凍結Phase1は不変です。runtime/install/buildの証拠は推測しません。以前のNO-GO/保留記述は過去の状態で、この最終記録が完了するのは設計レビューのみです。[Phase2権限](../specs/2026-10-02-organization-client-v0-domain-api-approval.md)により、元§50に沿った順序のPhase3作業が許可されます。
