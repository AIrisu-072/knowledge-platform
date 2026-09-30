# P4 Remote Source — 改訂設計の独立 architecture recheck

- 判定: **GO（この版の設計凍結へ進める）**。原レビューの P1 2件・P2 3件は閉じており、関連する新規 P1/P2 は認めない。
- 対象設計: `p4-remote-design-revision-1.md`、SHA-256 `f4112c7aa0cf7cbf61dca19c4beebcfb14a6578432ac45861644ab8ea616f9e9`。この SHA の文書に限る判定。
- 範囲: 承認済み Search / Discovery v0 設計、SD-T2/3/4/7、現行 `search-core` / `search-application` の Source・federation・Discovery・evidence・retention port との静的照合。実装、TDD、HTTP PoC、CI、本番接続の実施・成功判定ではない。

## 原指摘の閉鎖確認

1. **P1 / 同一 Source の generation:** 改訂 §1・§4 (`:9,129-139`) は Source/evaluation に sealed key を一つだけ与え、action の snapshot・scope・ACL revision・Resource version/digest を seal 前に照合する。list/hit/Claim/read の key を揃え、証明不能な追加 action は gap と新 evaluation に送る。現行 `CandidateFederator::validate_lists` (`crates/search-application/src/federation.rs:122-145`) と `PinnedSource.key` / `seen_resources` (`crates/search-application/src/discovery_service.rs:553-639`) に接続可能で、durable pin と S1 priority concat を変えない。
2. **P1 / actor と Source visibility:** 改訂 §2 (`:19-70`) は P4 の request-scoped entrypoint で trusted actor/evaluation/access handle を registry・routing・pin・network より前に照合し、全 tenant 横断の SourceId 一意性を起動時と registry 更新時に検査する。可視 Source だけで route を作り、未知・別 tenant・不可視 Required ID を同じ汎用 gap にし、途中取消時は Source 由来の結果・trace を除去する。現行の裸の `list_sources()` / `access_context: String` / Source ID 入り trace (`discovery_service.rs:167-179,506-519,721-839`) をそのまま公開しない実装契約になっている。
3. **P2 / ID なし candidate:** 改訂 §1・§5 (`:13,141-147`) は stable native ID のみ `QualifiedResource` へ入れ、ID なし hit は明示 `UnsupportedCoverage` gap、probe・binding・GET なしと定める。現行 `resource_ref: None` の qualification skip (`discovery_service.rs:612-640`) と `QualifiedResource.resource_ref` 必須 (`crates/search-core/src/discovery.rs:93-104`) に対して、qualified E2E を誤認しない。
4. **P2 / retention owner と expiry:** 改訂 §7 (`:159-173`) は全派生 store/handle の owner、lease、全 read/write と非同期 read 終了時の gate、取消・終了時の一括無効化を固定する。`NO_RETENTION` は evaluation と返却 stream の二段階 lease で success/error/disconnect/cancel/deadline を閉じ、`SESSION_ONLY` と expiry cache を別 owner とする。現行 `SessionWorkingSet` の無期限参照 (`crates/search-application/src/session.rs:35-56,115-155,199-228`) を利用可能と見なさず、既存 persistent gate (`crates/search-application/src/projection.rs:269-329`) を維持する。
5. **P2 / evidence 自己申告:** 改訂 §6 (`:151-157`) は provider JSON を untrusted hint に留め、registration grant、対象 version/digest、directness、citation chain、canonical lineage を検証した private provenance からだけ evidence role/origin を作る。同一 upstream の別ラベルを一票に畳み、未検証のものを direct support にしない。現行 core の role/origin 判定 (`crates/search-core/src/evidence.rs:57-69,219-242`) と `assemble_resource_claims` (`crates/search-application/src/evidence_resolution.rs:87-123`) の前で必要な信頼境界を設けている。

## 残る指摘

**なし。** Remote miss と Resource absence の分離、Source outage、同一 version の digest 衝突、current authorization、Source-local identity、retention は承認済み意味境界を弱めていない。設計に記載された port 配線、反例試験、実 TCP E2E、HTTP library selection/PoC は後続の実装・検証段階の受入条件であり、この静的設計 gate の既実施証拠ではない。
