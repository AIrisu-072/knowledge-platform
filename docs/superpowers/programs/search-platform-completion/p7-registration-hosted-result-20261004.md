# P7-02 登録・共有リースの合成DB結果：2026-10-04

## 確認した対象

PR #40の公開head `93a5781c733da7ede634e34c592579e034bc38ef`、tree `bc000c09db10c2ff8245651b7f3daea9c25e8cee` に対し、[CI 37211933903の専用ジョブ](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37211933903/job/111464710830)が成功しました。実checkoutはmerge commit `bc46f248c96c06942e3122ae1bc1c93bfd732c84` で、treeが一致しています。

Ubuntu 24.04、Rust 1.98.1、公式`postgres:18.6-bookworm`を使用しました。取得したimage digestは `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650` です。公開コードにある使い捨ての合成フィクスチャだけを使い、本番DBや実データには接続していません。

## 実行結果

15:12:31 UTCまでに、追加した次の対象がすべて成功しました。

| 対象 | 成功数 | 検証範囲 |
| --- | ---: | --- |
| `source_registration` | 18 | 完全集合・名前空間の分離、改訂/DTO/所有者、原子的rollback、commit中cancel、有限再試行、二つのoverflow |
| `source_lease` | 8 | 実DB6件と通信なし2件。2/4/8接続の排他、旧所有者の失効、登録変更、取得のcommitエラー時0claim、更新エラー |
| runtime `--lib` | 6 | 通信なしの共有ゲート・中断・世代stamp・SQL中断分類 |
| `coordination_migration` | 3 | 同じDBでのポインター/処理記録とSource行の前提 |
| `source_ownership_migration` | 6 | 独立migration台帳、恒久所有権、過去行の証明不足時の拒否 |
| `generation_schema` | 15 | 現在の世代/guard/pinのSQL制約、JSON null拒否、実roleの境界 |

新規対象の内訳は実DB24件と通信なし8件、既存スキーマは実DB24件です。同じジョブで、G07復旧2件とG08を含む既存Outbox対象61件も再度成功しました。件数はこのジョブの対象を表し、Search全体の完成率ではありません。

## 実証した意味と限界

登録の変更・削除・再有効化は古いleaseを失効させ、Source所有者や種別を別tenantへ再割当てしません。完全なホスト集合の再検査が失敗した場合は、部分更新を残しません。commit待機中のキャンセル後は既存leaseと新規取得を閉じ、両名前空間の再照合後に再開します。

取得/更新のエラーケースは、`RETURNING`後の遅延制約エラーを使っています。実ネットワークの応答喪失や、commitの成否が不明になる通信障害を再現したと主張しません。2/4/8は独立した接続数であり、OSプロセス数ではありません。実装前の時系列上の実DB REDを取得したという記録にもしていません。

[ローカル検証とAPI判断](p7-registration-lease-decision-20261004.md)および[ソース/ログの固定ハッシュ](p7-registration-lease-evidence-20261004.json)を保持します。既存generation SQLのJSON-null/GC権限の修正は、別の読み取り専用レビューでも確認しましたが、publisher・pin・GCの実装をそのSQLレビューだけで受入済みにはしません。

本番ホストの完全なinventory、起動許可、世代の内容保存とREADY/公開、pin/GC、検索APIと最終縦断は残っています。P7-06の世代登録・guardは別候補で進めます。15:27:50 UTCに通常CI全10ジョブの終端SUCCESSを確認し、[DSI PoC 37211933868](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37211933868)と[Sandbox 37211933852](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37211933852)もSUCCESSです。いずれも公開head `93a5781c733da7ede634e34c592579e034bc38ef` の結果であり、後続候補へ自動的に引き継ぎません。Domain migration番号9の統合衝突、以前のローカルソケット拒否、本番の手動適用という境界も維持します。
