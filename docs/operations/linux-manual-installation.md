# Linuxサーバーへの手動導入と復旧

## この手順でできること

所有者が自分でLinuxサーバーへ導入するための手順。対象は10月2日に設置したPCで、確定情報はCore Ultra 9 285KとLinux方針のみ。ディストリビューション、OS版、メモリー、ディスク、接続先は未確定である。「最新安定版」だけからUbuntu等を選定済みとは扱わない。

**現在実行できるのは、架空データだけを使うOrganization Browser PoCの導入である。本番利用開始の手順は未完成。** 固定の営業・事務profileを使い、そのポートへ接続した人は同じprofileとして扱われる。認証画面、実利用者の識別、production modeはない。実文書・顧客情報を投入せず、インターネットや社内LANへ公開しない。

- 固定ソース：PR57の `d383baccddd5081687b500f064f6fce195a24816`
- 既存確認：同一headの[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37205217912)、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37205217991)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37205217880)成功。使い捨てPostgreSQL・2名操作・HTTPサーバー再起動後の復元を確認済み
- この手順そのものの対象PCでの実行、常設DBのbackup/restore、PostgreSQLプロセス再起動後の確認は未実施。CI成功と区別する
- GPU、CUDA、外部モデル、Tauriは使わない。新しいAgent機能はこの固定版に含まれない
- 本書のコマンドは所有者が実行する。既存本番サーバーへの接続や秘密情報の送信を代行するものではない

## 1 開始前の確認

- [ ] `/etc/os-release`、`uname -m`、`uname -r`、空きディスクを確認した
- [ ] Linux x86_64で、既存DSI/DiffのLandlock・seccomp保護が動作する。CIの参照OSはUbuntu 24.04だが、選定OSの資格取得を意味しない
- [ ] 専用の非root Linuxユーザーを使い、そのユーザーだけが下記作業ディレクトリを読める
- [ ] Bash、Git、curl、tar、OpenSSL、C/C++ビルド環境、pkg-config、Fontconfig、mise/rustupをOS・各提供元の公式手順で導入した。ディストリビューション未定のためapt/dnf等のコマンドはここでは固定しない
- [ ] 既に許可されたDocker Engineを利用でき、`docker info` が成功する。Docker権限は強い権限であり、動作させるためだけにユーザー追加や保護設定変更をしない
- [ ] 127.0.0.1の15432、8090、8091番portが未使用。別の既存サービスを停止して流用しない
- [ ] 既存DB、既存storage、SearchのDBを再利用しない。中断後にこの初回手順を最初から再実行しない

保護・socket・workerの前提で拒否された場合は停止する。sandbox無効化、root実行、外向きbind、別経路への切替で通過させない。対象OSを確定してから、そのOSで必要なネイティブ依存と実runtime試験を確認する。

## 2 固定ソースとビルド

以降はBash。初回用の専用ディレクトリを作る。既に存在したら停止して中身を確認する。

```bash
set -euo pipefail
set +x
umask 077
export KP_HOME="$HOME/knowledge-platform-poc"
export KP_SOURCE_SHA=d383baccddd5081687b500f064f6fce195a24816
export KP_SOURCE="$KP_HOME/releases/$KP_SOURCE_SHA"
test ! -e "$KP_HOME"
install -d -m 700 "$KP_HOME" "$KP_HOME/releases" "$KP_HOME/config" \
  "$KP_HOME/storage" "$KP_HOME/backups"
git clone --no-checkout https://github.com/AIrisu-072/knowledge-platform.git "$KP_SOURCE"
cd "$KP_SOURCE"
git checkout --detach "$KP_SOURCE_SHA"
test "$(git rev-parse HEAD)" = "$KP_SOURCE_SHA"
git status --short
```

`AGENTS.md` と `mise.toml` を確認する。miseが設定への信頼確認を求めた場合は、内容を確認して所有者が判断する。固定版はRust 1.98.1、Node 24.21.0、pnpm 12.4.1。勝手に「最新」へ上げない。

```bash
mise install rust
mise install node
mise install pnpm
eval "$(mise env -s bash)"
export CARGO_TARGET_DIR="$KP_SOURCE/target"
PDFIUM_DYNAMIC_LIB_PATH="$(bash experiments/document-semantic-inspection/scripts/install-pdfium.sh)"
export PDFIUM_DYNAMIC_LIB_PATH
pnpm install --frozen-lockfile --ignore-scripts
cargo build --locked -p organization-server \
  -p document-semantic-inspection-worker -p document-diff-worker
pnpm --filter @knowledge-platform/document-web build
test -x "$CARGO_TARGET_DIR/debug/organization-server"
test -d "$KP_SOURCE/apps/document-web/dist"
git diff --exit-code
```

これは受入時と同じdebugビルド経路。releaseビルドの性能・適格性を主張しない。PDFiumは既存スクリプトが固定151.0.7881.0のarchiveとlibraryのhashを確認する。失敗した場合は非検証版へ差し替えない。既存Dockerfileはツール/scheduler向けで、Organizationアプリを配備するimageではない。

## 3 専用の合成DBと秘匿設定

この例はlocalhost限定のPoC専用PostgreSQLを新規作成する。DBのpostgres管理者を使用する簡易構成であり、本番の最小権限構成ではない。データは専用Docker volumeへ保持する。Dockerコンテナは自動起動設定にしない。

```bash
export KP_DB_CONTAINER=kp-organization-poc-db
export KP_DB_VOLUME=kp-organization-poc-pg18
export KP_DB_NAME=kp_organization_poc
if docker container inspect "$KP_DB_CONTAINER" >/dev/null 2>&1; then
  echo '同名containerがあるため停止'; exit 1
fi
if docker volume inspect "$KP_DB_VOLUME" >/dev/null 2>&1; then
  echo '同名volumeがあるため停止'; exit 1
fi
db_password="$(openssl rand -hex 32)"
printf 'POSTGRES_USER=postgres\nPOSTGRES_DB=%s\nPOSTGRES_PASSWORD=%s\n' \
  "$KP_DB_NAME" "$db_password" > "$KP_HOME/config/postgres.env"
export KP_DATABASE_URL="postgres://postgres:${db_password}@127.0.0.1:15432/$KP_DB_NAME"
unset db_password
export KP_RUNTIME_MODE=organization-synthetic
export KP_STORAGE_ROOT="$KP_HOME/storage"
export KP_DSI_WORKER="$CARGO_TARGET_DIR/debug/document-semantic-inspection-worker"
export KP_DIFF_WORKER="$CARGO_TARGET_DIR/debug/document-diff-worker"
export KP_WEB_DIST="$KP_SOURCE/apps/document-web/dist"
export KP_DSI_PDFIUM_RUNTIME_DIR="$PDFIUM_DYNAMIC_LIB_PATH"
for key in KP_HOME KP_SOURCE_SHA KP_SOURCE KP_DB_CONTAINER KP_DB_VOLUME KP_DB_NAME \
  KP_DATABASE_URL KP_RUNTIME_MODE KP_STORAGE_ROOT KP_DSI_WORKER KP_DIFF_WORKER \
  KP_WEB_DIST KP_DSI_PDFIUM_RUNTIME_DIR; do
  printf 'export %s=%q\n' "$key" "${!key}"
done > "$KP_HOME/config/runtime.env"
chmod 600 "$KP_HOME/config/postgres.env" "$KP_HOME/config/runtime.env"
docker pull postgres:18.6-bookworm
KP_DB_IMAGE="$(docker image inspect --format '{{index .RepoDigests 0}}' postgres:18.6-bookworm)"
test -n "$KP_DB_IMAGE"
printf '%s\n' "$KP_DB_IMAGE" > "$KP_HOME/config/db-image.txt"
docker volume create --label kp.purpose=organization-synthetic "$KP_DB_VOLUME"
docker run -d --name "$KP_DB_CONTAINER" --label kp.purpose=organization-synthetic \
  --env-file "$KP_HOME/config/postgres.env" \
  --publish 127.0.0.1:15432:5432 \
  --mount "type=volume,source=$KP_DB_VOLUME,target=/var/lib/postgresql" \
  "$KP_DB_IMAGE"
ready=0
for attempt in {1..60}; do
  if docker exec "$KP_DB_CONTAINER" pg_isready -h 127.0.0.1 -U postgres -d "$KP_DB_NAME" >/dev/null; then
    ready=1; break
  fi
  sleep 1
done
test "$ready" = 1
docker exec "$KP_DB_CONTAINER" psql -X -v ON_ERROR_STOP=1 -U postgres -d "$KP_DB_NAME" \
  -c 'SHOW server_version;'
```

versionが18.6であることを確認する。readinessはTCPを指定し、初期化中だけ動くUnix socketの一時serverを合格にしない。PostgreSQL 18の公式imageのvolume先は `/var/lib/postgresql`。古い17以前の例と混同しない。[公式imageの説明](https://hub.docker.com/_/postgres)

2つのenvファイルは秘密情報を含む。Git、チャット、画像、サポート用ログへ貼らない。`env` やcontainer inspectの全内容を公開しない。Docker管理者とOS管理者には参照され得る。ディレクトリ権限はディスク暗号化や本番secret管理の代わりにはならない。

## 4 明示的な初期化

初回の新しい専用DBだけで実行する。Document migration 0001〜0010と、別台帳のWork migration 0001〜0003を適用する。`serve` はmigrationやseedを実行しない。

```bash
source "$KP_HOME/config/runtime.env"
export KP_ORGANIZATION_PROFILE=sales-01
unset KP_BIND
"$KP_SOURCE/target/debug/organization-server" migrate
"$KP_SOURCE/target/debug/organization-server" bootstrap-poc
```

DocumentとWorkのmigrationは別々に適用され、両方を一括rollbackするコマンドではない。失敗・結果不明ならDBと台帳を調査し、ledgerの行削除やchecksum変更で通さない。`bootstrap-poc` は固定2名の合成Document権限を作り、異なる既存policyを上書きしない。

## 5 営業と事務を起動

Linuxサーバーの別々のterminalでforeground起動する。所有者が終了状態を確認できるよう、自動再起動やバックグラウンドdaemon化はここでは追加しない。

営業terminal:

```bash
set -euo pipefail
set +x
source "$HOME/knowledge-platform-poc/config/runtime.env"
unset KP_BIND
KP_ORGANIZATION_PROFILE=sales-01 "$KP_SOURCE/target/debug/organization-server" serve
```

事務terminal:

```bash
set -euo pipefail
set +x
source "$HOME/knowledge-platform-poc/config/runtime.env"
unset KP_BIND
KP_ORGANIZATION_PROFILE=office-01 "$KP_SOURCE/target/debug/organization-server" serve
```

3つ目のterminalでhealthを確認する。200と `{"status":"ok"}` が必要。healthだけで全業務経路を合格とはしない。

```bash
curl --fail --max-time 30 http://127.0.0.1:8090/health/ready
curl --fail --max-time 30 http://127.0.0.1:8091/health/ready
```

サーバー自身のブラウザーでは営業 `http://127.0.0.1:8090/tasks?view=context`、事務 `http://127.0.0.1:8091/tasks?view=queue` を開く。手元PCから見る場合は、既に許可・設定済みのSSH接続で8090/8091をそれぞれ手元の127.0.0.1へ転送する。SSH導入やfirewall変更はこの手順に含めず、serverを `0.0.0.0` へ変更しない。両ポートへの接続は両profileの操作権限を持つことになる。

## 6 合成文書とタスクの準備

初回だけ、営業serverへ合成文書を1件登録する。下のPOSTに自動retryは付けない。送信後の応答を失った場合、再POSTで二重作成せず、一覧から結果を確認する。

```bash
source "$HOME/knowledge-platform-poc/config/runtime.env"
umask 077
printf '【合成データ】2名の動作確認だけに使う共有資料です。\n' \
  > "$KP_HOME/organization-reference.txt"
curl --fail-with-body --max-time 60 \
  -F 'request={"folderId":"00000000-0000-7000-8000-000000000001","title":"PoC共有参照資料","documentMetadata":{},"versionMetadata":{}};type=application/json' \
  -F "file=@$KP_HOME/organization-reference.txt;type=text/plain" \
  http://127.0.0.1:8090/v1/documents \
  > "$KP_HOME/document-create-result.json"
```

保存したJSONの `documentId` と `documentVersionId` を確認する。営業画面の `/documents/{documentId}?view=authoring` で、その文書を「公開」する。公開成功後、同じ文書IDを入力してWorkの合成タスクを作る。

```bash
read -r -p '公開済みの合成documentId: ' KP_ORGANIZATION_DOCUMENT_ID
export KP_ORGANIZATION_DOCUMENT_ID
KP_ORGANIZATION_PROFILE=sales-01 "$KP_SOURCE/target/debug/organization-server" seed-work
```

seedは既存Workをリセットしない。別の入力文書や以前のforward-only定義を再利用して作り直す場合は、このDBを上書きせず新しい専用環境で行う。予約公開schedulerはこのOrganization手順では起動しない。

確認する操作:

- [ ] 営業が文案を保存し、事務には未提出本文が見えない
- [ ] 共有文書を参照し、営業が提出、事務が引き受けて提出内容を読む
- [ ] 事務が理由を付けて差戻し、営業が新試行で修正・再提出する。旧提出は変わらない
- [ ] 根拠・候補・採用/修正/却下を作り、明示選択分だけ提出へ含める
- [ ] 両HTTPプロセスを正常停止して同じ設定で再起動し、保存済み状態と非公開分離を再確認する

詳細は[既存の操作手順](organization-browser-poc.md)に従う。画像、ログ、DB、storageを外部へ送らず、結果だけを記録する。

## 7 正常停止と再開

両ブラウザーの操作・downloadを終了し、営業と事務の各terminalでCtrl+Cを1回送る。両方の `organization-server: graceful drain complete` とプロセス終了を確認する。処理中のstreamには全体の強制終了期限がないため、完了しない場合は接続中clientを確認する。強制killを正常停止と扱わない。

DBも停止する場合は、両アプリが終了してから行う。volumeは削除しない。

```bash
source "$HOME/knowledge-platform-poc/config/runtime.env"
docker stop --timeout 60 "$KP_DB_CONTAINER"
docker inspect --format '{{.State.Status}} exit={{.State.ExitCode}}' "$KP_DB_CONTAINER"
```

Dockerはtimeout後に強制終了し得る。正常終了でなければその事実を記録し、復旧確認前に安全としない。再開は `docker start "$KP_DB_CONTAINER"` 後、節3のreadiness確認、節5のアプリ起動、節6の保存済み状態確認を行う。初回の `migrate`・`bootstrap-poc`・`seed-work` は同じ版の通常再開では繰り返さない。

## 8 停止時バックアップ

**両アプリと、このDB/storageへ書く他の全プロセスを停止した状態で、DBとstorageを一組として保存する。DB自体は起動したまま。** `pg_dump` 単体の整合性は、別filesystemとの整合性を保証しない。

```bash
source "$HOME/knowledge-platform-poc/config/runtime.env"
set -euo pipefail
set +x
umask 077
BACKUP="$KP_HOME/backups/$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -m 700 "$BACKUP"
docker exec "$KP_DB_CONTAINER" pg_dump -U postgres -d "$KP_DB_NAME" --format=custom \
  > "$BACKUP/database.dump"
tar -czf "$BACKUP/storage.tar.gz" -C "$KP_STORAGE_ROOT" .
printf '%s\n' "$KP_SOURCE_SHA" > "$BACKUP/source.sha"
cp "$KP_HOME/config/db-image.txt" "$BACKUP/db-image.txt"
cp "$KP_HOME/config/runtime.env" "$BACKUP/runtime.env"
cp "$KP_HOME/config/postgres.env" "$BACKUP/postgres.env"
(cd "$BACKUP" && sha256sum database.dump storage.tar.gz > SHA256SUMS)
test -s "$BACKUP/database.dump"
```

backup内の設定も秘密情報を含む。source/binary/GUI/PDFiumを含む元のreleaseディレクトリも保持する。これはローカルPoCの停止時保存例で、暗号化した別媒体保存、保持期間、定期実行、災害復旧、復旧時間の資格取得は別途必要。[pg_dumpの範囲](https://www.postgresql.org/docs/18/app-pgdump.html)

## 9 元環境を壊さない復元確認

所有者自身が作成したbackupだけを使う。元DBと元storageは残す。両アプリを停止したまま、backupディレクトリを選び、hashを検証する。

```bash
source "$HOME/knowledge-platform-poc/config/runtime.env"
set -euo pipefail
set +x
umask 077
read -r -p '復元するbackupディレクトリの絶対path: ' BACKUP
test -d "$BACKUP"
(cd "$BACKUP" && sha256sum -c SHA256SUMS)
test "$(cat "$BACKUP/source.sha")" = "$KP_SOURCE_SHA"
KP_RESTORE_DB="kp_org_restore_$(date -u +%Y%m%d%H%M%S)"
KP_RESTORE_STORAGE="$KP_HOME/storage-$KP_RESTORE_DB"
mkdir -m 700 "$KP_RESTORE_STORAGE"
docker exec "$KP_DB_CONTAINER" createdb -U postgres --template=template0 "$KP_RESTORE_DB"
docker exec -i "$KP_DB_CONTAINER" pg_restore -U postgres -d "$KP_RESTORE_DB" \
  --exit-on-error --single-transaction < "$BACKUP/database.dump"
tar -xzf "$BACKUP/storage.tar.gz" --no-same-owner -C "$KP_RESTORE_STORAGE"
export KP_DATABASE_URL="${KP_DATABASE_URL%/*}/$KP_RESTORE_DB"
export KP_STORAGE_ROOT="$KP_RESTORE_STORAGE"
for key in KP_HOME KP_SOURCE_SHA KP_SOURCE KP_DB_CONTAINER KP_DB_VOLUME KP_DB_NAME \
  KP_DATABASE_URL KP_RUNTIME_MODE KP_STORAGE_ROOT KP_DSI_WORKER KP_DIFF_WORKER \
  KP_WEB_DIST KP_DSI_PDFIUM_RUNTIME_DIR; do
  if [[ "$key" == KP_DB_NAME ]]; then
    printf 'export KP_DB_NAME=%q\n' "$KP_RESTORE_DB"
  else
    printf 'export %s=%q\n' "$key" "${!key}"
  fi
done > "$KP_HOME/config/restore.env"
chmod 600 "$KP_HOME/config/restore.env"
```

復元先は空DBなので、先にmigration/bootstrap/seedを走らせない。`pg_restore --single-transaction` は復元SQLを一括transactionで処理する。失敗時に `--clean`、ledger修正、元DB削除で続行しない。[pg_restore](https://www.postgresql.org/docs/18/app-pgrestore.html)

節5の2つのterminalで、読み込むファイルだけを `config/restore.env` に変えて起動する。health、合成文書の原本、提出・差戻・根拠・非公開分離、以前の保存状態を確認する。元環境と同じportなので同時起動しない。元環境へ戻る場合は復元側を正常停止し、元の `runtime.env` で再開する。復元コピーへの新しい書込は元DBへ戻らない。

## 10 更新と切戻し

1. 新しい受入済みcommit SHAとそのexact CI結果を決め、別のreleaseディレクトリへ取得・ビルドする。稼働中のcheckoutやbinaryを上書きしない
2. 新旧のmigrationファイル・台帳・環境変数・操作仕様を比較する。新headに本書の固定SHAだけを差し替えて実行しない
3. 両アプリを停止し、節8のDB/storage/設定/releaseを保存する。節9の**別DB・別storage**で新しい候補のmigrationと起動・業務・復旧を先に確認する
4. 検証できた変更だけを所有者が適用する。更新対象の実DBに対するmigrationは明示操作であり、Gitのmergeでは実行されない
5. schema/dataに変更がないことを確認できる場合のみ旧releaseへの切替を検討する。旧binaryがschema不一致で拒否したら、保護を解除しない
6. schema/data変更後の切戻しは、互換性を確認したforward fix、または更新前のDBとstorageをセットで別環境へ復元して旧releaseを起動する。更新後の書込を失う可能性を所有者が判断する

**Git revertはDB migration、提出済みデータ、原本storage、外部へ送った情報を戻さない。** 下りmigration、DB巻戻し、旧ledgerへ偽装するコマンドは提供していない。

現在の統合注意点:

- Document/Orgには `0009_document_revisions_v0.sql` と `0010_document_version_updated_at.sql`、Search PR40には同じmigration列の `0009_outbox_delivery_v0.sql` がある。単純統合ではversion 9が重複する。適用済み台帳を調べ、互換方針と統合試験を確定するまで同一DBへ適用しない
- 新Agent sliceは別branchで進行中。本書の基点はWork 0001〜0003のみ。TODO：受入済みAgent commit、Work 0004の内容・互換性・新設定・停止/回復経路を確認してから手順を更新する。外部モデルが動くと推測しない

## 11 本番利用開始までの未達項目

- [ ] OSのdistribution/version、運用ユーザー、RAM/ディスク容量、backup先、接続方式を決定し、対象機で既存worker保護を実証する
- [ ] production Identity adapterを実装・受入し、実利用者とrole/assignmentを現在の認可に結び付ける。固定profileを本番認証として使わない
- [ ] TLS、公開範囲、DNS、reverse proxy、DB最小権限、secret管理、service定義、起動順、監視・アラートを設計して受入する
- [ ] migrationのversion衝突と既存DB互換を解消し、Document/Organization/Searchを組み合わせる対象commitで統合試験を行う
- [ ] Audit配送・保存、Search全体、Agent連携など必要機能の未完gateを閉じる。個別schemaや合成PoCの合格で代替しない
- [ ] 定期backup、暗号化、別媒体保存、restore演習、切戻し時のデータ損失/RPO/RTOを決めて検証する
- [ ] 本番の対象releaseと利用範囲を確定する。現在のmain mergeはCIを起動するが、production配備は行わない

## 根拠と保守

- [Organization設定と固定profile](../../crates/organization-server/src/config.rs)、[実装済みCLI](../../crates/organization-server/src/main.rs)
- [Document runtimeの境界](document-poc-runtime-v0.md)、[Organization操作](organization-browser-poc.md)
- [固定toolchain](../../mise.toml)、[Work migrationと台帳](../../crates/work-repository-postgres/src/lib.rs)
- [手順書の検証状況](../superpowers/execution/linux-manual-installation-guide-status.md)

OS/版の選定後にOS固有の準備手順を補う。新しいソース・migration・認証方式を採用した場合は、本書の対象SHAと受入記録を一緒に更新する。
