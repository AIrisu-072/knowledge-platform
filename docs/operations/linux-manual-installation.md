# Linuxサーバーへの手動導入と復旧

## この手順でできること

所有者が自分でLinuxサーバーへ導入するための手順。対象は10月2日に設置したPCで、確定情報はCore Ultra 9 285KとLinux方針のみ。ディストリビューション、OS版、メモリー、ディスク、接続先は未確定である。「最新安定版」だけからUbuntu等を選定済みとは扱わない。

**本書の対象は、架空データだけを使うOrganization Browser PoCの導入である。本番利用開始の手順は未完成。** 固定の営業・事務profileを使い、そのポートへ接続した人は同じprofileとして扱われる。認証画面、実利用者の識別、production modeはない。実文書・顧客情報を投入せず、インターネットや社内LANへ公開しない。

- 導入対象の資格：固定の模擬利用者2名・画像保存なしのUbuntu機能受入に合格した版（対象PCでの手順実行、本番認証、見た目全体の比較検証は対象外）。固定SHAと受入記録が未確定の版は実行しない
- 固定ソース：最終受入main `3d8deb253de19cb0954aa70a9a31cc5c4fc7540c` / tree `3f1ac6aa9e66d58bd5f01316e46334a48a64664f`
- GUI統合の確認：PR69初回登録、PR70取下げ・公開終了、PR71属性編集、PR72予約取消、PR73 WORKING backendを保持した[PR74](https://github.com/AIrisu-072/knowledge-platform/pull/74) exact `ce56801f7ec73ed284a99838f07cfe0c92cf71f4` / tree `3f1ac6aa9e66d58bd5f01316e46334a48a64664f`。[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37298371770)、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37298371873)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37298371821)。確認結果：required-checkを含む通常CI13/13・DSI・Sandboxが成功。Rust1599成功/9skip、指定実DB36成功、GUI404・runtime補助試験161成功。Document18件とHTTP再起動後5件、Agent9項目/provenance、Organizationの操作・往復・再起動・owned cleanup、公開artifact0を確認した。初回PUT・新版POST・続くPUTで、実成功応答のbody途中喪失から実headers/同一requestの失敗→UNKNOWN→同一要求の明示再送・結果一致・DB snapshot不変を確認。status/headersも全喪失する旧faultのGUI明示再送は未合格のままで、今回へ付け替えない
- 統合後mainの[push CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37301558995)。確認日：2026-10-05 11:31 UTC。確認結果：main自身のpush CIでrequired-checkを含む13/13 jobsが成功。Rust1599成功/9skip、指定実DB36成功。Document18件とHTTP再起動後5件、Agent9項目/provenance、Organizationの通常ナビ往復・操作・再起動・owned cleanup、公開artifact0を、PRとは別のmainログで確認した。exact head/clean、PostgreSQL18.6、固定合成2profileを照合した。作業版の固定再送資格は実成功応答のbody途中喪失に限定する
- 資格対象は画像なしUbuntuの実操作PoC。macOS golden比較は未実行・未更新。影響候補Mock 2・3・4・7の4枚に加え、他3枚の画素不変も未証明で、全visual資格は主張しない
- この手順そのものの対象PCでの実行、常設DBのbackup/restore、PostgreSQLプロセス再起動後の確認は未実施。CI成功と区別する
- GPU、CUDA、外部モデル、Tauriは使わない。Agentは固定の合成executorであり、既存Document現在認可を確認して候補を作る。本文分析・実LLM・外部MCP通信は行わない
- 本書のコマンドは所有者が実行する。既存本番サーバーへの接続や秘密情報の送信を代行するものではない

### 過去の受入記録

以下はGUI追加前の固定版に対する記録であり、上記の最終ソースや対象PCの手動導入へ資格を付け替えない。

- 当時の固定ソース：統合済みmain `6c514850850110a3c2f8b2b5664ec263510c5d47`。受入済み[PR67](https://github.com/AIrisu-072/knowledge-platform/pull/67) `a39c90c2` と同一tree `880b1a57abc6890ed47df5e7bc16a4694d4546cc`
- 当時の確認：PR67の[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37251574840)、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37251574859)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37251574842)成功。同treeで使い捨てPostgreSQL・2名の合成Agent/完了/保留再開/原本取得・HTTPサーバー再起動後の復元・cleanupを確認済み。統合後mainの[push CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37253316995)もrequired-checkを含む13 jobsと実受入が成功

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
export KP_SOURCE_SHA='3d8deb253de19cb0954aa70a9a31cc5c4fc7540c'
[[ "$KP_SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]] || { echo '受入済みの固定SHAが未設定です'; exit 1; }
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

初回の新しい専用DBだけで実行する。Document migration 0001〜0011（0011はOutbox）を `_sqlx_migrations`、Work migration 0001〜0006を別schema/台帳 `work.schema_migrations` へ適用する。Workの0004は合成Agent、0005は完了、0006は保留/再開の記録を支える。`serve` はmigrationやseedを実行しない。

```bash
source "$KP_HOME/config/runtime.env"
export KP_ORGANIZATION_PROFILE=sales-01
unset KP_BIND
"$KP_SOURCE/target/debug/organization-server" migrate
"$KP_SOURCE/target/debug/organization-server" bootstrap-poc
```

DocumentとWorkのmigrationは別々に適用され、両方を一括rollbackするコマンドではない。失敗・結果不明ならDBと台帳を調査し、ledgerの行削除やchecksum変更で通さない。`bootstrap-poc` はsales-01のfixture作成権限、office-01と固定Document provider `poc/poc-agent` のread/readHistoryを作る。異なる既存policyは上書きせず停止する。Agent対応前のDBへ暗黙にgrantを追加しない。

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

初回だけ、営業画面の通常GUIから合成文書を1件登録して公開する。ファイル選択はブラウザーを開いているPCのファイルを使う。SSH転送で手元PCから見ている場合も、Linuxサーバーではなく手元PC側にUTF-8の `organization-reference.txt` を作る。内容は架空の確認文だけにする。

ブラウザー側PCでBashを使える場合の例（同名ファイルがあれば停止する）。Bashを使わない場合は、テキストエディターで同じ本文をUTF-8の `.txt` として保存する。この操作ではサーバー用の `runtime.env` を読み込まない。

```bash
set -euo pipefail
umask 077
test ! -e "$HOME/organization-reference.txt"
printf '【合成データ】2名の動作確認だけに使う共有資料です。\n' \
  > "$HOME/organization-reference.txt"
```

1. 営業のOrganization画面でメインナビゲーションの「文書」を開く
2. フォルダーの「System Root」を選び、「文書を登録」を押す。Organizationの `bootstrap-poc` が権限を用意するのはこのルートであり、別のDocument PoC fixtureの「PoC Shared」は選ばない
3. 登録先が「System Root」であることを確認し、文書名を `PoC共有参照資料`、原本ファイルを上で作成した1件にする。「下書きとして登録」を押す
4. 登録が確認できると、対象文書のauthoring用途の「版・改訂」へ自動で移動する。登録直後はWORKINGの下書きであり、まだ公開されていない
5. 対象版と原本を確認して「公開する」を開く。「今すぐ公開」を選び、「公開対象の版とファイルを確認しました。」にチェックする
6. 画面の「公開する」を押し、「公開を確認」ダイアログの対象を確認して「確定する」を押す。成功表示を確認するまで次へ進まない
7. 公開成功後、同じ文書の「概要」で「記録・技術情報を確認」を開き、`Document ID` を控える。Version IDやrevision番号と取り違えない

初回登録の結果が不明なら、画面の「登録結果を確認」で照会する。初回登録には重複を防ぐ操作IDが無いため、再登録・再POSTをしない。照会できなければ「編集作業」の一覧や管理者に結果を確認する。未解決のまま別文書を作らない。

公開・予約公開の確定結果が不明なら、未公開や旧公開維持と断定せず、確認ダイアログの「同じ内容で再試行」で同じ操作ID・同じ対象・同じ要求を再送して結果を確認する。要求は公開画面の一時状態に保持されるため、版の変更、公開方法・予約日時の変更、公開画面の開き直し、画面からの離脱、ページの再読み込み、タブ終了を避ける。既に元の要求を失った場合は新しい公開要求を送らず、管理者に元の操作結果を確認する。公開成功を確認するまでは `seed-work` を実行しない。

次は**既存のLinuxサーバー側terminal**で行う。保存済み設定を読み、控えた公開済みDocument IDを入力してWorkの合成タスクを作る。

```bash
source "$HOME/knowledge-platform-poc/config/runtime.env"
read -r -p '公開済みの合成documentId: ' KP_ORGANIZATION_DOCUMENT_ID
export KP_ORGANIZATION_DOCUMENT_ID
KP_ORGANIZATION_PROFILE=sales-01 "$KP_SOURCE/target/debug/organization-server" seed-work
```

seedは既存Workをリセットせず、新規fixtureだけに完了/保留/再開を含む定義versionを使う。migration適用だけで既存workflowの定義・担当・進捗を変更しない。以前のforward-only/差戻/完了のみの定義や別の入力文書から作り直す場合は、このDBを上書きせず新しい専用環境で行う。予約公開schedulerはこのOrganization手順では起動しない。予約取消GUIがあっても、予約時刻の自動公開が稼働することを意味しない。[Document PoCのscheduler起動例](document-poc-runtime-v0.md#scheduler-and-other-boundaries)は `KP_RUNTIME_MODE=poc` 用であり、`organization-synthetic` のrequesterを解決しないため流用しない。

確認する操作:

- [ ] 営業が文案を保存し、事務には未提出本文が見えない
- [ ] 営業と事務のタスク内で公開改訂・内容の版・原本一覧を確認し、明示取得した原本を確認する。文書参照だけでTaskや未保存入力を変更しない
- [ ] 営業が提出、事務が引き受けて提出内容を読む
- [ ] 事務が理由を付けて差戻し、営業が新試行で修正・再提出する。旧提出は変わらない
- [ ] 根拠・候補・採用/修正/却下を作り、明示選択分だけ提出へ含める
- [ ] 選択した根拠を使って合成Agentを明示実行し、候補を人間が採用/修正/却下する。Agent結果だけで提出や完了が確定しない
- [ ] 営業/事務の担当中タスクを保留し、同じ試行・担当・private保存内容のまま再開する。未保存入力はタブ内だけで、自動保存しない
- [ ] 最終事務タスクを明示完了し、過去提出・根拠・判断・Agent結果を現在権限で読めること、新しい担当/提出が作られないことを確認する
- [ ] 両HTTPプロセスを正常停止して同じ設定で再起動し、完了状態・保存済み内容・操作結果と非公開分離を再確認する

文書の属性編集・既存複数原本の選択差替え・予約取消・公開状態の操作は[文書GUI手順](document-gui-v0.md)を参照する。初回登録は単原本で、複数原本の追加登録は今回含まない。

詳細は[既存の操作手順](organization-browser-poc.md)に従う。画像、ログ、DB、storageを外部へ送らず、結果だけを記録する。

## 7 正常停止と再開

両ブラウザーの操作・downloadを終了し、営業と事務の各terminalでCtrl+Cを1回送る。両方の `organization-server: graceful drain complete` とプロセス終了を確認する。処理中のstreamには全体の強制終了期限がないため、完了しない場合は接続中clientを確認する。強制killを正常停止と扱わない。停止/再起動時に残った未完了Agent実行は `outcome_unknown` として扱い、自動再実行しない。元の実行ID・operation IDで保存結果を確認する。

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

節5の2つのterminalで、読み込むファイルだけを `config/restore.env` に変えて起動する。health、合成文書の原本、提出・差戻・根拠/判断・合成Agent結果・完了/保留状態・非公開分離、以前の保存状態を確認する。元環境と同じportなので同時起動しない。元環境へ戻る場合は復元側を正常停止し、元の `runtime.env` で再開する。復元コピーへの新しい書込は元DBへ戻らない。

## 10 更新と切戻し

1. 新しい受入済みcommit SHAとそのexact CI結果を決め、別のreleaseディレクトリへ取得・ビルドする。稼働中のcheckoutやbinaryを上書きしない
2. 新旧のmigrationファイル・台帳・環境変数・操作仕様を比較する。新headに本書の固定SHAだけを差し替えて実行しない
3. 両アプリを停止し、節8のDB/storage/設定/releaseを保存する。節9の**別DB・別storage**で新しい候補のmigrationと起動・業務・復旧を先に確認する
4. 検証できた変更だけを所有者が適用する。更新対象の実DBに対するmigrationは明示操作であり、Gitのmergeでは実行されない
5. schema/dataに変更がないことを確認できる場合のみ旧releaseへの切替を検討する。旧binaryがschema不一致で拒否したら、保護を解除しない
6. schema/data変更後の切戻しは、互換性を確認したforward fix、または更新前のDBとstorageをセットで別環境へ復元して旧releaseを起動する。更新後の書込を失う可能性を所有者が判断する

**Git revertはDB migration、提出済みデータ、原本storage、外部へ送った情報を戻さない。** 下りmigration、DB巻戻し、旧ledgerへ偽装するコマンドは提供していない。

現在の統合注意点:

- この固定版ではDocument `0009_document_revisions_v0.sql` / `0010_document_version_updated_at.sql` を保持し、OutboxをSQL本文不変で `0011_outbox_delivery_v0.sql` へ配置済み。旧Search `0009_outbox_delivery_v0.sql` 適用済み・不明履歴は変換せず停止する。[判断記録](../decisions/2026-10-04-search-main-migration-integration.md)と[STOP条件](search-main-migration-stop.md)に従い、既存DBへこの初回手順を流用しない。所有者の実環境に旧Search9がないことは未証明
- Work 0001〜0006はDocumentと別の `work.schema_migrations` 台帳を使う。旧checksumは保持する。合成Agent/完了/保留再開は新規fixtureの定義を使用し、既存workflowの定義を自動昇格しない。モデル用秘密情報や新しい認証設定は不要
- PR62初回の再起動後read失敗とPR65初回のresponse.body()観測bytesの実encoding原因は未特定。PR67の実Download照合・再起動後復元成功を、対象PCの復旧資格や原因解消と読み替えない

## 11 本番利用開始までの未達項目

- [ ] OSのdistribution/version、運用ユーザー、RAM/ディスク容量、backup先、接続方式を決定し、対象機で既存worker保護を実証する
- [ ] production Identity adapterを実装・受入し、実利用者とrole/assignmentを現在の認可に結び付ける。固定profileを本番認証として使わない
- [ ] TLS、公開範囲、DNS、reverse proxy、DB最小権限、secret管理、service定義、起動順、監視・アラートを設計して受入する
- [ ] 対象実DBのmigration履歴・互換性を確認し、Document/Organization/Searchを組み合わせる対象commitで統合試験を行う
- [ ] Audit配送・保存、Search全体、Agent連携など必要機能の未完gateを閉じる。個別schemaや合成PoCの合格で代替しない
- [ ] 定期backup、暗号化、別媒体保存、restore演習、切戻し時のデータ損失/RPO/RTOを決めて検証する
- [ ] 本番の対象releaseと利用範囲を確定する。現在のmain mergeはCIを起動するが、production配備は行わない

## 根拠と保守

- [Organization設定と固定profile](../../crates/organization-server/src/config.rs)、[実装済みCLI](../../crates/organization-server/src/main.rs)
- [Document runtimeの境界](document-poc-runtime-v0.md)、[文書GUIの登録・属性編集・原本差替え・公開操作](document-gui-v0.md)、[Organization操作](organization-browser-poc.md)
- [固定toolchain](../../mise.toml)、[Work migrationと台帳](../../crates/work-repository-postgres/src/lib.rs)
- [手順書の検証状況](../superpowers/execution/linux-manual-installation-guide-status.md)

OS/版の選定後にOS固有の準備手順を補う。新しいソース・migration・認証方式を採用した場合は、本書の対象SHAと受入記録を一緒に更新する。
