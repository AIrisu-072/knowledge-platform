# 原本構成編集・初回複数原本登録の進捗

2026-10-08。承認範囲は[設計](../specs/2026-10-08-document-original-management.md)、[計画](../plans/2026-10-08-document-original-management-implementation.md)。branch `feat/document-originals-20261008`、base `6440e026`、計画commit `defef0d7`。専用worktreeで実装し、rootの作業を変更していない。push/PR/main統合は親の調整待ち。

## 実装済み

- 作業版だけで原本追加・除外・取消・上下移動。最終原本の除外禁止。構成変更時だけordinal再採番。保持原本と補助ファイルの監査付き取得、UNKNOWN固定bytes再送、OCC・権限・世代guardを維持。
- 初回複数原本の新multipartと、全原本・Document・初版・domain/auditイベントの原子的DB保存。legacy単原本を維持。null/空/重複/欠落/未参照partsや不正path/mediaを拒否。
- ordered全File IDsの成功・UNKNOWN receiptと回復照合。部分一致を成功としない。GUIは一つの文書へ全原本を登録し、複数選択後に一件へ除外しても入力したpathを保持する。
- multipart準備を回復marker保存前に行う。送信前の容量拒否で回復不能UNKNOWNを作らない。schemaもlegacy/atomicの混在を拒否する。
- [日本語操作手順](../../operations/document-original-management.md)と実runtime受入sourceを追加。旧公開・全原本bytes・固定再送・公開切替・再起動snapshotを検証する試験を用意した。

## 実行した検証

Node 24.21.0、既存の固定依存を専用worktreeにhardlinkコピーして実行。offline installはtarball不足、通常installはDNS接続不可で成立しなかった。rootの依存を変更せず、依存の版やlockを更新していない。

- RED: 初回複数GUI新2件FAIL（既存19PASS）、SDK新2件FAIL（既存10PASS）、作業版構成helper/GUI不在、atomic command不在、HTTP新形式とnullmanifestの反例を確認。
- 最終全GUI: 正しい `apps/document-web` cwdで **1786件 / 75 suites PASS**。初回全GUIをroot cwdから実行した時の9件失敗はcwd依存の既存契約テストで、正しいcwdへ修正して全件再実行した。
- TypeScript application/SDK、schema生成check、GUI production build成功。webpackの既存size/runtimeChunk警告は残る。
- SDK **14件 PASS**、API contract **21件 PASS**。OpenAPI lint成功、Searchの既存localhost server警告1件。
- Rust focused **24件 PASS**: HTTP10、create application12、commit identity1、recovery validation1。
- PostgreSQL **18.6-bookworm** +実FSの専用試験 **1件 PASS**: 全2原本とdomain/audit各2イベント、部分/不一致/逆順の回復拒否、権限喪失、新しいrepository/storageアダプターでbytes保持、遅いoutbox CHECK失敗時の全DB行rollback。DBプロセスそのものの再起動試験ではない。
- strict Clippy（application/API/repository `--all-targets -- -D warnings`）、`cargo fmt --all -- --check`、`git diff --check`成功。
- 実runtimeの型検査・5試験収集・記録抑止source試験15件成功。実browser実行の成功を意味しない。
- registration/schema/SDKの独立レビューでP2を2件検出し修正、再レビューGO。Rust全体の独立レビューは親の実施待ち。

DBは所有する使い捨てcontainerだけを使用。最終container `18c43935…` は `127.0.0.1:32769` に束縛し、試験後の削除を確認。既存の利用者containerやセキュリティ設定を変更していない。

## 次のexact actionと未実施

共有Cargo cacheが増えディスクが4 GiB reserveを下回ったため、新しいheavy buildとbrowser harnessを止め、全Cargo終了を親へ通知した。親が所有する `target/debug/incremental` だけを整理し、5.2 GiBへ回復した。PR108統合main `6a34de3f0904949daca304b9178ef5125ea13d82` を通常mergeし、組合せhead `664847658c2b3d3413aa342ee3ec214e70257f7e` / tree `6254deb2fe3aee894c8e27125e0b9affd137154f` を保持した。製品treeは統合前と変わらない。

実browser harnessは `run.mjs` がqualified Linux sandboxとPDFium `libpdfium.so`を必須とするため、このMacではproduction実受入を資格化できない。無駄なbuildや確認の無効化をせず親へ通知した。次は独立Rustレビューと、Linux hosted exact-headで実Document browser journey・再起動persistenceを実行する。tools/document-poc-runtime/run.mjsとCIは別担当所有なのでこのbranchでは変更しない。

全workspace Rust、実browser、DBプロセス再起動、exact-head hosted CI、main統合後CI、対象PC導入、本番認証/TLSは未実施。実機や本番の成功を記録していない。既読の意味は変更しない。
