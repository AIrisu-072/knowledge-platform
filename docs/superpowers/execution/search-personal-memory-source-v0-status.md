# Search Personal Memory Source v0：状況

Status: PROPOSED / 設計判断待ち（実装は未着手）

## 2026-10-15 — 設計と計画の草案（KP-0）

- 基点 main `ead6830e28bd18b93cd71e2087818ebfdab37a2b`、branch `design/personal-memory-source-v0`（別 worktree）。製品 code・`spec/`・migration は変更していない。build・試験・計測・配備先への接続は行っていない。
- 成果物: [設計](../specs/2026-10-15-search-personal-memory-source-v0-design.md)、[計画](../plans/2026-10-15-search-personal-memory-source-v0.md)、本書。
- 監査 C（KP Search の適合）の file:line を同じ commit で読み直し、設計 §2 にまとめた。新しく見つけたもの: Remote の本文 wire に本文が無い（`crates/search-source-http/src/protocol.rs:156-167`）、Remote の本番 transport が `100.64.0.0/10` を拒む（`transport.rs:408-419`）、`PgVectorIndex` が公開済み stage を RAM から外さない（`crates/search-runtime/src/vector_store.rs:123-183`）、durable read が pointer の移動ごとに世代全体を再検証して RAM に載せる（`durable_read.rs:1-16`）、公開 `SearchItemDto` に digest と世代が無い（`crates/search-api-http/src/dto.rs:364-378`）。
- 進行中の Draft PR #85・#86・#99（手元の ref、fetch なし）を読み、設計 §2.1 に関係をまとめた。#85 の bigram analyzer（所有者が当面 Lindera より選んだ）と #99 の世代の部品化（所有者が承認したのは 2026-10-07 の設計で、PR は作業中の Draft）の上に乗る形に、日本語の字句（設計 §3.4）と取り込みの頻度（§3.3）を改めた。migration の番号は #85・#99 が 0010〜0013 を使うので、各 PR の時点で決める。
- 独立 review（read-only、76 件の file:line を開いて確認、74 件は主張どおり）の指摘を反映した: 進行中の PR との重なり（高）、`SourceKind` の分岐数と compile で漏れが分からない箇所（中）、KP-3 の migration が追加だけでは済まないこと（中）、Memory の字句 schema を KP-4 へ移したこと（中）、試験名の食い違い（中）、秘密の読み方・error code の書き方・G15 の確定・`TraversalBudget` の欄・P2-02 の基準・DB 配置の判断・G6 の比較条件・qualification workflow の担当・状態の語（低）。
- Source protocol: AP 側の担当（ap-core）と `personal-memory-source/1` で合意した。正本は agent-platform `design/personal-memory-p0` の `docs/design/personal-memory/03-core-schema-api.md` §5.4・§5.5、commit `302fdffa402a4702f280247e6ab14290741eeeba`（file の SHA-256 `36fa10d0447cc073f599dd32b3cedea390e2444405b1006c49db9b89def03f2f`）。KP 側の要約は設計 §4。KP の変更要求 6 点（走査の始めに固定した `snapshot_token`、`disabled`・`forgotten` の同じ扱い、`condition` を送らない、`summary` は版の一部、KP の既存 Problem の綴り、保存時の照合の `limit` 20）はすべて取り入れられた。KP→AP の許可は scope の claim でなく AP の `source_grants` で決め、`items:batchGet` は `{ids}` だけ（`changed` なし）とすることに KP が同意した。所有者の承認はまだ。
- 本人の space の KP への登録は、host の上で owner が `search_server inventory publish` で行う（AP 側の手順書 AP-7・O-3 と KP-6 の手順書）。toolbox は KP を触らない。
- 未解決の判断: 設計 §6 と、依頼者への質問（PR の説明に載せる）。
- 次の exact action: 親が push と Draft PR を作る。所有者が設計を承認したら KP-1 と KP-3 を別 worktree で始める。KP-2 は #85 の merge 後、KP-4 は #99 の merge 後に始める。

## 2026-10-15 — 独立レビュー（p0-review）の対応（KP-0 の改訂）

- 対象: p0-review（VERDICT changes-needed、HIGH 4・MEDIUM 13・LOW 19。KP は基点 `7ba63b15` を読んだ）のうち KP の 3 文書に掛かるもの。扱いの一覧は設計 §8。製品 code・`spec/`・migration は変更していない。build・試験・計測・配備先への接続は行っていない。
- 終えたこと:
  - H3: #99 の先端 `b7b8cd89` で、Unit 索引が本文を stored で持ち（`crates/search-tantivy/src/body.rs:271-279`）、差分の世代が前の segment file を hard link し（`:402`）、消した Unit を削除の印で隠す（`:418-424`）ことを読んで確かめた。Tantivy の統合が削除の印の文書を写さないことは vendored の source（`third_party/search/tantivy-0.26.2/src/indexer/merger.rs:258, 388-392, 439-440, 495-515`）で確かめた。設計 §3.3 に「忘却の物理的な消去」（強制の書き直し、時限の回収、行の消去、含まないもの、期限 T = 24 時間、実装までは `purged` を送らない）を書き、計画 KP-4 に範囲と試験を足した。文言は AP の fix-core と合わせた。
  - M5: 設計 §3.5・§3.7 に lane の規則と `lanes` を足した（今の Vector の lane は同じ世代だけを使う、main の `crates/search-application/src/vector.rs:415-422`）。
  - M10・M13・LOW 9・12・17: 設計 §2.1・§3.6・§3.7・§3.8・§4・§6、計画の冒頭・§1・§4。LOW 1 は却下（`discover_route.rs:76` は正しい）。
  - lead decision 2026-10-15（KP-2 の基準の固定、v0 の 1 つの database、Memory に GPU を使わない、pin の意味を変えない、P0〜P6 と予算を外したときだけの最適化）を設計の冒頭と各節、計画に書いた。
  - owner decisions 2026-10-15（第 2 回）を反映した: 配備はすべてこの Mac（launchd、専用の利用者、macOS の PostgreSQL、CPU の e5-small、load 20 を守る上限の案、眠ったときの扱い、KP の data を backup から外す推奨。設計 §3.9）。本番の予算はこの Mac の軽い qualification（KP-7）、規模は RTX 5090 の重い計測（新しい KP-7R、ssh の承認後、機材つき）で、数値を分ける（設計 §3.10）。#85・#99 は 2026-10-29 を判断日として待ち、それまでは依らない PR を先に進める。#86・#99 の branch には触らない。KP-6 は「この Mac への配備」に変わり、ssh を待たない。
- 実行した確認（読み取りだけ）: `gh pr view 85|86|99 -R AIrisu-072/knowledge-platform --json state,isDraft,mergeable,baseRefName,updatedAt,title`（2026-10-15）。結果: #85 OPEN・Draft・`CONFLICTING`・base main・更新 2026-10-07。#86 OPEN・Draft・`CONFLICTING`・base main・更新 2026-10-15。#99 OPEN・Draft・`MERGEABLE`・base は #85 の branch・更新 2026-10-15・題「作業中: 検索世代を不変の部品と一覧で持つ（段1：Unit一覧）」。#99 で所有者が承認したのは設計（2026-10-07）で、PR ではない。手元の ref で #99 の branch（`b7b8cd89`）と #86 の branch（`5a87a9af`）の file を読んだ（fetch はしていない）。
- branch: `design/personal-memory-source-v0`（push は親が行う）。
- Source protocol: `personal-memory-source/2` に上げた。AP 側の正本は agent-platform `design/personal-memory-p0` の 03 §5.4・§5.5、commit `ee5dea94e507f4cbbb9edf1539210ff50d5b2438`、契約の範囲（`<!-- personal-memory-source/2 begin -->` と `end` の印の間）の SHA-256 `2415f631fc87c0686d032b786d65f696ed5c0a7649fb4be371e4c40896e31b31`（設計 §4。その commit の file から計算し直して一致を確かめた。その後の AP の commit `94fc0d81ab58ed8ddb758454c5a34bfa1e1dc79b` と `183aaafc91bef622f4a74c88c3f991305a8d0eed` でも同じ値。固定値が同じなら、引く AP の commit は最初の記録のままでよい、p0-review r3 LOW 2）。fix-core と fix-kp の合意（担当の間の合意）で、所有者の承認はまだ。
- 問い合わせ API の固定値（p0-review r2 LOW 6）: 設計 §3.7 の印 `<!-- personal-memory-source/2 query-api begin -->`・`<!-- personal-memory-source/2 query-api end -->` の間の byte 列の SHA-256 は `c05acd4a78ad422459c268310ab2d059ec6b2f6f4d5115fff0464c0fa3c23c04`（command は設計 §4）。r2 の扱いは設計 §8 の r2 の行（LOW 6・12 を直した）。
- Design Freeze からの差分の提案と承認の状態: KP-0 は設計の承認前なので Design Freeze はまだ無い。`/1` からの差分（purge の意味と期限、`lanes`、K8 の digest）は設計 §4 の版の履歴にあり、所有者の承認待ち。
- 未解決の判断: RTX 5090 の機械への ssh の承認の時期（KP-7R だけが待つが、AP の 02 が KP-7R を release の条件に入れたので v9.0.0 の前に要る）。2026-10-29 に #85・#99 が動いていなければ owner が決め直す。この Mac での配備の細部（PostgreSQL 18.6 を macOS で使えるか、`peer` 認証、plist の権限、APFS の local snapshot への除外）は KP-6 で確かめる（設計 §6）。
- 次の exact action: 親が push と Draft PR を作る。所有者が設計を承認したら、KP-1 と KP-3 を別 worktree で始める（#85・#99 を待たない）。KP-1 の merge 後に KP-6 の手順書と macOS の release binary の CI の job を始める。KP-2 は #85 の merge 後に、`gate.md` を最初の commit にして始める。KP-4 は #99 の merge 後（2026-10-29 に owner が別の道を決めたらそれに従う）に始める。
