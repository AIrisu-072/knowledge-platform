# Desktop Workspace Runtime：実行状況

## 2026-10-08 — 4回目の独立reviewの修正（`fbe962a`、`6504778`）

- 4回目のreview（修正の確認、mainの機能がshellの制限で壊れていないかの全件調査、completeness critic）。「修正の確認」の担当がCPU負荷試験のコマンド内で止まったため、負荷試験を禁じて再実行した。成立した指摘を修正。
- **ウィンドウを閉じる操作で、ページの離脱確認が働いていなかった。** shellはウィンドウを即座に閉じ、ページの `beforeunload`（未保存の文案、処理中・結果未確認の操作）が一度も動かなかった。Linuxでは閉じる要求をいったん止め、`webkit_web_view_try_close` でページに閉じさせ、ページが応じたときだけ閉じるよう修正（`webkit2gtk` 2.0.2を直接依存に追加。依存の木には既存）。WebDriverの操作中は離脱確認が自動で承諾される（WebDriverの規格）ため、WebDriverを使わずに起動したアプリをX上の実際のクリックとキー入力で操作して確認した。Windowsは未実装（確認項目7に追加）。
- **デスクトップ版ではタスク・検索の画面へ画面の操作だけでは移れない**（起動時は文書の画面で、そのメニューに「タスク」「検索」が無く、URL欄も無い）。実GUIの確認はscriptでURLを指定して移動していたため見落としていた。入口は依頼者の判断事項として手順書の既知の制約に記録。
- 「ファイルを作成」の書き込み中にアプリが落ちると途中のファイルが残り、画面からは完成できない（承認済みの設計の帰結）ことを、既知の制約と復旧の表に記録。
- 画面：作成フォームが状況変更を説明したら、一覧側の同じ説明は消す（再試行の成功後も残っていた）。shell試験：API定義のheader一致を `$ref`・path単位・引用符付きYAMLまで確認。
- 実GUIの証跡：確認手段の分類の残り（WebDriver、IPC、scriptによる操作）を修正し、「X操作」を追加。タスク画面の接続先未設定時の表示を追加。修正前のshellでの失敗の確認は、証跡の条件を満たさない参考の実行として記録。実行は固定版のNode 24.21.0で、`mise run desktop:gui:e2e` と同じ手順（画面の本番build、shellのbuild、backendのbuild、harness）を個別に実行した（この環境のmiseはrepositoryの設定を信頼済みにしていないため）。
- 検証（ローカル、Linux）：
  - 実GUI：`run-XbKm2e`・`run-NKcwUg`（commit `6504778`、Node 24.21.0、連続2回）で26シナリオ・217項目がすべて成功、`qualifying: true`（画面103、WebDriver 2、ページのscript 40、IPC 39、IPC・ディスク2、ディスク20、ログ3、準備8）。参考：閉じる処理を外したshellでは閉じる操作の場面が失敗（`run-AR2LPn`）。
  - shell：単体28件・設定固定6件・transport shim 5件、clippy -D warnings、fmt、desktopの `cargo deny check`。architecture-lint成功。
  - 画面：全69 suites／1660件、型検査、本番build（Node 24.21.0）。
- 未検証：Windows実機・WebView2・MSVC build（依頼者。確認項目7に閉じる操作を追加）、Windows版broker（未実装）、Windowsでの閉じる操作の離脱確認（未実装）、macOS、手順書の「Linuxで未確認」の項目。
- 依頼者の判断事項：デスクトップ版の入口（タスク・検索の画面への導線、または起動時の画面）。
- 次のexact action：push → exact-head CI → Draft解除と統合 → 統合後のmain CIを確認 → 最終報告。

## 2026-10-07 14:40 UTC — 3回目の独立reviewの修正（`d18df99`）

- 2回目のreviewは、最後の検証1件（強制終了の確認。対象は `6a7fae8` で修正済み）がコマンド内で止まったため打ち切り、修正commitの確認と抜け漏れの確認を3回目として実施（12件中11件が再現つきで成立、1件は反証）。
- **重要：デスクトップ版でタスク画面の作業ファイル保存が必ず失敗していた。** mainから取り込んだPR #102の作業ファイル内容のPUTは、操作ID・期待revision・担当の専用header（`x-operation-id`・`x-expected-revision`・`x-acting-assignment-id`・`x-expected-artifact-revision`）を使うが、shellの要求header許可リストに無く、serverが422を返していた。
  - 4種を許可リストに追加。API定義（`spec/api/`）のheader parameterがすべて許可リストにあることを試験し、loopbackの試験用serverへのPUTで4種が届き名乗り用headerは届かないことを試験。
  - 実GUIに「タスク画面の作業ファイル」の場面を追加（画面からファイルを選ぶ→保存済み→「ファイルを取得」でDownloadsの内容が一致）。修正前の許可リストのshellではこの場面が失敗することも確認。
  - 手順書の「既存の画面がそのまま動き」を、実GUIで確かめた範囲に合わせて修正し、タスク画面の未確認の操作を一覧に追加。
- 画面（ローカルWorkspace）：読み取り中に「開く」・一覧の失敗が起きたとき、遅れて終わった読み取りの内容を表示しない（世代で判定）。2ページ目以降からの「開く」で古いページを取り直さない（取得は1回）。状況変更以外の一覧の失敗は、一覧の取得が成功したら消す。「開く」で作成フォームの失敗表示も消す（入力は保持）。状況変更の説明は、作成フォームが同じ説明を出しているときは一覧側に重ねて出さない。aria-disabledのボタンも無効表示。いずれも画面試験を先に追加し、修正前に失敗することを確認。
- shell：本文の途中での時間切れ（504）、応答の上限ちょうど・1バイト超過（宣言・逐次）、要求の上限試験を空きportで行う（8080番portで開発用serverが動いていても結果が変わらない）。設定固定の試験の `cargo metadata` から `--offline` を外す（新しい環境では、host以外の対象のcrateが未取得で失敗していた）。
- 実GUIの証跡：確認項目の名前の先頭に、何で確かめたか（画面／IPC／ページのscript／ディスク／ログ／準備）を付け、report.jsonの `checkCounts` に集計。前回の「205項目のうちIPC 42・ページのscript 3、残りは画面」は誤りで、IPCは38、ページのscriptによる確認は約29件が画面の確認に数えられていた。処理中のボタンの見た目は、scriptでボタンを無効にする確認から、実際のWorkspace作成の応答を一時的に止めた間の確認に変更。
- 作業プロセスの再起動でDockerのdaemonが止まっていたため起動し直した（実GUIの準備で失敗した実行 `run-glAok2` はこのため）。
- 検証（ローカル、Linux）：
  - 実GUI：`run-p5Dill`・`run-29mvyi`（commit `d18df99`、連続2回）で25シナリオ・212項目がすべて成功、`qualifying: true`（画面105、ページのscript 36、IPC 38、IPC・ディスク2、ディスク20、ログ3、準備8）。
  - shell：単体28件・設定固定6件・transport shim 5件、clippy -D warnings、fmt、desktopの `cargo deny check`。変異27件（前回の21件＋専用header2件・本文の時間切れ・上限の境界3件）をすべて検出。architecture-lint成功。
  - 画面：全69 suites／1659件、型検査、本番build。
- 未検証：Windows実機・WebView2・MSVC build（依頼者が実施）、Windows版broker（未実装、fail-closed）、macOS、手順書の「Linuxで未確認」の項目（タスク画面の作業ファイル以外の操作を含む）。
- 次のexact action：push → exact-head CI → 4回目の独立review（今回の修正の確認）→ 指摘があれば修正 → Draft解除と統合 → 統合後のmain CIを確認 → 最終報告。

## 2026-10-07 10:10 UTC — 2回目の独立reviewの修正

- 2回目の独立review（4観点＋反証の検証＋completeness critic）の指摘を修正（`6a7fae8`、`3abfeca`）。
- shell：
  - Tauriが実際に使う設定を固定：`tauri.<platform>.conf.json` 等の上書きファイルが無いこと、build時の `TAURI_CONFIG` を拒否（build.rs）、解決後の設定（CSP・capability・window・asset protocol等）、`cargo metadata` で解決したTauriのfeature。上書きファイルを置くと、ファイル一覧の試験と（再build時に）解決後の設定の試験が失敗することを確認。
  - 転送の上限を値として持たせ、loopbackの試験用serverで413・502（宣言・逐次の超過、途中切断、接続不可）・504・正常応答（データ化とheader）を単体試験。main window以外の拒否も試験。それまで手順書は「単体試験あり」としていたが、実際は判定関数だけだった（review指摘）。
  - 変異21件（前回の15件＋上限・時間切れ・途中切断・main window判定）をすべて検出。
- 画面：「開く」で同じ場所を開き直しても作成フォームの入力を消さない（作り直さず、一覧の取り直しと表示のリセットだけ）。自動の再取得で状況変更の説明をすぐ消さない。ダイアログ内の無効なボタンも無効表示。いずれも画面試験を先に追加してRED→GREEN、一覧の再取得失敗時にプレビューを隠す試験は該当行を外すと失敗することを確認。
- 実GUI確認（harness）：画面の状態の確認は画面で判定し、IPCで記録を見る確認は「IPC：」、ページのscriptで試す確認は「ページのscript：」と明記。強制終了は書き込みの途中で止まるまで最大3回試行。応答の置き換えは「型の無い失敗応答」と記述し、応答が永久に返らない場合は未対応として手順書に記録。開いているフォルダーの解除、再読み込みの印、IPC応答の絶対path検査、停止済みprocess groupへ再送しない、docker run中の中断でもcontainerを残さない（Ctrl-Cで確認）、中断の後片付け中はdriverを起動しない（中断試験でdriverが1つ残ったため）、staleness判定の入力追加。
- 文書：手順書（未確認一覧の追加と訂正、debug buildの開発者ツール、GTKの選択画面が存在しないフォルダーを作る仕様、Windows項目4・5）、計画のW2番号、実装差分、dependency-rulesの説明。
- 前回の記録の訂正：`1a393d7` の時点では、手順書の24シナリオの結果と「証拠とした実行」は存在せず（その時点の唯一の実行 `run-uYeNzS` は失敗）、後のcommitで揃えた。`baad3bc` での2回目の実行（`run-bcbfgy`）は、`xdotool type` が「第」を落とし、GTKの選択画面が「二フォルダー」を作って返したため4シナリオが失敗（harnessの問題。`1193ace` で貼り付けに変更）。
- mainの更新（PR #100〜#105、Document詳細・タスク画面・organization-serverの変更を含む）を `21d5209` で取り込み（衝突なし）。取り込み後の実GUIで、文書一覧が読み込み後に選択行へfocusを移す動作（Document画面の既存の動作）と最初のTabが競合して1回失敗したため、skip linkの確認を共通ShellのままローカルWorkspace画面で行うよう変更（`c31e6d0`）。
- 検証（ローカル、Linux）：
  - 実GUI：`run-YnbwvF`・`run-VDyd9g`（commit `3abfeca`、連続2回）と、main取り込み後の `run-AZ0Gve`・`run-O8wwVK`（commit `c31e6d0`、連続2回）で、いずれも24シナリオ・205項目がすべて成功、`qualifying: true`。
  - shell：単体23件・設定固定6件・transport shim 5件、clippy -D warnings、fmt、desktopの `cargo deny check`、変異21件をすべて検出。architecture-lint成功。
  - 画面：全69 suites／1654件（main取り込み後の `c31e6d0`。取り込み前の `89f613c` では64 suites／1562件）、ローカルWorkspace画面の試験21件、型検査、本番build。
- 未検証：Windows実機・WebView2・MSVC build（依頼者が実施）、Windows版broker（未実装、fail-closed）、macOSでの実行、手順書の「Linuxで未確認」の項目（応答が永久に返らない場合を含む）。
- 次のexact action：push → exact-head CI → 2回目reviewの残りの検証結果とcriticを確認 → Draft解除と統合 → 統合後のmain CIを確認。

## 2026-10-07 09:20 UTC — 独立review（1回目）の修正と、実GUI確認の拡充

- PR：[#103](https://github.com/AIrisu-072/knowledge-platform/pull/103)（Draft）。前回の記録以降のcommit：`0075241`（tauri-driverをmiseの `[tools]` から外す。CIの `mise install rust` が追跡対象の `mise.lock` を書き換え、作業ツリー検査が失敗していたため）、`38be3f8`（1回目reviewの修正：遷移元originの完全一致、transport shimの素通し、XDGの無い端末のダウンロード先、接続先の形式誤りの表示）、`a621559`、`1a393d7`、`ef53c5c`、`d2d9fea`、`baad3bc`。
- `38be3f8` のCI（run 37589403315）は全job成功。security jobでdesktop lock（420 package）のOSVが「No issues found」（承認済みの2件だけ除外）。
- 1回目reviewの残りの修正（`a621559`）：
  - `/v1` 応答をデータとしてだけ返す（JavaScript等は `application/octet-stream`、CSP sandbox）。実アプリで、JavaScriptとして登録した原本を `<script>` で読み込むとアプリのoriginで実行できることを確認してから修正し、修正後は実行されないことを確認。
  - desktop用deny.tomlのライセンス例外を `=` の厳密版指定へ（裸の版はcargo-denyでは範囲指定になることを確認）。
  - capabilities・tauri.conf.json・Tauri feature・build.rsを `tests/config.rs` で固定（変異で検出を確認）。
  - ローカルWorkspace画面：「開く」で必ず取り直す、成功で失敗表示を消す、一覧の失敗時に古いプレビューを隠す（画面試験を先に追加）。
  - THIRD_PARTY_NOTICESが配布用の完全な通知ではないことを明記。
- 実GUI確認の拡充（`1a393d7` ほか）：review・completeness criticが挙げた未確認項目を実アプリで確認するシナリオを追加（応答の消失による結果不明、書き込み中の読み取り、8MiBの上限、ページ送り、利用中フォルダーの解除、二重送信・二重クリック、PATCH・multipart PUT、担当と委任、不正な接続先、強制終了、iframe・遷移・新規ウィンドウ）。証跡の欠陥（CSPの判定がCORSで成立していた、WebKitが正規化する脱出形式、検査場所の誤り、未実行のシナリオがあってもpassed、Xvfbの番号衝突、中断時の後片付け等）も修正。
- 実GUIで見つけて直したもの：未確定の操作で止めたボタンが、有効なボタンと同じ見た目だった（cursor・色を無効表示へ。GUIで変更前の状態を確認してから修正）。
- 確認の途中で直したharness側の問題（いずれも製品コードではない）：Tauriの `__TAURI_INTERNALS__.invoke` は書き換えできないため、応答の消失はLinuxのcustom protocol IPCが呼ぶ `window.fetch` で注入。modalの裏の同名ボタンを押していた。強制終了がbrokerの作成開始より前に届いていた（書き込み開始後にSIGKILLするよう変更し、8MiB中4MiBの時点で停止→再送で収束を確認）。skip linkの確認がWebKitの順次移動の起点に左右された。
- 検証（ローカル、Linux）：
  - 実GUI：`run-43HUha`（commit `baad3bc`、commit済みの作業ツリー）で24シナリオ・197項目すべて成功、`qualifying: true`。詳細と未確認の一覧は[手順書](../../operations/desktop-workspace-runtime.md)。
  - shell：単体14件・設定固定3件・transport shim 5件、clippy -D warnings、fmt、desktopの `cargo deny check`。変異15件（転送先の `/v1` 判定、要求・応答header、loopback、接続先の正規形、Origin・Referer、OPTIONS、応答の型・sandbox・`+json`、遷移元のuser情報とorigin、ダウンロードのblob判定とDownloads直下）をすべて検出。
  - 画面：全64 suites／1560件のうち1件（Documentの移動ダイアログのfocus復帰。この変更の対象外のファイル）が、shellの変異試験のbuildと並行した高負荷時に失敗。単独では3回連続84/84成功。型検査、本番build。
- 未検証：Windows実機・WebView2・MSVC build（依頼者が実施）、Windows版broker（未実装、fail-closed）、macOSでの実行、手順書の「Linuxで未確認」の項目。
- 次のexact action：独立review（2回目）の結果を確認・修正 → push → exact-head CI → Draft解除と統合 → 統合後のmain CIを確認。

## 2026-10-07 07:30 UTC — Tauri shellの実装と実GUI確認（依頼者が判断5項目に合意）

- 依頼者が判断5項目すべてに合意し、「クラウド環境でTauriを構築し、デスクトップアプリのGUIから確認する。CIにはしない」と指示。[判断事項](../../decisions/2026-10-07-tauri-v2-desktop-qualification.md)へ合意内容と適用を記録しました。
- 基点：PR95統合後のmain `04076b1`（PR96を含む）。branch `claude/upbeat-tesla-qcu94x` をmainから作り直しました。main `04076b1` のCI run 37578317667は全job成功で、PR95統合直後（`93947f3`）のrust-test失敗（無関係なDocker pullの切断）は後続mainで解消済みです。
- 実装：`apps/desktop/src-tauri`（独立Cargo workspace、Tauri 2.12.1）。単一window、IPC 1件、rfdのフォルダー選択、URL schemeの置き換えによるasset配信と `/v1` 転送（loopbackの1 originだけ）、遷移・新規window・downloadの制限、desktop用のdeny.toml／osv-scanner.toml、architecture-lintの境界、ローカル専用のmise task（`desktop:check`／`desktop:build`／`desktop:gui:e2e`）。
- 実GUIで見つけて直したもの：
  - WebKitGTK 2.52がcustom schemeへのBlob/FormData本文でSIGSEGV（gdbで確認、文字列・バイト列は正常）→初期化scriptで本文をページ内で確定。
  - WebKitGTKは同一originの要求にOriginを付けない→Origin必須をやめ、「付いていれば一致」の多層防御へ（RED→GREEN）。
  - `<a download>` のblob URLが遷移制限で拒否されていた→自アプリのblob URLだけ許可（RED→GREEN）。
  - ローカルWorkspace画面：同じフォルダーの「開く」で再取得しない／置換検出後も古い一覧を表示／二重起動の理由を表示しない→いずれも画面試験を先に追加して修正。
- 検証（ローカル、Linux）：
  - shell：単体10件、変異4件（転送先・header・loopback・Referer判定）をすべて検出、clippy -D warnings（Linux、`x86_64-pc-windows-gnu`）、fmt、`cargo deny check`（desktop）。
  - root：fmt、`cargo deny check`、architecture-lint、assurance scan/plan/run/report、repo:policyの追跡物検査、gitleaks（今回のcommit範囲0件）。
  - GUI：全64 suites／1557件、型検査、本番build。
  - 実GUI（`mise run desktop:gui:e2e`）：17シナリオ・104項目がすべて成功（tauri-driver 2.1.0、WebKitWebDriver、WebKitGTK 2.52.6、Xvfb、xdotool、PostgreSQL 18.6、organization-server）。証跡は `apps/desktop/e2e/.state/run-*/`（git管理外）。
- 未検証：Windows実機・WebView2・MSVC build（依頼者が実施、[手順](../../operations/desktop-workspace-runtime.md)）、Windows版broker（未実装、fail-closed）、macOSでの実行、OSVの実送信（この環境からapi.osv.devへは接続不可。PRのsecurity jobで確認）。
- 次のexact action：独立review（実行中）の指摘を確認・修正 → push → Draft PR → exact-head CI（特にsecurityのOSV） → 統合 → 統合後のmain CIを確認。

## 2026-10-07 04:20 UTC — 修正commitの再reviewと追加修正

- 修正commit（c5f9564/4df588a）の独立再review：
  - 旧指摘1〜5、10、11は修正済みと確認されました。
  - 6、7、8、13、14は一部修正にとどまり、新しい欠陥が5件見つかりました。そのうち4件は再reviewで実際に再現されています。
- 再現された新規欠陥（N1、N2、N4、N5）と、上限処理の問題（N3）の修正（3446677/af55c22）。いずれもREDの反例試験を先に追加しています。
  - N1（Medium）：再試行が、利用者による同じファイルの上書き編集を消し得ました。意図bytesの真の接頭辞（自分の書込み途中）である場合だけ作り直し、それ以外はconflictとして保持します。
  - N2（Medium）：結果不明のまま解除できたため、画面が固着しました。未確定の間は解除・追加・名前変更を無効にします。
  - N3（Low）：保留中の作成予約に上限が無く、直前に追加した記録が押し出されることがありました。予約を16件に制限し、記録の上限を1024件へ広げ、直前に追加した記録は押し出しません。
  - N4（Low–Medium）：再確認が `stale_context` になると、操作を破棄していました。操作IDを保持し、Workspaceを更新して再確認します。
  - N5（Low）：作成ダイアログを閉じられませんでした。「あとで確認する」を追加し、一覧から同じ入力で再開できます。
  - 応答の照合を追加しました：名前変更・解除の応答のWorkspace、解除済みのbinding、回復receiptの操作ID。
- 残存事項（記録のみで、このPRでは直していません）：
  - FreeBSD等ではerrnoを消去できず、一覧がfail-closedになります。
  - 種類を確認してから開くまでの間にFIFOへ差し替えられた場合の、短い競合（Linuxでは `O_PATH` 化で解消できます）。
  - musl版ではbtimeが無く、識別子の強化が効きません。
  - 作成直後、identityを記録する前にcrashすると、空ファイルが残ります。再試行は `AlreadyExists` になります。`O_TMPFILE`+`linkat` 化が候補です。
  - 作成receiptの `sha256` は形式だけを確認しています。
- 修正後のローカル検証：
  - broker：単体12、統合37、wire 5、合計54件。clippy、fmt。
  - GUI：全59 suites／1444件、型検査、本番build。
  - 既存mock E2E 6件、desktop-bridge E2E 6件（Chromium 1194）。
- 次のexact action：push → 新headの全必須CIを確認 → Draft解除と統合を判断 → 統合後のmain CIを確認。

## 2026-10-07 04:00 UTC — 独立reviewの指摘を修正（PR95）

- [PR95](https://github.com/AIrisu-072/knowledge-platform/pull/95)（Draft）。初回head `592ba99` のCI run 37567267684では、rust-testを除く全job（security、policy、rust-static、desktop-runtime-bridge、document-poc-runtime、portability-macos等）がSUCCESSでした。rust-testは実行中のまま、次のpushで置き換えました。
- 独立review（`d515aa3..5dbca37` を対象）で、実際に再現された欠陥が見つかりました。いずれもREDの反例試験を先に追加してから修正しています。
  - Important：registryの保存に失敗したとき、解除・追加・名前変更がメモリにだけ反映され、再試行で成功扱いになり、再起動で巻き戻った。
  - Important：操作記録が上限で押し出されると、同じ操作IDで2つ目のWorkspaceとmanaged rootが作られた。
  - 修正済み：binding外へ移された親フォルダーへの作成で、ファイルが孤立して残った（592ba99）。
  - Pending状態の作成の再試行で、contextを検査せず、他者の同一内容ファイルを採用し得た。
- 再現前の指摘（suspected/Minor）のうち、PR範囲内のものも修正しました。
  - network/FUSE/overlay上でのlease（ローカルFSの許可一覧に限定）
  - readdirのerrorを終端と区別していなかった
  - FIFO/特殊ファイルを種類確認の前に開いていた
  - binding rootの識別にbtimeを追加
  - 古いcontextのhandleが上限枠を占有していた
  - pickerがpanicした場合のticket解放
  - 画面移動で結果不明の操作が失われた（QueryClient単位のstoreで保持し、確定まで移動を止める）
  - IPC応答の照合（offset／世代／長さ／eof、receiptの操作ID／ref／size、一覧の親locator）
  - recoverWorkspaceをmutationとして扱う
  - SIGURGの扱いは文書に明記しました。
- 修正後のローカル検証：
  - broker：単体11、統合37、wire 5、合計53件。clippy、fmt、architecture-lint、macOS/Windowsの `cargo check`。競合系6件は8回連続PASS。
  - GUI：全59 suites／1441件、型検査4構成、本番build。
  - 既存mock E2E 6件、desktop-bridge E2E 6件（いずれもChromium 1194、新しいbuild）。
- 次のexact action：push → 新headの全必須CIを確認 → Draft解除と統合を判断 → 統合後のmain CIを確認。

## 2026-10-07 UTC — Runtime側の実装と検証（Tauri shellは判断待ち）

- 基点main：`d515aa38085c9ed7e41f8103d9c1a6c576025fd4`（引継ぎ時と同じで、作業開始時に再確認済み）。branch：`claude/upbeat-tesla-qcu94x`。計画は[こちら](../plans/2026-10-07-desktop-workspace-runtime.md)、契約差分は[こちら](../specs/2026-10-07-desktop-workspace-runtime-amendment.md)。
- 既存成果の区別：
  - mainに統合済み：Organization Product/Domain API/UI設計とPhase1–3の凍結、Browser PoC、preview、gitleaksの4 fingerprint。
  - mainに無い：PR50〜53は未統合のDraftです。内容は文書と、buildしない資格確認用のlockだけです。#52・#53のsecurity失敗は、資格確認lock内のglib/proc-macro-errorをOSVが検出したことによるもので、未解消です。
  - 未公開の候補：`7d64603`/`b4c221a` はrepositoryに実体がなく、再利用できません。
  - PR50〜53の内容は今回のPRに取り込んでいません。Tauri資格確認の結論は[判断事項](../../decisions/2026-10-07-tauri-v2-desktop-qualification.md)へ新しく記録しました。
- 完了（R1–R5）：
  - broker
  - wire（単一IPC・13 command）
  - frontend contract/adapter/Provider
  - `/local-workspaces` 画面
  - テスト専用stdio bridgeによるChromium通しE2Eと、CI job `desktop-runtime-bridge`
- TDDの記録：
  - brokerの統合試験31件と、wire試験4件は、stub実装に対してREDを確認してから実装しました。
  - 並行書込みでの混在snapshotは、実装後の反復試験で反例が出ました（12回中3回）。新しい試験でREDを再現（6回中2回）してから、read lease方式に修正しました。
  - inode番号の再利用で置換を見逃す反例も見つかり、statxのbtimeを識別子に含めて修正しました。
  - 作成途中のcrashからの復旧試験2件は、実装の後に追加した回帰試験で、初回からGREENでした。
- ローカル検証（Linux、Node 22.22／pin版はNode 24.21）：
  - broker：単体10、統合31、wire 4、合計45件PASS。clippy -D warnings、fmt、architecture-lint PASS（禁止patternの陰性確認も実施）。
  - `cargo check --target x86_64-apple-darwin|x86_64-pc-windows-gnu` は警告0です。
  - 競合系試験は10回連続PASS。
  - 全GUI：59 suites／1439件PASS（mainの1421件に新規18件）。型検査（app、document-poc-runtime、organization-runtime、desktop-bridge）、本番build（既存種別の性能警告3件）、preview試験36件。
  - 既存mock E2E 6件PASS。事前導入済みChromium 1194で実行しており、pin版1243ではありません。
  - desktop-bridge E2E 6件が3回連続PASS（同じChromium 1194）。
- 未検証：
  - Tauri shell（依存の追加・build・実行がSTOP）
  - WebView2
  - Windows 10 Pro／11の実機
  - Windows版brokerの動作。fail-closedのstubだけで、実装も未着手です
  - Document APIのdesktop経由の転送
  - macOSでの実行（`cargo check` のみ）
  - Linuxでの、他人所有ファイルやlease非対応FS上での読み取り（`unavailable` として拒否する設計どおりであることを試験していない）
  - pin版Chromiumでの実行（PRのhosted CIで確認予定）
- 判断待ち（依頼者）：MPL-2.0の例外（5件）、Linux専用advisoryの扱い、Windows CIか実機か、WebView2の規約、desktop lockのOSV送信。詳細は[判断事項](../../decisions/2026-10-07-tauri-v2-desktop-qualification.md)。
- Design Freezeとの差分：[実装差分](../specs/2026-10-07-desktop-workspace-runtime-amendment.md)のとおり。ローカル専用の論理Workspace、追加の応答値、単一IPC、lease付きsnapshot。依頼者の2026-10-07の依頼の範囲内として記録しました。server側のWorkspaceは予約済みで未実装です。
- 次のexact action：独立review → push → Draft PR → hosted CI（`desktop-runtime-bridge` を含む全必須job）→ 受入確認後にmainへ統合 → 統合後のCIを確認。Tauri shellは承認後に `apps/desktop/src-tauri` として追加します。
