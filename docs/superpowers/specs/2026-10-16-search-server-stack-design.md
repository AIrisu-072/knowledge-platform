# 検索サーバーの分離と複数サービスの一括起動 — 設計と実装計画（v0）

2026-10-16。状態：**所有者の承認待ち**。基点はmain `1f6ef414`。

## 目的

検索をAPIとして使えるようにする。検索のAPIは文書管理とは別のプロセスで動かし、文書管理・検索・DBなどの複数サービスを、リポジトリの設定1つでまとめて起動・停止できるようにする（所有者の方針、2026-10-16）。

到達点は、今の文書管理と同じPoCの水準とする。文書サーバーは呼出しを固定のプロフィール（`poc-human` / `poc-agent`）として扱い、本番の認証を持たない（`crates/document-server/src/identity.rs`）。検索サーバーもこれに合わせ、本番の認証・TLS・本番運用資格はこの設計の範囲に入れない。

## 現状（2026-10-16 確認）

- 検索のAPIを起動できるのは検証用host（`experiments/search-validation-corpus/host`）だけ。本番用の部品（`build_search_api_runtime`、`DurableDocumentPorts`、`search_api_http::send::serve`）を組み立て、利用者の確認だけを合成のトークンで行う。loopback以外への公開を拒否する。
- `document-server` と `organization-server` は検索のAPIを持たない。GUIの検索画面は「未実装」の表示だけ。
- 索引を作る `search_outbox_worker` は別プロセスとしてすでにある。
- コンテナは直下の `Dockerfile` 1つ（用途別のDockerfileは `ARCH_DOCKERFILE_VARIANT` で禁止）。本番の成果物は `linux/amd64` のOCI image（`REQ-DEV-PRODUCTION-LINUX-AMD64`）。Kubernetesは開発基盤の仕様でv0の対象外（`spec/architecture/development-container-ci-architecture-v0.md` 59節）。複数サービスの起動設定はまだ無い。

## 設計

### 1. 検索サーバー `crates/search-server`

新しいcrateと実行ファイル `search-server`。検証用hostの組み立てを本番のcrateへ移し、利用者の確認だけを置き換える。

- コマンド：`serve`（APIを開く）と `migrate`（検索のschemaを `search_runtime::migrate` で適用）。文書サーバーの `serve` / `migrate` と同じ形。
- 利用者の確認（PoC）：プロセスごとに固定のプロフィール（`human` または `agent`）を持つ。検索APIの規約どおりBearerの確認は行い、プロセスに設定した秘密のトークンと一致した呼出しだけを、そのプロフィールの利用者として通す。トークンは秘密値として扱い、表示もログ出力もしない。文書側の主体・グループは文書サーバーのPoCと同じ（`poc` 発行者の `poc-human`・`poc-users`、`poc-agent`・`poc-agents`）なので、文書のACLがそのまま効く。
- 公開範囲：既定はloopback。それ以外へ開くには、文書サーバーと同じく明示の設定（`KP_POC_ALLOW_NON_LOOPBACK`）と起動時の警告が要る。
- 設定は環境変数：`KP_IDENTITY_PROFILE`、`KP_DATABASE_URL`（秘密）、`KP_BIND`、`KP_SEARCH_TOKEN_FILE`（秘密）、`SEARCH_WORKER_CONFIG`（workerと同じSource設定）、Vectorを使うときのモデルの場所。
- 権限の最終確認と文書の現在の版は、今と同じく同じPostgreSQLを文書側のライブラリで直接読む（HTTPで文書サーバーへ問い合わせない）。
- 検証用hostは、検証の計測用として残す（合成の利用者で多数の利用者を作れるため）。共通の組み立ては新しいcrateから使う。

### 2. 複数サービスの一括起動：Docker Compose

リポジトリ直下に `compose.yaml` を置く。Kubernetesは開発基盤の仕様のv0非対象のままとし、必要が具体化した時点で同じimageから別に足す。

| サービス | 中身 | 備考 |
|---|---|---|
| `postgres` | PostgreSQL 18.6（公式image、digest固定） | 永続volume |
| `init` | `document-server migrate`・`bootstrap-poc`、`search-server migrate` を一度だけ実行 | 終わってから他が起動 |
| `document-human`・`document-agent` | `document-server serve`（PoCの2プロフィール） | 原本保存のvolumeを共有 |
| `search-worker` | `search_outbox_worker`（本文抽出のsandboxを含む） | 原本保存（読取り）と索引のvolume |
| `search-human`・`search-agent` | `search-server serve` | 索引のvolume（読取り） |

- ホスト側の公開portは `127.0.0.1` だけに結ぶ。
- 秘密値（DBのパスワード、検索のトークン）は `.env` ではなくcomposeのsecretとして、リポジトリ外のファイルから渡す（リポジトリには例の形だけ）。
- Vectorのモデルは、固定revisionで取得したディレクトリを読取り専用でmountする（imageに含めない）。モデルが無ければVector無しで起動する。

### 3. image：直下の `Dockerfile` にtargetを足す

既存の `rust-build` 段から、`document-server`（DSI・差分worker、固定版PDFiumを含む）、`search-worker`（本文抽出workerを含む）、`search-server` のtargetを足す。実行用の土台は既存の `bootstrap-tools` と同じ `debian:bookworm-slim`、利用者は非root（uid 10001）。本文抽出のsandbox（Landlock・seccomp）がコンテナの中で効くかは実装で確かめ、効かない場合は弱めて動かさず、止めて報告する。

### 4. 操作：miseのtask

`stack:build`（image作成）、`stack:up` / `stack:down`、`stack:smoke`（下の受入確認を自動で行う）。CIは既存のgateを変えない。composeのsmokeをCIへ入れるかは、所要時間を測ってから決める。

### 5. 仕様への追加

- `spec/selection/library-tool-selection-v0.md` に「Docker Compose v2：単一ホストのPoC構成の起動。Kubernetesは開発基盤のv0非対象のまま」を追加。
- 検索サーバーのPoCの利用者確認は、本番の認証経路ではないことを、文書サーバーと同じく明記する。

## 受入確認（`mise run stack:smoke`）

1. `stack:up` で全サービスが起動し、`init` が成功で終わる。
2. 文書APIで文書を登録・公開し、検索のworkerが反映したあと、検索サーバーのSearchとDiscoverでその文書が返る。
3. 文書のACLで見えない文書は、検索サーバーの結果・件数・抜粋に出ない（`human` と `agent` で結果が違う）。
4. トークンが無い・違う呼出しは401になる。
5. 検索サーバーを再起動しても、同じ結果が返る。
6. 既存の全CI（CI、DSI PoC、DSI Sandbox Preflight）が成功する。

## 範囲外

本番の認証・TLS、Kubernetes、GUIの検索画面の接続（別の作業）、運用操作の試験（版の更新・取下げ・権限変更・漏えい・backup/restore）、10万文書、更新中のp99、精度の改善（組合せの変更・タイトル付きの埋め込みは仕様の判断待ち）。

## 実装計画

| 段 | 内容 | 確認 |
|---|---|---|
| 1 | `crates/search-server`：検証用hostの組み立てを移し、PoCの固定プロフィールとトークンの確認、`serve` / `migrate`、loopbackの既定 | 単体試験（トークン一致・不一致、プロフィールの対応）、architecture-lint、既存のRust gate |
| 2 | `Dockerfile` のtarget（document-server、search-worker、search-server） | `docker buildx build --platform linux/amd64` の各target、非rootで起動 |
| 3 | `compose.yaml`、secretの渡し方、volume、miseのtask | `stack:up` / `stack:down` が冪等に動く |
| 4 | `stack:smoke`（受入確認1〜5） | Linux（amd64のCI相当環境と開発用VM）で成功 |
| 5 | 仕様（選定）・手順書・状態記録 | 独立レビュー、hosted CI |

各段は別のcommitにし、1本のDraft PRでmainへ。mergeはhosted CIの成功後に所有者が確認する。deployはしない。
