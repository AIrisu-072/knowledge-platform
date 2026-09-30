# P3 Durable HyperGraph — build guard 独立再審査

- 判定: **GO for design freeze candidate**。下記 3 入力を合成した incremental base / target と GC の契約に、今回の範囲の P1 / P2 は残らない。PostgreSQL は暫定第一候補であり、製品選定や production qualification の GO ではない。
- 固定した入力 SHA-256: `p3-graph-design-revision-1.md` `ba9e8616d9dd8fc569280f20956c841929155366fa30246dadee0f0c73e8aa59`、`p3-graph-build-guard-amendment.md` `4f4597622f2053a7a782c5f0bf7704681c5542ed589d0b4fc3612ffa01595ab0`、`p3-graph-cleanup-order-correction.md` `e0754819b37b4fc7eaf9cf1104ec634ed15549f520df2e1f25d45b1d8951d691`。先行の `p3-graph-architecture-recheck.md` が残した build/GC 一件だけを再審査した。
- 方法: 静的な契約・SQL 順序照合と [PostgreSQL 公式 constraints](https://www.postgresql.org/docs/current/ddl-constraints.html#DDL-CONSTRAINTS-FK)、[explicit locking](https://www.postgresql.org/docs/current/explicit-locking.html)、[date/time functions](https://www.postgresql.org/docs/current/functions-datetime.html) の確認。実 DB 試験、migration、PoC、P7 接続、build / CI は実行していない。

## 一件の閉鎖判定

1. **guard の成立と batch gate。** 追補 60–62 行は最初の copy 前に Source lock 下で READY base の receipt を確定し、新規 BUILDING target と token / fence / DB expiry を一 transaction で登録する。各 copy / delta batch は base `FOR SHARE` と target `FOR UPDATE` を generation key 順に保持し、guard の token / fence / 有効期限、base READY receipt、target BUILDING を再確認する。copy 完了時には delta 前の target 全体を base digest / count と照合し、`copy_verified_at` 後にだけ delta を通す。snapshot 読取後、最終 DB clock 再確認までに期限切れとなった batch は rollback する契約である。
2. **lock 順と両 key の保護。** 追補 58–65 行は Source→generation key 順→guard→evaluation lease とし、copy / delta / validate が generation lock 後に Source lock を取らない。GC は current / 有効 evaluation lease に加えて base または target の有効 guard を拒否条件にする。guard を参照する両 generation の `ON DELETE RESTRICT` FK は、期限切れ後も cleanup まで物理削除を防ぐ。READY→publish の間も guard が残り、pointer CAS 成功と guard DELETE は同じ commit で可視化される。CAS 敗北は abort まで guard を保持する。
3. **FK と expiry / abort の削除順。** 元の追補 65 行は期限切れ target を guard より先に削除する記述で、`ON DELETE RESTRICT` と衝突していた。拘束する修正 3–6 行は Source・base / target generation・guard・lease を所定順に lock して current / pin / token / fence / expiry を再確認し、未公開・未 pin target の `DELETING` 遷移、**guard DELETE → participant → relation → resource → target generation DELETE** を一 transaction で行う。途中失敗は guard と target を共に rollback する。公開・pin された想定外の target は削除せず integrity failure とする。これで元の順序矛盾は閉じた。PostgreSQL の `RESTRICT` は参照先 DELETE の後まで検査を遅らせられないため、この順序は必要である。
4. **失効 handle と GC の収束。** 修正 6 行により guard と target は commit で同時に消え、その後だけ base の GC が可能になる。追補 65 行の旧 handle の renew / batch / READY 化 / publish は fail closed のまま。複数 guard が同じ base を参照する場合も、追補 63 行の「有効 guard を持つなら拒否」に従って全 guard の状態を判定する。未失効 guard、current、有効 pin のいずれかがあれば base を退役させない。

## 後続 gate の有限条件

設計上の追加修正要求はない。freeze / plan は上記 3 SHA の拘束関係を保持する。実装受入では次を別途実証する。

- 実 PostgreSQL で source / sorted generation / guard / lease の lock 順、DB role / trigger、READY 不変性、base と target の FK 保護を確認する。
- barrier 付きの copy↔GC、READY→publish↔GC、CAS 敗北、期限切れ後の stale handle、snapshot 読取後かつ最終 DB clock 再確認前の expiry を注入し、fail closed と全 transaction rollback を確認する。
- 修正 8 行どおり、期限切れ未公開 target の削除に SQLSTATE `23503` が出ないこと、子行 DELETE 失敗時は guard / target が共に残ること、cleanup commit 後だけ base GC が可能なことを確認する。

比較 PoC による backend 選定、P7 の Projection / lexical / Graph composition と実 runtime 接続、復旧・性能・security・exact-head qualification は未完了であり、本レビューの GO から推定しない。
