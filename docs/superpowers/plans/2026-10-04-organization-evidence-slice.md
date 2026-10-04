# Organization Browser PoC — 根拠・候補・人間判断の最小slice

## 既存設計・継続範囲

所有者の2026-10-04「続けてください」に基づくBrowser先行の継続。基点は実証済み[PR56](https://github.com/AIrisu-072/knowledge-platform/pull/56) `cf28175d9b2467afd7225fa4f92f1d7a801d4002`。凍結[Domain/API§6–8・13–14](../specs/2026-10-02-organization-client-v0-domain-api-design.md)、[Product/UX§8](../specs/2026-10-02-organization-client-v0-product-ux-design.md)、[UI§6–7](../specs/2026-10-02-organization-client-v0-ui-design.md)を実装し、再設計しない。

目的は、営業・事務が担当中のタスクで既存Documentを根拠として登録し、候補を作り、採用・修正・却下を別記録で残して引継げること。固定2名、同じ一時DB/Chromium、画像非公開の既存hosted検証を延長する。Agent/model外部実行、Search、native、添付、production identity、新taxonomyは追加しない。

## 最小操作と保存契約

1. 現在の共有入力Documentの公開版に属する改訂と実contentItem/representationを既存APIから選択。人間記載の該当箇所を添え、reference-only Evidenceを登録する。原本本文を複製せず、fragment omissionは`not_retained`、coverageは`unknown`。公開版以外の新規登録はこのsliceでは未対応と表示する
2. 1件以上の認可されたEvidence revisionを選び、immutable Findingの候補文を保存。候補と原本の事実を混同しない
3. exact Finding revisionに採用・修正・却下を記録。修正時は採用文を必須とし、元候補/根拠/過去判断を上書きしない。実Human actor/acting assignment/attemptはserverで確定する
4. 提出確認で共有するEvidence/Finding/Decisionを明示選択。依存する根拠と候補を含む不変revision集合だけを既存handoffへpinする。未選択や新attemptの記録はprivateのまま。人間判断は自動的にsubmit/returnを実行しない

officeの既存`canEdit`は文案編集用のままにし、根拠登録/候補登録/判断用のserver-derived capabilityを別に設ける。営業型・事務型は同じContext Surface moduleを使う。

## 実装と必要最小確認

- [x] backend: `work-domain`の既存aggregateへ3記録・revision参照・閉じた3commandを追加。`work-application`の小さいsource認可port、`work-repository-postgres`の既存transaction/OCC/ledger/staging、`work-api-http`の凍結8操作を接続。既存migrationを変更せず追加migrationでactionを拡張する
- [x] Document接続: `organization-server`のadapterから既存Revision/History read serviceを使用。登録はPublished、保持した正確な参照の読出しはHistory。別Document・異なるrevision/version・不正locatorを拒否。Work scope確認後にprovider preflightし、Work lock内はscope/revision/参照集合と短命server receiptのfreshnessだけを再確認。本文返却/replayにも現在のprovider認可を適用する。これはcross-provider atomic transactionの保証ではない
- [x] UI: `TaskHomePage`の既存「根拠」placeholderを独立した小さいmoduleに置換。既存Document API、operation recovery、attempt-keyed cacheを再利用。候補/根拠/判断の分離、確認cancel、UTF-8 8KiB、unknownの同ID回復、権限喪失やtask切替後の内容消去を確認する
- [x] 最小TDD: private list/direct/recovery拒否、cross-context/attempt参照拒否、modified必須文、旧record不変、明示選択と依存closure、OCC・same-operation再送/変更payload拒否、source不一致/現在認可拒否
- [ ] 既存ignored PostgreSQL試験と2名journey/persistenceを延長し、実保存・採用/修正/却下・選択提出・private非開示・両HTTP server再起動後復元・cleanupをhostedで確認。新runner/framework/workflow/依存は追加しない
- [ ] 限定独立レビュー、既存Document回帰、通常CIをexact headで確認。ローカルDB/listener/browser拒否は再試行しない。公開は別Draft候補を親へ渡す

claim/reasonは既存凍結のUTF-8 8KiB、参照選択は100以下、body/responseは1MiB以下を守る。未対応provider/保持fragment/細粒度line locatorを自動推定しない。

## PoCの有界collection profile

限定レビューで合法record増加後に一覧全体が取得不能となる点を確認したため、各取得対象の現在試行＋受領済みEvidence/Finding、対象Findingの可視HumanDecisionを各16件までに制限する。上限で新規保存を拒否し、既存recordを隠したり切り捨てない。このsliceのcollection APIは完全集合の1pageだけを返し、limit省略時50・指定は16–100、cursorは未対応としてprovider呼出し前に明示拒否する。Frozen API全体のkeyset pagination完成は主張しない。8KiB本文上限・可視集合上限・JSON escape後の実serialized collection byte上限を組み合わせ、合法write後に既存一覧が1MiBを超えて読めなくならないことを試験する。

現在attemptと実担当/責任が残る間はCompletedでも本人のprivate recordをreadonlyで閲覧・回復できる。登録はActiveのみ。新attemptへ置換された旧private未選択recordはlist/direct/recoveryから非開示とし、過去snapshotの明示選択recordは現在のsnapshot認可とprovider確認に基づいて閲覧する。既存WorkArtifactと同じ境界を保つ。
