# 単一ホストのPoC構成（docker compose）

文書サーバー、検索のworker、検索サーバー、PostgreSQLを、リポジトリ直下の `compose.yaml` でまとめて起動します。MacとLinuxで同じファイル・同じimageを使います。設計は `docs/superpowers/specs/2026-10-16-search-server-stack-design.md`、選定は `spec/selection/library-tool-selection-v0.md` 13.7。

**本番の構成ではありません。** 利用者の確認はPoCの固定プロフィールで、本番の認証・TLSはありません。外に開くportは `127.0.0.1` だけです。個人が1台の中で使うことを前提にしています（所有者の判断 2026-10-16）。

## 認証なしで使うときの守り

文書サーバーには認証がなく、届いた呼出しはすべてそのプロセスの固定プロフィール（人は管理の権限まで持つ）になります。検索サーバーはトークンを確認します。そのうえで、全サーバーに次の確認（`crates/http-local-guard`、`KP_ALLOWED_HOSTS`）を入れています。

- Hostが `127.0.0.1:<port>` か `localhost:<port>` でない要求は403。別の名前を127.0.0.1へ向ける攻撃（DNS rebinding）を防ぐ。
- 書込みの要求（POST・PUT・PATCH・DELETE）で、ブラウザが別のサイトからの要求と示すもの（`Origin` が上の名前以外、または `Sec-Fetch-Site` が `cross-site`・`same-site`）は403。開いた別のサイトのページから、文書を登録・変更されるのを防ぐ。
- curlやAgentのようにこれらのヘッダーを送らない呼出しは通る。

残るリスク：同じマシンで動くほかのプログラム（ブラウザ以外）と、同じcomposeのネットワークにあるコンテナは、文書サーバーを人のプロフィールで呼べます。共有のマシンや、ほかの人がログインするマシンでは使わないでください。複数人で使うときは、`IdentityAdapter`（文書）と `SearchCredentialVerifierPort`・`VerifiedActorResolverPort`（検索）へ認証の実装を差し込みます。

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

- 秘密値：`${KP_STACK_DIR:-~/.knowledge-platform/stack}/secrets/`（DBの `kp` と下の4つのroleのパスワード、検索サーバー2つのトークン）。`stack:secrets` が無いときだけ作る（0600）。リポジトリには入れない。
- データ：Docker volume（`pgdata`、`storage`＝文書の原本、`lexical`＝字句索引、`search-config`＝Source設定）。`stack:reset` で消える。

## 制限（v0）

- 本番の認証・TLS、Kubernetes、運用操作（版の更新・取下げ・権限変更・backup/restore）の手順は範囲外。
- 索引に入る形式はテキストだけ（Source設定 `search-source.sh` のprofile）。
- Vector（意味の検索）は既定で無効。使うときは固定revisionのモデルのディレクトリを `KP_SEARCH_VECTOR_MODEL_DIR` で渡し、コンテナへmountする（今後の作業）。
- DBの接続：初期化（migration、権限の設定）だけがsuperuserの `kp`。各サービスは自分専用の非superuserのroleで接続する（`deploy/compose/service-roles.sql`）。どのroleもDDLはできない。
  - `kp_document`（文書サーバー）：文書の表の行の読み書きだけ。検索の表には触れない
  - `kp_search_delivery`（検索のworkerの配送）：文書のoutboxの受取りと完了の記録だけ（`crates/outbox-delivery/sql/least_privilege.sql`）
  - `kp_search_worker`（検索のworkerの索引作成）：検索の表は `search_registration`・`search_builder`・`search_coordinator`・`search_gc` の範囲、文書の表は検索の結果を決める表の読取りだけ
  - `kp_search_server`（検索サーバー）：起動時のSourceの登録（`search_registration`）と読取り（`search_reader`）、文書の表は読取りだけ
  - 文書の監査の表と既読の記録は、検索のroleから読めない。最初の版の共通role `kp_app` は、`stack:up` の初期化で削除される
