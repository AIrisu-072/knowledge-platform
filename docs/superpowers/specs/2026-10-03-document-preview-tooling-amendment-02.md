# Document GUIツール変更02: 本番buildによるpreview

日付: 2026-10-03 UTC。状態: **提案。独立した設計レビュー・実装権限は未確定**。

## 理由と、置き換える契約の正確な範囲

固定版OSVのpnpm extractorは、最初のYAML documentしか読まない。
byte対応を保持した第2documentを忠実にネイティブ走査すると、webpack-dev-server6.0.0 ->
http-proxy-middleware4.2.0 -> micromatch4.0.8経由でbraces3.0.3が検出される。
現在、このadvisoryを解消する互換性のある公開patchはない。
証拠・STOP（元レビュー資料、7d646030に記録）と
削除案の比較（元レビュー資料、7d646030に記録）は、
実装やセキュリティ例外の承認とは別のものとして扱う。

置き換えを提案するのは、次のツール選択だけである。

1. [変更01](2026-10-01-document-gui-integration-v0-design-amendment-01.md)の
   webpack-dev-server6.0.0という選択: この依存を削除し、Webpack5.111.1、
   webpack-cli7.2.3、既存のReact・アプリケーション・build・testの選択を維持する
2. [計画追補01](../plans/2026-10-01-document-gui-integration-v0-production-implementation-addendum-01.md)の
   dev-server smoke手順: 本番buildによるpreview・security・起動の適格性確認と、
   変更しない全mock/実runtime受入検証で置き換える
3. 実装が承認された場合、spec/selection/library-tool-selection-v0.mdの既存Document GUI例外の
   そばに、この機能だけに適用する補足説明へのリンクを加える。
   他の機能のツールとHMRの選択は変更しない

規範となるarchitectureでは、標準の開発反復手順にFrontend HMR/buildを挙げており、
もう1か所のHMRへの言及はfilesystem/watchの性能を説明している。
本提案は、Document GUIに限定してbuild・手動refreshへ変更することを明示するものであり、
全体としてHMRが不要だと述べるものではない。
凍結済みGUI/APIやOrganizationの利用者向け受入条件に、HMRやwebpackのコンパイルoverlayを
要求するものはない。現在のhot:true/error-overlay設定は、実際に開発者へ利便性を提供しており、
それが失われることになる。アプリケーションのError/Conflict/Partial状態は、必須のまま変更しない。

**最小限の動作変更:** devとmock browserの起動時に既存の本番bundleを1回buildし、
固定snapshotをローカルで配信する。開発者は編集後に再build・再起動する。
HMR、自動refresh、browser上のコンパイルoverlayはない。
build errorはterminalに残し、server起動を必ず阻止する。
最初の変更ではwatch/reloadを対象外とし、変更中distやstale chunkをめぐる競合を避ける。

## 独立したNode組込みpreview: 新しいbackendは作らない

開発・テスト用asset serverをapps/document-web/scripts配下へ狭い範囲で追加する。
既存の固定版Node24.21.0の組込み機能だけを使い、第三者packageは使わない。
現在の同じReact本番Webpackコマンド、設定分岐、CSS抽出、minify、chunk、生成client、
business/UI sourceは変更しない。
古い開発専用devServer設定は削除してよいが、本番分岐のbyte・動作が同等であることを明示的に証明する。

previewは次の条件を満たさなければならない。

- 固定のdocument-web distだけを127.0.0.1:8080で配信する。利用者が変更できるhost、
  外部へのbind、任意のfilesystem root、reverse proxy、公開deploy modeは設けない
- 起動時に、検証した通常のbuild済みassetだけをsnapshot化する。ファイルごと・件数・合計byteの
  上限を設け、祖先path・配下要素のsymlink、hardlinkによる曖昧さ、index欠落・未buildを拒否し、
  単一の不変世代を保持する。配信snapshotから.map/dotfileを除外し、それらへのリクエストを拒否する。
  変更しない本番buildが出力する通常のsource mapを理由に、dist全体の起動を失敗させてはならず、
  このpreviewに合わせるために本番出力からsource mapを削除してもならない
- assetのGET/HEADと、拡張子のないHTML navigationに対するindex.htmlへのfallbackを保持する。
  asset欠落、危険なencoding/path要素、navigationではないリクエストを、成功したHTML応答にしてはならない
- /v1と/healthのnamespaceを配下も含めて拒否する。正本となるAPI/identity/workflow実装、
  mockのbusiness応答、隠れたfallbackを設けない
- 本番と互換性のあるCSP/security headerを使う。開発用eval bundleを配信するためにunsafe-evalを
  加えたり、既存の本番CSPを緩めたりしてはならない。本番Rustのheader/serverには触れない
- 想定外のHost/method/upgradeリクエストを拒否し、request/headerのtimeoutとresource利用を制限する。
  固定portが使用中なら明確に失敗させ、身元不明processを終了・再利用しない。
  shutdown時は自分が所有するlistener/connectionだけを閉じる
- /index.htmlなど、決定的な静的readiness pathだけを公開する。
  新しいアプリケーションhealth/business endpointや権限を伴うコマンドは導入しない

これは既存のNode開発用asset serverを置き換えるものである。
選択済み技術構成における、独立したJavaScript business backendの禁止は維持する。
既に適格性を確認したRust本番静的serverと管理下の実runtime harnessを、実際のAPI/runtime受入検証の
正本として維持する。凍結済みのOrganization D2の6-asset server、CSP、harness、画像取得済みPR48は、
再利用・拡張・編集しない。

## Script、mock test、lockの境界

実装候補pathは次に限定する。

- apps/document-web/package.json: dev-serverの直接依存を削除し、範囲を限定したpreview/unitコマンドを
  追加する。dev/e2eはpreview/browser起動前に、変更しない本番buildを実行する
- apps/document-web/webpack.config.cjs: 不要になったdevelopment-server blockだけを削除し、
  本番の全設定・出力の意味を保持する
- apps/document-web/playwright.config.ts: 管理下の起動・readinessだけを変更する。
  baseURL、browser/project、locale/viewport、1 worker、retryゼロ、assertionを保持する
- apps/document-web/scripts/preview.mjsとpreview.test.mjs: 隔離したserverとunit/negative test。
  API/client/React sourceは編集しない
- pnpm-lock.yaml: 固定版pnpmがサポートする削除・枝刈りだけを行い、新しい第三者package/version/licenseは
  追加しない。説明できないグラフ変更はinstall前に拒否する
- 既存mise検証entrypoint: 既に管理下にあるLinux frontend/runtime jobへ、preview testと変更しない
  mock E2E検証を狭く追加する。実runtimeコマンドを変更せず、artifact/image upload経路も追加しない
- この変更文書・計画、機能限定の選定補足、事実に忠実なreview/status検証記録

固定版pnpm12.4.1を使う。webpack-cliのdev-server peerはoptionalである。
全体のpeer policyを変更したり、optional dependencyを抑制したりしてはならない。
lock全体の再生成・固定点確認とfrozen installにより、document-webとdocument-mcpの両方で
dev-server/http-proxy-middleware/micromatch/bracesが解決されなくなったことを証明する。
既存pnpmの第1・第2documentは、引き続き実態どおりに走査する。宣言の削除だけでは不十分である。
package名の置換、ローカルfork、scanner除外は認めない。

既存mock E2Eの6 testと全assertionを保持する。lazy chunk、deep navigation、keyboard/focus、
capability/confirmationのtiming、JST表示、reduced motion/layout、Mock1–7のscreenshot比較を含む。
新serverを通すためだけに期待値を緩めたりbaselineを更新したりしてはならない。
既存mock routeはnavigation前に/v1をinterceptする。previewがそのbusiness応答の責務を
引き継ぐことはない。
既存mock testの7件のscreenshot比較は、darwin上だけで実行される。
したがって、変更なしのLinux上の6 test実行が検証するのは機能assertionであり、これらの画像比較ではない。
現在のmacOS baselineとplatform条件を保ち、互換性があり、明示的に承認されたcloud-macOS runner・
観察設計が用意されるまで、画像比較を別の未達gateとして記録する。
依頼者のMacを使う、比較を削除・skipする、Linux baselineを再生成する、Linux実行でvisual受入れが
完了したと主張する、といったことは行わない。
新たなscreenshot upload、恒久的なmirror、画像取得権限の変更は行わない。

標準のmock起動では、未知のlistenerを黙って再利用せず、自分のprocessを所有するべきである。
これは従来の非CI時の再利用設定からの、起動方法だけの変更として記録する。
HTML Accept headerによるfallback動作をreadinessと誤認しないよう、readiness probeは
/documentsから/index.htmlへ変更してよい。起動・cleanupを明示的にテストする。
変更しない本番buildは、readinessの時間計測を始める前に実行する。

## 保持する意味と公開範囲の境界

同一sourceによるReact/browser/Tauriの方針、TanStack Router/Query、Motion、reduced motion、
keyboard/focus、Documentのroute/client/binary bridge、Version/Revision/OCC/lifecycle/Diff、
capability認可、アプリケーションerror、正本APIを維持する。
CORS/CSPの弱体化、Business APIの追加、本番identityの選択、Document backendの変更は行わない。
必須の利用者向け動作や、Documentの正本としての意味を変更する必要が生じたら、STOPし、
その正確な矛盾を報告する。

受入済みPR43と画像取得済みPR48のsource branchは不変とする。
別のレビュー済み検証・ツール変更用Draftに、この変更文書と後の実装を含めることはできるが、
merge/deployは承認されていない。現在のTauriの2-advisory例外はbracesを含まない。
H1は完全な適格性確認を待つ。Windows Runtime/native/licenseのgateは維持する。

## 設計承認の境界

この文書は、現在のタスク指示で承認された読み取り専用の設計・計画作業である。
新preview、枝刈り後のグラフ、変更しない受入testが合格したという主張ではない。
独立レビュー、親による正確な契約・test計画のレビュー、開発時refreshのトレードオフに関する
依頼者の明示的判断が完了するまで、実装してはならない。
新しい脆弱性例外は要求しない。

## Browser PoCへの再利用時の注記（2026-10-04）

元レビューのTauri/依存調査資料は別trackの履歴であり、この最小PoCには同梱しない。本変更の承認・動作範囲は上記本文と同梱承認記録を保持する。Tauri fixture・guard・新たなscanner除外は取り込まない。
