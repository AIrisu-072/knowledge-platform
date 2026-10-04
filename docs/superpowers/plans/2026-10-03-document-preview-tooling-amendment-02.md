# Document本番ビルド・プレビューの検証計画

状態: **提案。実装前の設計レビューが必要**。
仕様: [ツール変更02](../specs/2026-10-03-document-preview-tooling-amendment-02.md)。

## P0. 契約を固定し、設計をレビューする

- 公開前提head152262b5/tree0dc1b2e4と、受入済み・画像取得済みの各headを保持する
- 実在するpnpm検査範囲の不足、追加のbraces検出、互換性のある公開修正版がないことを記録する。実行しないTauri検証用規則で除外しない
- 開発時の利便性の変更、変えないアプリ動作、限定的なpreviewのsecurity、optional peerの除去、全test計画を独立レビューする
- 最小限の契約差分とtest計画を親へ提示する。独立レビュー後、実装前に、自動更新から手動の再ビルド・再起動へ変えることについて依頼者の明示的判断を得る
- 必須の利用者向け動作やDocumentの正本意味論を変える必要があればSTOPとする

## P1. 限定した依存グラフを本当に削除できることを示す

P0レビューと実装権限が揃った後に限り実施する。

- 固定Node24.21.0/pnpm12.4.1を使う。直接依存のwebpack-dev-serverだけを削除し、サポートされたlock-only解決を使う。package scriptや全体のpeer-policy変更は行わない
- 変更前後の全グラフとlicense識別子を比較する。新しいpackage/version/licenseは承認されていない。想定外の変化があればinstall/use前に停止する
- 全importerでdev-server/http-proxy-middleware/micromatch/bracesの解決結果が消え、ソース変更後にwebpack-serve呼出しが残っていないことを確認する
- 2回目の依存解決で結果が変わらないこと、frozen・ignore-scripts install、両pnpm documentの完全なネイティブOSV走査を確認する。元の失敗と修正証拠を残す

## P2. 本番distプレビューをTDDで実装する

- loopback・固定root、安全なGET/HEAD asset、HTML deep-link fallback、lazy chunk、予約API/health path、missing assetに対するRED testを先に書く
- traversal、percent/double encoding、backslash、NUL/control、dotfile/map、symlink/hardlink、未知Host/method/upgrade、resource/timeoutのnegativeを加える
- dist欠落・未build、初回build失敗、使用中port、所有するprocessのstartup/readiness/shutdown、身元不明serverを再利用しないことをテストする
- Node組込み機能だけで、固定snapshotを実装する。proxy/HMR/watch/backendの動作は加えない
- 提供前に変更していない本番buildを実行し、本番CSPとの互換性を保つ

## P3. 既存の受入条件を保持する

- schema、generated-clientの最新性、TypeScript、Jest、MCP build/testを確認する
- 本番Webpack buildを変えない。必要なchunkとdev-server/HMR clientの不在を出力で確認し、アプリのstyleと利用者向けerrorを保持する
- mock E2Eの既存6機能testを全て維持する。assertion緩和・skip・retry増加は行わない。deep link/lazy chunk/history、focus/keyboard、reduced motion、API timingを含む
- 既存screenshot比較7件はdarwin限定である。Linuxの機能PASSでは完了しない。platform条件とmacOS baselineを保持し、完全なmock受入れを主張する前に、互換性と権限が確認できるcloud-macOSの独立visual gateを通す。runner/観察範囲は現時点で未解決であり、依頼者のMac利用、新規upload、Linux baseline再生成、自動的なvisual受入れは行わない
- 以前のcloud-browser拒否を回避せず、既に検証された所有下のLinux frontend/runtime jobで正確なheadのbrowser確認を行う。新たなscreenshot/artifact uploaderや公開範囲拡張は承認されていない
- 実際のDocument server/Postgres/workerによる、正確な新headのcomposition-root acceptanceを行う。D2の凍結source/harnessと全security/privacy/capture gateは変更しない

## P4. レビューして公開する

- 正確なsource/security/graph/testを独立レビューし、Critical/Importantを解消する
- 親が選ぶ場合はレビュー済みバイトを別Draftへ公開する。remote head/tree、実際に適用されるhosted gate、前提・画像取得済みheadの不変を確認する
- 開発者向けHMR・自動更新・コンパイルoverlayの喪失を正確に説明する。previewを本番runtimeやbusiness backendと呼ばない
- 依存・判定規則の適格性確認が完了して初めて、先のH1情報取得専用計画を再開できる。Runtime受諾・有効化は別扱いのままとする

この設計専用checkpoint自体は、実装・依存解決・build・server・browser実行の権限を与えない。
