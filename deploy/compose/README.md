# 単一ホストのPoC構成（docker compose）

文書サーバー、検索のworker、検索サーバー、PostgreSQLを、リポジトリ直下の `compose.yaml` でまとめて起動します。MacとLinuxで同じファイル・同じimageを使います。設計は `docs/superpowers/specs/2026-10-16-search-server-stack-design.md`、選定は `spec/selection/library-tool-selection-v0.md` 13.7。

**本番の構成ではありません。** 利用者の確認はPoCの固定プロフィールで、本番の認証・TLSはありません。外に開くportは `127.0.0.1` だけです。

## 必要なもの

- Docker（Macは OrbStack または Docker Desktop、LinuxはDocker Engine）と Compose v2
- `python3`（受入確認だけで使う、標準ライブラリのみ）

Apple Siliconでは `linux/arm64` のimageを作ります。本番と同じ `linux/amd64` にするときは `KP_STACK_PLATFORM=linux/amd64`（OrbStackではRosettaで動きます）。

## 操作

```sh
mise run stack:build    # imageを作る（初回はワークスペース全体のreleaseビルド）
mise run stack:up       # 起動（秘密値が無ければ先に作る）
mise run stack:smoke    # 受入確認（登録→検索、ACL、401、検索サーバーの再起動）
mise run stack:down     # 停止（データは残る）
mise run stack:reset    # 停止してデータも消す
```

`mise` を使わない場合は `bash deploy/compose/stack.sh <同じ名前>`。

## 起動するもの

| サービス | 内容 | ホストのport |
|---|---|---|
| `postgres` | PostgreSQL 18.6 | 公開しない |
| `document-migrate` → `search-migrate` → `search-roles`、`search-config` | 初期化（一度だけ実行して終わる） | - |
| `document-human` / `document-agent` | 文書サーバー（`poc-human` / `poc-agent`）。`document-human` はGUIも配る | 8080 / 8081 |
| `search-worker` | 文書の公開を受けて索引を作る | - |
| `search-human` / `search-agent` | 検索API（Search、Discover、Resource、Sources） | 8090 / 8091 |

portは `KP_DOCUMENT_HUMAN_PORT` などで変えられます（`compose.yaml` 参照）。

## 検索APIの呼び方

検索サーバーはBearerのトークンを確認します。トークンは秘密値のファイルにあり、画面やログに出しません。

```sh
token=$(cat ~/.knowledge-platform/stack/secrets/search_human_token)
curl -s -H "Authorization: Bearer $token" -H 'Content-Type: application/json' \
  -d '{"query":"検索したい語","coverage":"bodyRequired","pageSize":10}' \
  http://127.0.0.1:8090/v1/search
```

`search-human` は人のプロフィール、`search-agent` はAgentのプロフィールとして答えます。文書のACLで見えない文書は、結果・件数・抜粋に出ません。

## 秘密値とデータ

- 秘密値：`${KP_STACK_DIR:-~/.knowledge-platform/stack}/secrets/`（DBのパスワード、検索サーバー2つのトークン）。`stack:secrets` が無いときだけ作る（0600）。リポジトリには入れない。
- データ：Docker volume（`pgdata`、`storage`＝文書の原本、`lexical`＝字句索引、`search-config`＝Source設定）。`stack:reset` で消える。

## 制限（v0）

- 本番の認証・TLS、Kubernetes、運用操作（版の更新・取下げ・権限変更・backup/restore）の手順は範囲外。
- 索引に入る形式はテキストだけ（Source設定 `search-source.sh` のprofile）。
- Vector（意味の検索）は既定で無効。使うときは固定revisionのモデルのディレクトリを `KP_SEARCH_VECTOR_MODEL_DIR` で渡し、コンテナへmountする（今後の作業）。
- DBのroleはPoC用に1つ（superuser）。本番のrole分離は検証していない。
