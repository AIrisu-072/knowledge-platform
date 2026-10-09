# 専用 ext4 DB の PG18 起動確認

PG18 公式 image の既定 PGDATA は `/var/lib/postgresql/18/docker`。entrypoint は PGDATA 内を postgres へ chown してから実行ユーザーを切り替える。所有者だけが通過できる新規 bind root を `/var/lib/postgresql` に置くと、既定の子 directory の chown だけでは root が利用者所有のまま残り、postgres が Permission denied で停止する。

この runner は **新しく作る使い捨て DB directory だけ** を `PGDATA=/var/lib/postgresql` として渡す。公式 entrypoint がその directory の所有者を postgres に変更し、mode 700 を維持する。外側の private evidence directory は利用者所有のまま。既存 directory は再利用せず、ext4・実体 path・inode・単一 read-write bind の照合は維持する。hosted tmpfs の既定引数は変更しない。major upgrade 用の配置や既存 DB 移行手順ではない。

根拠は [公式 image の PGDATA 説明](https://hub.docker.com/_/postgres) と、使用 image の `/usr/local/bin/docker-entrypoint.sh`。既存の postgres:18.6-bookworm を使用し、確認時の RepoDigest・UID・entrypoint を private evidence に残す。

## 小さい実機確認

固定 Linux x86_64 依存を使い、空の利用者所有 mode 700 の専用 ext4 directory を作る。次の opt-in test は新規 baseline/fixed directory だけを使用し、旧設定の Permission denied と修正後の TCP readiness・18.6・PGDATA・mode/owner・mount を比較する。所有 label を照合して container を削除し、DB directory と証拠は保存する。

```sh
KP_DOCUMENT_LOAD_EXT4_CANARY=true \
KP_DOCUMENT_LOAD_EXT4_CANARY_DIR=/absolute/private/ext4/canary \
node --test tools/document-load-qualification/test/local-storage-docker.test.mjs
```

通常受入を ext4 で確認する場合だけ `KP_POC_OWNED_EXT4_DATABASE=true` を指定する。small にはさらに `KP_DOCUMENT_LOAD_SMALL=true` を指定する。いずれも harness が新規 DB を作り、単一 bind を検証し、自分の container を終了処理する。外部 DB 指定との併用、false/空値、GitHub Actions での opt-in は拒否する。証拠の配置先 `KP_POC_EVIDENCE_DIR` 自体が ext4 の canonical private path であることを先に確認する。

```sh
KP_POC_OWNED_EXT4_DATABASE=true node tools/document-poc-runtime/run.mjs
KP_POC_OWNED_EXT4_DATABASE=true KP_DOCUMENT_LOAD_SMALL=true node tools/document-poc-runtime/run.mjs
```

これらの成功は10万件の成功ではない。長時間 local launcher は修正 head のレビュー・CI・統合後に別途開始する。安全係数2、RSS・disk・memory・時間の停止線は変更しない。権限エラーを chmod 777、host側の既存データへの chown、sandbox の省略で回避しない。
