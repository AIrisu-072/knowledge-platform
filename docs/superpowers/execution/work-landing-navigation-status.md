# 最初の画面と主ナビゲーション（案A）：実行状況

再開先：[計画](../plans/2026-10-08-work-landing-navigation.md)。branch `claude/upbeat-tesla-qcu94x`（main `6a34de3` から作成）。

## 2026-10-15 — PR128統合後mainへの追従境界

- ローカル保存head `f6437bbc` に、PR128統合後のmain `29026cc7486070410eb0b892e63c43eb7d1f5de5` を通常mergeし `1f2b1ff0` とした。競合なし。親からPR128 exact-headの14 CI成功と統合後CI `38765965535` 実行中の引継ぎを受けたが、この作業ではGitHubの結果を独自取得しておらず、統合後CI成功とは記録しない。
- 追従による変更は `Cargo.lock`、`experiments/search-discovery-poc/Cargo.lock`、`experiments/search-http-client-poc/Cargo.lock` の3fileのみ。それぞれrustls `0.23.45` → `0.23.46` とchecksum更新。Searchの機能sourceは取り込んでいない。
- `f6437bbc` と追従headの `apps/`・`packages/`・Node lock/workspace差分は空で、GUI製品・試験・依存は同一。したがって全画面1838件の無意味な再実行は行わず、そのsourceに対する前節の証拠を保持する。新しいRust依存の組合せ資格を旧GUI成功から主張しない。
- 追従headで固定Node 24.21.0のschemas check、画面・organization-runtime・desktop-bridgeの型検査がすべて成功。差分・空白検査も成功。push・PRのmain統合・CI削減なし。
- 次のexact action：親へこの保存headを引き渡す。PR123のmain統合後にさらに通常追従し、双方の変更とactiveを保持した最終headを独立review・正規CIで資格化して、同じPR112へ公開する。ここではまだpushしない。

## 2026-10-15 — 初回判定が応答しない場合の復帰補修

- PR112 head `07f572be` の独立reviewでP2を確認。3秒後に文書一覧を開いても元のsession要求はpendingのままで、errorだけを対象とする再確認へ進めない。Desktopのproxyは180秒、Browserは画面側の有限上限が無かった。
- 判定要求だけに10秒の期限とAbortControllerを追加し、ヘッダー待ち・本文待ちのどちらも「分からない」として既存の10秒再確認へ進める。3秒のlanding、404判定、10秒以内に遅れて成功した結果の共有、通常タスク画面のsession読取契約は保持する。新しい製品機能や設計承認は追加しない。
- 反例2件を先に追加し、修正前は両方 `Expected: error / Received: pending` で失敗。修正後は同じ文書画面のメニューが再確認で復帰し、期限後の旧応答が新しい結果を上書きしないことも確認した。
- 作業先は専用detached worktree `knowledge-platform-work-nav`。push・mergeなし。検証結果と次のexact actionは本節の追記を参照。
- 旧base `07f572be`＋補修の検証（固定Node 24.21.0）：関連3 suites59件とタスク・担当/委任2 suites104件成功。schemas check、画面・organization-runtime・desktop-bridgeの型検査、本番webpack build成功（既存3 warning）。全画面は75 suites1776件中1775成功、`document-move.test.tsx:175`「閉じた移動の遅いfocus復帰はreopen-idle後のfocusを上書きしない」1件失敗（366.553秒）。この失敗は保持し、mainとの組合せで別に再確認する。mainの一覧focus修正との関連を推測したが、別のダイアログ試験なのでその推測は撤回した。
- 次のexact action：この補修をローカルcommitに保全 → 承認されたmain `ead6830e` を通常merge（双方activeとmain製品を保持）→ 組合せの画面試験/型検査 → 親へ引渡し。最終main追従・独立review・exact-head CI・push/merge判断は親が行う。旧baseの成功を組合せheadへ転用しない。

### main追従と文書移動試験の切分け

- 補修 `d59c2f2b`、main `ead6830e` の通常merge `7001f589`。競合はactive先頭だけで双方の節を保持。main比の製品差分はPR112と補修の6fileだけで、mainの文書原本・viewer・focus・Desktop proxyを巻き戻していない。
- `7001f589` の全画面は79 suites1838件中1837成功、同じfocus試験1件失敗（312.885秒）。schemas check・画面/organization-runtime/desktop-bridgeの型検査・本番build成功（3 warning）。旧baseの結果とは別の証拠。
- 実main `ead6830e` の参照checkoutで同じ依存・固定Nodeを使い `reopen-idle` 単体を1回実行すると成功（5.456秒）。PR組合せも同じ単体指定を1回実行して成功（3.705秒）。したがってmain対照だけではPR起因と断定しない。hook無効controlも1回成功したが、補助根拠としてだけ保持する。全体失敗を成功扱いへ置き換えない。
- 独立診断と照合し、旧harnessがwindow RAF全体を捕捉しcancelも再現せず、旧close処理とReact Ariaのfocus containmentをまとめて再生する点を補修。close操作が最初に登録するRAFだけを保持し、他のRAFとcancelは本来のschedulerへ渡す。再open-idleでは旧triggerのfocus呼出しを監視し、ライブラリが新dialogへfocusを収める正常動作と区別する。
- harness補修後は文書移動全87件成功（15.358秒）。本番close callbackの世代ガード `isCurrent(closing, key)` だけを一時除去すると、旧trigger.focusが1回呼ばれ `Expected calls: 0 / Received: 1` で単体RED（2.764秒）。世代ガードを完全復元し本番ファイルがHEADと同一であることを確認してから、最終全画面・型検査を開始した。assert緩和やskipで失敗を隠していない。
- ログ：`/tmp/pr112-work-nav-jest.log`（旧base）、`/tmp/pr112-work-nav-main-jest.log`（組合せ・旧harness）、`/tmp/pr112-main-reference-focus.log`（実main対照）、`/tmp/pr112-main-combination-focus.log`（PR単体対照）、`/tmp/pr112-move-harness-green.log`（87件）、`/tmp/pr112-move-harness-mutation-red.log`（世代ガード除去RED）。これらはローカル証拠でありCIやWindows実機の資格ではない。
- 最終harness補修後の全画面は79 suites1838件すべて成功（240.348秒、`/tmp/pr112-work-nav-final-jest.log`）。同じ最終sourceで画面・organization-runtime・desktop-bridgeの型検査も成功。製品sourceは本番build成功時から変更していない。
- 次のexact action（更新）：harnessとこの証拠をローカルcommitへ保存し親へ引渡す。親が独立review結果を確認し、必要な最終main追従とexact-head CIを行う。push・PRのmain統合・CI削減は本作業では行わない。Windows実機と実server E2Eの新head再資格も未実施。

## 2026-10-08 — 実装と検証

- 背景：PR #103（デスクトップ版、main `8d4b94a`）の確認で、デスクトップ版ではタスク・検索の画面へ画面の操作だけでは移れないことが分かった。調べると、承認済みの設計（主ナビゲーションはタスク・文書・検索、最初の画面はタスク）に共通画面の実装が合っていなかった。依頼者が案A（Work APIを使えるserverでは設計どおりにし、文書だけのserverは今のまま）を指示。
- 実装（`ced8ed6`）：`GET /v1/organization/session` の成功でWork APIあり、404で無し、それ以外は分からない（今までどおり）と判定し、結果だけをQueryに保持。`/` はありならタスク、それ以外は文書一覧（判定は最大3秒）。AppShellはありならどの画面でもタスク・文書・検索を出す。
- 画面試験を先に追加（`test/work-availability.test.tsx`、16件）：実装前に新しい動作の10件が失敗し、今までの動作の6件は成功することを確認してから実装。
- 実serverのE2E（CIで実行）：organization-serverで文書画面を直接開いてもメニューにタスク・検索が出る。document-serverでは `/` の判定の要求が404で、文書一覧が開き、メニューにタスク・検索が出ない。
- デスクトップの実GUI確認：起動直後はタスク画面でメニューが5項目、メニューから文書・タスクへ移る、接続先が無いときは今までどおり文書の画面と文書のメニュー。文書の画面から始めていた場面は、メニューの「文書」から開くよう変更。

## 独立reviewの指摘と対応（2026-10-08）

`ced8ed6` の独立review（読み取りのみ、CIで動く試験への影響も確認）。

- **Important：最初の判定が失敗すると、同じ画面に留まる限り確かめ直さない。** デスクトップ版がbackendより先に起動した、backendが一時的に502・504を返した等で、文書画面のメニューにタスク・検索が出ないまま、URL欄の無いデスクトップ版ではタスク画面へ移れない。分からない間だけ10秒ごとに確かめ直し、あり・無しが分かれば止めるよう修正（`4233c71`。画面試験2件を先に追加し、修正前に失敗することを確認）。
- Minor：ブラウザーでの `/` → タスクがCIで確かめられていない。organization-serverはブラウザーの `/` をserver側で `/tasks` へ移すため、ブラウザーのE2Eでは画面側の `/` の処理を通らない。画面側の分岐は画面試験（最初の画面の判定）とデスクトップの実GUI（起動直後）で確かめている。対応しない。
- Minor：organization-serverで文書画面などを直接開くと、判定が返るまで文書の表示で描かれ、その後Organizationの表示（メニューの幅、ロゴの表示）に切り替わる。記録のみ。
- Minor：`/` は判定の間（最大3秒）何も表示しない。backendが応答しない場合だけ。記録のみ。
- Minor：判定が失敗した場合、起動時に判定の要求が2回出る（`/` と画面）。影響なし。
- Minor：判定は利用者情報の解読まで成功したときに「あり」になる。将来、認証が入って401・403が返ると「分からない」になる。今のPoCは固定の合成利用者のため起きない。本番の認証を設計するときに見直す。
- Minor：判定が3秒を超えると、実GUI確認の前提（起動直後がタスク）が崩れる。遅いbackendの場合だけ。
- 確認して問題なしとされた点：Queryの削除・無効化が判定の結果に触れないこと、Routerの非同期の転送、jestでの `fetch` の扱い、mockのE2E（404）、デスクトップのbridge E2E（503）、document-serverの404、実serverのE2Eの固定（source照合）、閉じる操作の場面が両方ともタスク画面で測ること、architectureの規則。

## 実GUI確認の手順の修正

起動直後がタスク画面になったことで、確認の手順に2件の不安定さが出た（製品の不具合ではない）。

- 閉じる操作の場面：X操作で押す位置をWebDriverで測る際、行を選んだ直後は右の詳細欄が読み込み中で「詳細を開く」の位置が動く（測るたびに586・607、落ち着くと568）。1.5秒後にX操作で押すと位置がずれ、確認が出なかった（`run-UXiQVC`）。WebDriver側もX操作と同じ待ち時間の後、位置が動かなくなってから測り、落ち着くまでの時間をX操作の待ち時間に足すよう修正（`89c2858`）。この場面だけの実行で、修正前は2回とも失敗、修正後は3回とも成功。
- 作業ファイルの場面：起動直後のタスク一覧は文脈の読み込み後にまとめ直されるため、見つけた要素が押す前に置き換わった（`run-FGlsbl`、stale element reference）。置き換わっていたら探し直して押し、起動直後はタスク一覧が出るまで待つよう修正（`b0aae6e`）。

## 新しい判断（承認済みとして扱わない）

1. Work APIの有無を `GET /v1/organization/session` の結果で判定し、404だけを「無し」とする。通信できない・5xx・デスクトップshellの502〜504・形の不正は「分からない」として今までどおりの表示にし、次に画面が開いたとき、または10秒ごとに確かめ直す。
2. `/` は判定を最大3秒待ち、過ぎたら文書一覧を開く。
3. メニューの「編集作業」「文書履歴」は残す（設計は3項目だが、2026-10-05・06の指示で追加されたもの。今回の指示の範囲外）。

## 検証（ローカル、Linux）

- 画面：全75 suites／1772件、型検査、本番build（固定版Node 24.21.0）。実serverのE2E（`e2e-organization`・`e2e-runtime`・`e2e-desktop`）の型検査。
- デスクトップの実GUI：commit `b0aae6e` で `run-gqJgEX`・`run-yzGxlM`・`run-klGvzO`（09:07–09:21 UTC、連続3回）、26シナリオ・227項目がすべて成功し `qualifying: true`（画面110、WebDriver 2、プロセス2、ページのscript 40、IPC 39、IPC・ディスク2、ディスク20、ログ3、準備9）。実行ファイル（debug build）のsha256 `989fefffd309ff77b1a90ed20abb6eb10f8e682a7336d6ace89739a6ff3eb9d6`。途中の `ced8ed6` で連続2回成功（`run-S2In3W`・`run-bncwwq`）、`4233c71` と `89c2858` でそれぞれ1回失敗（上の「実GUI確認の手順の修正」）。
- 画面（`4233c71` 後）：全75 suites／1774件、型検査、本番build。
- 実serverのE2E（CI）：`b697afc`・`4233c71` の `document-poc-runtime` で、document-server（`/` の判定が404で文書一覧）とorganization-server（文書画面を直接開いてもタスク・検索）の受入がどちらも成功。
- 気づいた点（今回の変更の前からある表示。変更していない）：Organizationの表示では主ナビゲーションの幅が狭く、文書画面のフォルダー操作のボタンが何行にも折り返す。これまではタスク画面を開いた後だけだったが、Work APIのあるserverでは文書画面を直接開いても出る。デスクトップ版のタスク画面にも「ブラウザーPoC」の案内が出る。

## 未検証

- Windows実機・WebView2（依頼者の指示で後回し）。
- 実serverのE2E（e2e-organization・e2e-runtime）はCIだけで実行（ローカルでは型検査だけ）。

## 次のexact action

Draft [PR #112](https://github.com/AIrisu-072/knowledge-platform/pull/112) のCIと独立reviewの結果を確認し、指摘を修正 → CI成功 → Draft解除と統合 → 統合後のmain CIを確認 → 依頼者へ報告。
