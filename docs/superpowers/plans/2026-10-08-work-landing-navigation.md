# 最初の画面と主ナビゲーションを承認済みの設計に合わせる

## 目的と根拠

承認済みのOrganization Clientの設計（[product UX](../specs/2026-10-02-organization-client-v0-product-ux-design.md) §3「Primary navigation has exactly タスク / 文書 / 検索. Landing is タスク.」、[UI design](../specs/2026-10-02-organization-client-v0-ui-design.md)、[承認記録](../specs/2026-10-02-organization-client-v0-product-ux-approval.md)、10-07の各改訂）に、共通画面の実装を合わせる。

- 今の実装：`/` は常に文書一覧へ移る。メニューの「タスク」「検索」は、タスク画面を一度開いて利用者情報を読んだ後か、タスク・検索の画面にいるときだけ出る（10-04のブラウザーPoCで導入）。ブラウザー版はURLで `/tasks` を開いて使っていたため目立たず、URL欄の無いデスクトップ版（PR #103）で、タスク・検索へ画面の操作だけでは移れないことが表に出た。
- 依頼者の指示（2026-10-08、案A）：Work APIを使えるserverにつないだときは、最初の画面をタスクにし、メニューに常にタスク・文書・検索を出す。文書だけのserverでは今のままにする。ブラウザー版・デスクトップ版の両方に効く。

## 判断（この計画で決めたこと）

1. **Work APIの有無の判定**：`GET /v1/organization/session`（既存の `workApi.getSession`）の結果で決める。
   - 成功（200で形が正しい）＝あり。organization-serverはこのpathで401・403・404を返さない。
   - **404＝無し**。document-serverには `/v1/organization` の経路が無く、404を返す（既存のpreview・mockのE2Eも同じ）。
   - それ以外（通信できない、5xx、デスクトップshellの502・503・504、形の不正）＝**分からない**。今までの動作（文書の画面とメニュー）のまま。次に画面が開いたとき、または同じ画面に留まっていれば10秒ごとに確かめ直す（デスクトップ版がbackendより先に起動した場合など）。
   - 判定の結果（あり／無し）だけをQueryに保持する（`['work-api-availability']`、期限なし）。利用者情報そのものは保持しない（TaskHomePageの `['organization-session']` は従来どおり毎回取得）。表示の切り替えだけに使い、権限の判断には使わない。
2. **最初の画面（`/`）**：判定が「あり」ならタスク（`/tasks`、表示の指定なし＝担当の表示Profile）、それ以外は今までどおり文書一覧。判定は最大3秒待ち、それを過ぎたら文書一覧を開く（遅れて分かった結果はメニューに反映される）。organization-serverはブラウザーの `/` をserver側で `/tasks` へ移すため、この判定が効くのは主にデスクトップ版と、server側の転送の無い配信。
3. **主ナビゲーション**：判定が「あり」なら、どの画面でもタスク・文書・検索を出す（Organizationの表示）。タスク・検索の画面と、利用者情報を読み終えた後は、今までどおり判定の要求を出さない。
4. **変えないこと**：文書だけのserverの動作（最初の画面、メニュー、`/tasks` を直接開いたときの「タスクを開けません」）。Document・Searchの画面の中身とAPI。メニューの「編集作業」「文書履歴」（設計の「exactly 3」より後の2026-10-05・06の指示で追加されたもので、今回の指示の範囲外。設計との差は残る）。

## 実装と検証

1. 画面試験を先に書き、失敗を確認してから実装する（`test/work-availability.test.tsx`：判定の各場合、メニュー、取り直さないこと・確かめ直すこと、タスク・検索の画面で要求しないこと、最初の画面、上限時間、結果の共有）。
2. `src/application/work-availability.ts`（判定と最初の画面）、`AppShell`（メニュー）、`main.tsx`（`/` の行き先）。
3. 実serverのE2E（CIで実行）：organization-serverで文書画面を直接開いてもメニューにタスク・検索が出る（`e2e-organization/support.ts`）。document-serverで `/` の判定の要求が404になり、文書一覧が開き、メニューにタスク・検索が出ない（`e2e-runtime/document-runtime.spec.ts`）。
4. デスクトップの実GUI確認（ローカルのみ）：起動直後がタスク画面でメニューが5項目、メニューから文書・タスクへ移る、接続先が無いときは今までどおり文書。文書の画面から始めていた場面は、メニューの「文書」から開くよう変更。連続2回の `qualifying: true` を取る。
5. 全画面試験・型検査・本番build、独立review、Draft PR、CI、統合、統合後のmain CI。
