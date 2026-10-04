# Searchのmain統合候補とRemote観測の検証記録

2026年10月4日16:43 UTCの状態。Search全体は**ACTIVE / WIP**である。

## 公開済みの根拠

PR #40のheadは `1571ee49af66defa6a0e2b738c81f299a2c0ef4f`、treeは `ca7ecabe4fe8732b12cdf80d9d43df2645a00ada`。CI `37215215461`、DSI PoC `37215215436`、Sandbox `37215215410` は終端SUCCESS。CIのcheckout `f956efbacf450eb761b3673eb54616155cdab62c` は同じtreeである。

Search専用job `111474215982` ではG07/G08と既存登録・lease・schema試験に加え、P7-06の実DB12件＋純粋4件、runtime純粋8件、非公開handleのcompile-fail doc2件が成功した。EVENT/MANUALの原子的なBUILDING登録、guard更新、期限切れ拒否、ロール境界を確認した。expiry試験のbarrierは開始位置を揃えるもので、実際のSQLロック待ちをまたいだ観測の証拠ではない。READY、Graph、世代公開、pin、GCの受入ではない。

## 今回の候補

main `9c90f383f2f88f5312d541aecb8eef8766bb1ffc` と上記Searchを統合する。ローカル統合commit `a1a2e4004bf3345e2eeaa16bf848535b73835aac` / tree `cfc5e995482b2a9f203ea917d471b4354768bbd8` に、独立レビュー済みP4-05の5ファイルをbyte不変で加えた。製品ソース基点は `2078f7adbf0bd1e35e21210ceace308c9b00b8e3` / tree `bcd2e39ac9f86cd463c121fe9c01c12c2a4a5e08`。本記録と現在状態の追記は、その後の説明だけの差分である。

[統合判断](../../../decisions/2026-10-04-search-main-migration-integration.md)に従い、Document9/10は不変、Outboxは本文不変で11へ配置する。旧Search9適用済みDBは自動変換せず停止する。[実環境STOP手順](../../../operations/search-main-migration-stop.md)を参照。既存本番に旧Search9がないとは確認できていない。

[P4-05](p4-05-remote-observation-20261004.md)はcatalog認証済みcontext、閉じたRemote操作・観測型、完全列挙の限定absence receiptを接続する。登録差替え、重複IDの誤pin、Debug漏出をREDから修正した。direct absenceの登録能力は未資格のためdirect missはUnknownのまま。実HTTP providerやdisclosureはまだ配線していない。

## 検証と次の作業

- 統合候補のapplication/core純粋350件＋doc8件、対象全targetのstrict Clippyは成功した。Rust1.98.1、既存cache、offline/locked、jobs2を使用した
- migration、OpenAPI、architectureの限定検証と独立レビューは[統合判断](../../../decisions/2026-10-04-search-main-migration-integration.md)に記録した。P4-05の独立レビューは残存Critical/Importantなし
- この新しい統合候補に対するhosted全CIと履歴DB4経路は未実行。旧headの成功を新候補へ流用しない
- 次はmainをbaseとする別Draftへ公開し、同じheadで公式PostgreSQLの新規・base8・Document9/10・旧Search9停止を検証する。PR #40と元の履歴は保持する。main mergeは結果を確認して別途調整する
- 本文reader資格・BodyUnitManifest、payload/索引、Remote世代seal/lease、検索API、世代公開/pin/GC、最終縦断が残る。後続source作業は進めるが、ローカルDB/socketの拒否を再試行しない。実環境への接続・導入は実施していない
