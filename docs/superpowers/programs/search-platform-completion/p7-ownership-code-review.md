# P7-01 Source ownership migration 独立コードレビュー

## 判定

**局所 GO** — 固定 SHA の `0002` migration と二つの focused test に、P7-01 の物理 ownership 境界を破る反例は見つからなかった。これは同一 PostgreSQL DB・trusted host proof を前提とする SQL schema の判定であり、production factory、実 role、durable reconcile、READY/publish の判定ではない。これらは未接続なので **production GO/READY は不可**。

対象は未コミットの `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff`。Foundation Draft PR #34 は OPEN/Draft、head は同じ `80a4796`、九つの hosted check は成功しているが、この dirty code は含まない。凍結設計、改訂 1、最終 plan と実装 receipt を照合した。

## 独立確認

| 入力 | 開始時 = 終了時 SHA-256 |
| --- | --- |
| Search `0002` | `a2abe81f2233267ff909af629f6571f68c1af36e52fd08670cf1f75daf8ff1ca` |
| ownership test | `29c4fcf84502113c5a153d576ec1865ce8ec5bdc897585244422d99bd6f83b41` |
| coordination test | `8a8150f93ffbd3756f3c978204049ebba5c2b33197abe0ecad29217da9fdaa51` |
| Search `0001` | `a9904c78307c5ae39099021569639f28ea08c99f7f65a9f60a51bf89bd1cc5a4` |
| `search-runtime/src/lib.rs` | `81dd87ab19013b00b760003febf4a46a96c57aecc30d992b7fd20ac32241f42e` |

`0001` と `lib.rs` は review 開始から変更なし。既存 executable の SHA-256 は ownership `9f0818945db51345c1344620c0d1a2123ccb97761154f2747b2e59841e1afb83`、coordination `7a8597f98960fa945e33246bdc5c0f3d0e4a76b37338bec0d25ef284aa6f9646`。mtime はそれぞれ `1790773523`、`1790773595` で SQL/test より新しく、ownership executable に現行 SQL の legacy refusal と owner guard 文言が埋め込まれていた。Cargo 再コンパイルはしていない。

使い捨て `postgres:18.6-bookworm` の cached image ID `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650` で executable を独立再実行した。`source_ownership_migration --test-threads=1` は **6/6、exit 0**、`coordination_migration --test-threads=1` は **3/3、exit 0**。対象二 Rust test の `rustfmt --check --edition 2024` も exit 0。writer receipt の strict Clippy exit 0 は参照したが再実行していない。workspace fmt は別 crate の未完成状態のため主張しない。

別の使い捨て PostgreSQL 18.6 DB に固定 `0001`、`0002` を適用し、ownership→Source の一 transaction INSERT は成功した。ACTIVE→TOMBSTONED の両行更新後、同一 SourceId を別 tenant／DOCUMENT として INSERT すると ownership PK で拒否された。同一 tenant／REMOTE のまま両行の activation を 2→3 に進める再有効化は成功し、最終行は `tenant-a|REMOTE|ACTIVE|3|true|3`。実行後に container は停止した。

## 確認した境界

- `0002:21-80,206-241`: 既存 Source 全件と current/receipt が参照する generation key に trusted proof の行を要求する。証明がない migration は transaction ごと失敗し、test は旧 pointer、receipt、Search ledger `0001` のみ、ownership 表不在を確認する。旧 receipt の bundle version を digest から補わない (`0002:200-204,368-388`)。
- `0002:85-174`: tenant key の非空・trim・Cc・UTF-8 256 bytes、kind、正数 revision/activation、`v1` JSONB object の 65,536 bytes 上限、lowercase SHA-256 形式、Document/Remote の別 revision/digest と単一 serial 行を拘束する。DB は DTO 内容と digest の正規対応、host snapshot の完全性を計算しない。これは P7-02 adapter の必要条件。
- `0002:243-329`: owner/kind/SourceId の UPDATE・owner DELETE を拒否し、registration/visibility/digest/state の変更には activation 増加を要求する。Source と ownership の owner・revision・activation・active は deferred trigger で同一 commit に一致する。新規 INSERT、通常の二行更新、tombstone／再有効化は妨げない。
- `0002:331-388`: namespace revision の逆行、同 revision で異 digest、serial 行 DELETE、新 receipt の version 欠落と旧 version 書換えを拒否する。Domain `_sqlx_migrations` は ownership test の前後で 9 件同一、Search `0001` checksum と `0002` の順序・再適用不変も同 test が確認した。

## 残る gate と反例

1. **実 role は未検証。** 別の使い捨て DB で migration/table owner `postgres` が `TRUNCATE search_source_ownership CASCADE` を実行すると、row trigger を通らず ownership、Source、receipt が `0|0|0` になった。これは owner/superuser 権限の事実であり、runtime role の bypass を証明するものではない。P7-03 の別接続 role/grant gate では app role に table ownership・`TRUNCATE`・trigger disable・DDL を与えず、読書き権限を限定して検証する必要がある。現在の `REVOKE ... FROM PUBLIC` (`0002:390-393`) だけを production role 証拠にしない。
2. **host proof の由来と全 key は SQL だけでは証明できない。** `0002` は host が作る public proof 表の存在と行整合を検査するが、proof が実際に durable host authority から来たか、current/receipt に現れない外部 generation が尽くされているかは判定できない (`0002:5-19,47-73`)。P7-02 の trusted host adapter、P7-03 の恒久 generation identity、P7-12 の startup 全 key/current scan が済むまで、旧 pointer を READY と解釈しない。
3. **lease fencing／unknown commit は別契約。** 一致した二行の registration 更新を schema が許すことは、lease token 失効・`fence_epoch` 増分、host revision の commit 直前再確認、移行や登録の応答不明時の再読を証明しない。P7-02/P7-12 で実接続・故障注入が必要。P3 native/Graph と P1/P3 二 digest mapping も未 qualified であり、publish は閉じたままにする。

次の exact action: P7-02 で実 SQL ledger/host authority/fence の real-PG regression、P7-03 で実 role と generation identity、P7-12 で旧 key/current の startup fail-closed をそれぞれ独立 review に掛ける。コード・spec・graph・journal・Cargo は本 review で変更していない。
