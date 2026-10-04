# P7-06 完全構築登録の最小候補と検証範囲

2026-10-04。**実装候補 / 実DB検証待ち。** 凍結済み計画のP7-06、共有永続化改訂1の§1〜3、P6のSearch完了ロール方針を対象とする。前提のP7-02実結果は[別記録](p7-registration-hosted-result-20261004.md)にあり、今回の実DB合格へ流用しない。

## 実装と判断

- `PgSourceRegistrationLedger::generation_registrar`を唯一の公開構成入口とし、既存poolと同じ`CurrentGate`を渡す。両登録名前空間の確認が必要で、登録応答不明・キャンセル時に閉じたgateを、旧ACTIVE行や発行済みguardで迂回できない。操作開始、commit直前、成功・失敗の応答確認後でstampを検査する。閉鎖は`StoreUnknown`、DBの正確なbindingや期限の不一致は`Lost`とする
- EVENTは実outbox行→固定Source、MANUALはSourceからロックする。EVENTの経路は構成時のDocument登録に固定し、実行時のpayloadからSourceを選ばない。実outboxのevent ID・type・aggregate route/ID・発生時刻・lease token・期限と、Source owner token・epoch・期限を照合する
- 所有者・登録DTO/digest・改訂番号・activation・保持条件を照合し、`build_fence_seq`の一回の加算、恒久identity、FULL/BUILDING target、同じtoken/fenceのguardを一接続・一transactionで保存する。衝突・guard INSERT失敗・counter overflowは全rollback。DBがtransaction中止を証明する`40001`/`40P01`/`55P03`だけを最大3回再試行し、応答不明のcommitを再試行しない
- EVENT/MANUALは別の非公開fieldを持つ型であり、公開constructor・Deserialize・相互変換を持たない。renewはSource→target→guardの順でロック後にDB時計を再評価し、保存manifest・snapshot・origin・event/epoch・token/fenceを再照合する。期限切れguardを延長・再発行しない
- この初期FULL入口は`PersistentResource`だけを許可する。`PersistentDiscoveryMetadata`は検証済みmetadata-only経路が未配線、`CacheWithExpiry`は成果物の期限設計が未配線のため、この入口では拒否する。metadataの永続保持を一般に禁止する新方針ではない。NoRetention/SessionOnlyの内容は永続化しない
- TTLは正の整数microsecond、最大120秒とした。DBの丸めによる0秒化や無制限leaseを避け、更新で継続できる最小の実装判断であり、原設計への新たなowner承認を主張しない。lock timeoutは2秒、statement timeoutは5秒
- snapshot一致は登録参照とmanifestのbinding検査であり、本文再読、成果物のdigest再計算、READY証明ではない。manifest全fieldを`dto_version=v1`のJSON envelopeへ保存する。P7-04以降はこの保存形を整合させる必要がある
- Search完了roleにoutboxの`SELECT`と行ロック用`UPDATE(lease_token)`だけを追加した。adapterはoutboxのどの配送列にも書かない。Search単体のスキーマ検査DBにはDomain表がないため、その場合は付与を保留する。EVENTの構成ではDomain移行後にrolesを適用する。既存配備の過大な権限を除去・監査したとは主張しない

変更はruntimeの登録/guard 2モジュール、module export、既存ledgerのfactory、role付与、有限の試験と本記録だけ。migration、依存関係、Graph、READY成功、公開、pin、GCは追加していない。

## この候補で取得した検証

Rust 1.98.1、既存offline cache、locked、jobs=2、debug=0、incremental=0。Cargo共有枠を直列化し、開始時の空き1536MiB下限を検査した。

| 検査 | 結果 |
| --- | --- |
| 初期TTL関数だけを同梱した`rustc --test` | 有効TTL拒否の意味的RED: 1 fail / 1 pass、exit 101。その関数修正後2 pass / exit 0 |
| `cargo test -p search-runtime --offline --locked --no-run` | 全runtime test targetのコンパイル成功、exit 0 |
| `cargo test -p search-runtime --offline --locked --test full_guard pure_ -- --test-threads=1` | 通信なし4/4、exit 0 |
| `cargo test -p search-runtime --offline --locked --lib` | 純粋8/8（既存6とTTL2）、exit 0 |
| `cargo test -p search-runtime --offline --locked --doc` | 私有fieldとMANUAL→EVENT型誤用のcompile-fail 2/2、exit 0 |
| `cargo clippy -p search-runtime --offline --locked --lib --tests -- -D warnings` | 最終候補exit 0。途中の入れ子if警告1件を修正 |
| 変更Rustのrustfmt / `git diff --check` | 成功 |

`tests/full_guard.rs`には実DB用12試験を定義・コンパイルした。対象はEVENT/MANUAL登録、誤経路/イベント/フェンス、期限切れ、登録変更、guard INSERT失敗、キー衝突、overflow、独立接続と期限境界バリア、再発行拒否、並行fence加算、共有gate閉鎖、実LOGINロールの成功と禁止DML。正のREADYを作る試験はなく、期限切れREADYの拒否だけを検査する。 期限境界のbarrierはrenewの開始を揃えるが、実SQLがロック待機へ入ったことまでは観測しない。成功しても、待機中の期限横断を決定的に実証したとは扱わず、期限切れ後の拒否として記録する。

**今回この12件を実行していない。** 実DBの時系列RED/GREEN、SQLロールの実動作合格、workspace全テスト・総合gate合格を主張しない。ローカルDB/Docker/socket/listenerを起動・再試行していない。純粋REDはTTL境界だけの証拠であり、SQL transactionのREDではない。

## 次の実行

独立source review後、正確なDraft公開treeに固定した公式PostgreSQLの合成hosted環境で、`cargo test -p search-runtime --locked --test full_guard -- --test-threads=1`と既存runtime回帰を実行し、実結果を別記録へ追加する。Graphの本番資格、READY/公開/pin/GC、Source本文snapshotの再読・全bundle再計算、全体受入は引き続き未完了。マージ・デプロイは含まない。
