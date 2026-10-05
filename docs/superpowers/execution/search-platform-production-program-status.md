# Search Platform本番化プログラム — 状態

## 2026-10-06

**ACTIVE / WIP。** [計画](../programs/search-platform-production/plan.md)。基点はmain `ce8ed4f`、branchは `feat/search-platform-production-20261006`。

完了：A1。判定手段（selector）が無いSourceの候補や、別の主語についての主張は、Claimの判定に含めない。判定結果が一件も無い必須Claimは、候補があれば `unknown` として結果に残し、範囲内に判定できるSourceが無ければ `no_evaluating_source` の理由（公開コード `UNSUPPORTED_COVERAGE`）を付ける。`search-application` の全379件と、実DB/FS・実TCPの `server_e2e` 7件が成功した。複数SourceのDiscoverは `sufficient` を返す。

次の作業：B4（P7の永続世代を読む本番用のactor port factory）。
