# SearchとDocumentを同じDBへ更新する前のSTOP条件

本書は候補の確認・停止手順であり、実環境へのmigration実行許可ではない。[統合判断](../decisions/2026-10-04-search-main-migration-integration.md)はDocument9/10を保持し、SQL本文が同一のOutboxを11へ配置する。所有者の既存DBに旧Search9がないことは確認できていない。

## 初めに止める条件

次のいずれかに当たる場合、統合版の `migrate`、初期化、seed、通常起動へ進まない。

- 対象DBとstorageの所有者・用途・正確な現在版、適用台帳を特定できない
- 既存schemaがあるのに `_sqlx_migrations` がない。台帳を新規作成して既存schemaを正規履歴と推測しない
- 台帳のversion9が旧Search `outbox delivery v0`、または下記のOutbox SHA-384に一致する
- 失敗行、欠落、未知version、同じ番号のchecksum不一致、適用の成否不明がある
- 新candidateの同一headで合成DB履歴試験・全適用CIが成功していない
- 書込み停止、DB/storage/設定/releaseの一組のbackup、別DB・別storageでの復元検証を完了していない

公開済みbranch名やversionの最大値だけでは履歴を判断できない。読み取り権限と接続先を所有者が確認したうえで、最初に `SELECT to_regclass('public._sqlx_migrations');` を調べる。台帳が存在する場合だけ、以下を読み取る。接続文字列・認証情報・実データをrepositoryへ保存しない。

```sql
SELECT version, description, success, encode(checksum, 'hex') AS checksum_sha384
FROM _sqlx_migrations
ORDER BY version;
```

必要なのは全行と配布する全migrationのchecksum照合である。分類に使う特に重要な値は次のとおり。

| 正規ファイル | SQLx SHA-384 |
| --- | --- |
| Document9 | `e28d223ddc113dda166d3e45204542175317e6a85ec136a3a586bf2ded42beee9a11207f0d82bb3162eed8b74c0ecf53` |
| Document10 | `7e2c6d485f986ae0fb93aa8dc4ea2ea5f7cfd3a85c1299e35740c68f290ee852a76919b35b20e79b3dd42195727d5a16` |
| 旧Search9／統合後Outbox11のSQL本文 | `df0d246c1b3ab0c344a18c13936630a8d618fad2ae21649c9a13a16ac7d77cca75ca201b00f2b7acc956f93549b84783` |

## 旧Search9が見つかった場合

**ここで停止する。統合版のmigrationを試しに流さない。** 元の台帳・DB・storage・releaseを保持し、その履歴専用の移行案と復元検証を別途決める。統合候補にはこの履歴を変換する機能がない。

既存SQLxはversion9のchecksum不一致で拒否し、通常起動のread-only互換検査も拒否する。エラーを消すためのversionの9→11変更、checksum差替え、台帳行削除、`ignore_missing`、既存schemaを残した空台帳への作替え、Outbox SQLの再実行、DB削除・再作成は行わない。失敗時も同じ操作を繰り返して成功扱いしない。

## 対応予定の正規履歴でも必要な確認

新しい専用の空DB、checksum一致の正規1〜8、正規Document1〜10の三つだけが本候補の合成upgrade試験対象である。既に正規1〜11なら同じ版での再適用は無変更を要求する。それ以外を近い状態へ丸めない。

実際の更新では既存の[Linux手順書のbackup・別環境復元・更新節](linux-manual-installation.md)を確認し、対象releaseに一致した手順を用意する。同手順書の固定Document/Organization版に、候補SHAだけを差し替えない。本候補の統合試験が成功しても、大表の索引作成・ロック時間、配送worker起動条件、実本番の認証・権限・運用は別途確認が必要である。

Git mergeはDB migrationを実行しない。Git revertもDB/storage・配送結果を戻さない。deployと実DB適用は所有者の明示操作として扱う。
