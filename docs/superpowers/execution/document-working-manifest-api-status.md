# WORKING編集 D1 backend/APIの状況

## 2026-10-05 06:28 UTC — 完成sourceからD1を分離

- 元完成commit `f639fbf0a55412887bacb9f70d44ed69a2bbefc1` / tree `0bf0b1b3c8e6868282c4eba26398f0be3213e8eb` を保持する。D1 branch `feat/document-working-manifest-api-20261005`、作業基点main `5d262557`
- 初期分離時のD1製品/契約29pathは元commitとbyte一致。分割独立レビューで下記の既存T10回帰を発見したため、2pathだけ限定補正する。共通承認追補を保持し、計画と本状況は2PRの実行段階に正確化する。元のGUI/固定binary transport/runtime20pathはD2へ留保
- D1は初回null-base更新、同内容許容、以前公開current=nullの除外、MIME保持/任意trusted旧DSI確認、nullable ledger、state capability、正確manifest readとAPI/SDKを含む。旧原本の再検査を修復前提にせず、新候補検査を必須にする。APIの現在認可/OCC/DSIを正本とし、新Human制限やmigration/依存を加えない
- 安全な複数原本GUIはまだこのD1に含まれない。既存の単原本新版フォームをD1で拡張したとは主張しない。D2で全manifest共通編集へ置き換える

## 検証と制約

元完成treeはGUI313・pureRust159・DB31 compile-only・API17/client11・4crate Clippy/fmt/architecture・独立source review GOを確認済みだが、このD1へ合格を移し替えない。これからD1単独の既存GUI/型/build/API/client、純粋backend、compile-only、独立境界reviewを行う。

元作業の検証逸脱も保持する。広いrepository --lib選択で既存Docker DB試験2件（publish_read_helpers_round_trip_operation_and_working_candidate、publish_candidate_lookup_preserves_missing_and_integrity_distinctions）が不存在docker socketへの接続で即失敗し、DB起動せず再試行しなかった。別のapi:contract実行はRedocly telemetry無効化の継承漏れで拒否され、最終成功は公式REDOCLY_TELEMETRY=offで取得した。分割後は既知purecase明示選択、DBcompile-only、全node検証で公式telemetry offを徹底する。

macOS golden比較は未実行・未更新。D1はGUI source/baselineを変えず、D2は影響候補4枚/他3枚不変未証明の資格境界を保持する。全visual qualification済みとは主張せず、画像なしUbuntu実操作受入と独立source/DOMレビューで今回PoCを判断する。

次のexact action: D1単独検証・独立review・localcommit後、親が通知する最新合格mainを履歴保持で統合する。D2をstackし、分割和の製品sourceを元f639と照合する。GitHub公開/hosted資格/最終mergeは親、実サーバー導入は所有者の手動操作。

## 2026-10-05 06:40 UTC — D1単独検証と独立レビュー補正

D1単独の旧GUI279件/22 suites、型・build・MCP/seed/runtime型、API17、client6、lint、診断26、collection14journey/2persistenceが成功した。binary transportの新しい5試験はD2であり、D1へ混ぜない。Rustの補正前実行は159 uniqueケース成功。crate+test target+case名で重複0を確認した（24+90+38と別libの2+2+2+1）。DBは3targetをcompile-onlyし、source宣言testは10+2+19=31、実行は0。

分割独立レビューは一度NO-GOを出した。現公開が分類待ちだと共通current旗がfalseになり、既存T10 endPublicationまでdisabledになる回帰である。T10はStorage/DSI分類を要件としないため、現公開存在と編集適格性を分離した。create/edit/rebaseは分類済みcurrentを必要とし、endPublicationは従来どおりcurrent PUBLISHEDだけを必要とする。withdraw、publish、予約、取消、downloadの条件はその共通旗に依存せず変更しない。

修正は `crates/document-repository-postgres/src/action_capability.rs` と `crates/document-api-http/tests/read_http.rs` の2pathに限定。実production predicateのRED（Disabled/NotCurrentが返る）とGREEN2件を取得し、legacy currentのcapability行列と認可付きT10実mutation成功を確認するDBcaseを追加した。DBcaseはcompile-onlyとし実行しない。元f639は改変せず、分割和の差分はこの既存意味を戻す補正として明示する。修正後のfresh検証・独立再レビュー・最新main統合はこの後に記録する。

06:43 UTC: T10補正のlocked/offline再実行はpure2件とread_http compile-onlyが成功し、Cargo.lockの一時的な順序変更を元blobへ完全復元した。新read_httpのsource宣言は20件（D1のDB対象は計3target/32宣言case、実行0）。独立再レビューでsource/契約/分割境界GO、残件なし。27backend pathは元f639とbyte一致、残る2pathだけ承認したT10補正で、D2留保20pathとlockはbaseと一致する。ここでsourceをlocalcommitし、合格main c4388433へ履歴保持mergeして、その新しい組合せで全最終検証を取り直す。

## 2026-10-05 06:50 UTC — 合格mainの予約取消を保持したD1統合候補

source `9212c7b01f0e3e269dbedc2964342e7b4d489561` と合格main `c4388433e97e239057921f2412d1ac3e00900de4` を両parentとして保持する。read_httpとAPI契約の末尾追加test、Active先頭だけが競合し、両者を残して解消した。OpenAPIからSDKを再生成し全generated fileのbyte再現性を確認した。現在予約ID/read・取消GUI・元の固定要求回復を落とさず、D2のGUIはまだ入れない。

新しい組合せでfresh検証を実施した:

- GUI302件/23 suites、client6、API18、型/schema freshness/build/MCP/seed/runtime型、lint成功。既存Webpack3 advisoryとlint warning1を保持
- 164 unique pure Rustケース成功、package/target/caseで重複0。DBは4targetをcompile-onlyし、source宣言testは10+2+3+21=36、実行0。4crate全target strict Clippy、fmt、architecture成功
- 有限診断/記録配線28件、既存journey15/7files・persistence3/3filesのcollection-only成功
- 新独立共存reviewはGO、blockingなし。main専用17path/D1専用25pathは各parentのbytesを保持し、重複8pathを個別確認。document-web既存118pathはmainと一致。Review後はこの検証記録だけを追記した

D1はlocal source/契約資格のみ。新exact-head CI・実DB/browser/restart/cleanupは公開後に親が確認する。元f639、D1source、mainの履歴は保持し、次は日本語Draft packetを親へ渡してD2をstackする。公開/main mergeは親、実サーバー導入は所有者が手動実施する。
