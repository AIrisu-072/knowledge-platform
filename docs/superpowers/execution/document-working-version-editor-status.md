# WORKING複数原本編集の状況

## 2026-10-05 08:04 UTC — D2最終source/DOM検証完了、stacked公開資格はこれから

- branch `feat/document-working-manifest-gui-20261005`。保存済みD2 `b6ec14b471282e4fa1b50b6ebf30004133e483b5` と、metadata/予約取消main `b4663e41` を保持するD1 `75b9df5111ca7010f41696d9ae389ae197b8ce7f` を両parentとして統合する。元完成 `f639fbf0` とD1各checkpointも保持する
- D2差分はD1比25path。backend・OpenAPI/schema・生成SDK・lock/workflowはD1と同じbytes。手書きclientのbinary transportとその試験はD2範囲であり、固定multipart Blobを担う。専用metadata/取消などmain-only 13pathを完全保持した
- 元f639からの限定補正は、公開mainとの共存、同名原本をpath/ordinalで区別するラベル、server available/disabled理由の表示、編集中の基準固定である。D1のT10補正はD1記録へ分離し、業務条件を追加しない
- 編集sessionのmanifest・mode・元対象を固定し、背景revision/source/current/display/purposeの変化や読取失敗では文書名/File選択を保持して保存を止める。明示最新確認が同じsessionで整合したsnapshotを得た場合だけ採用する。採用要求は不整合・別source読取中も一度で消費し、後続の背景応答に入力破棄の権限を残さない
- 遅い明示読取の完了はcontext/session世代/attempt/操作オブジェクト同一性で限定し、新しいpending/unknown操作を消さない。成功statusの正確なdocument queryだけで整合を判定し、エラー時の残留cacheで権限を復活させない。新しい自動merge・永続draftはない

### 最終の新鮮な検証

- 全GUI400件/28 suites成功。WORKING52件と独立再reviewの144件/8 suitesはその限定再実行であり、unique成功数へ足さない。型/schema freshness・production build・runtime型が成功
- API18/client11・有限診断/記録/用途35・安全なruntime純粋123・MCP build・lintが成功。有限35は123と加算してunique件数を主張しない。既存Webpack advisory3件/API lint warning1件を保持
- 既存runnerへのcollection-onlyはjourney18件/9filesとpersistence5件/5files。D2でCargo・DB/socket/listener/browser・画像は実行していない。D1のRust164 unique/DB4target36宣言case compile-onlyをD2で再実行したとは扱わない
- 独立source/DOMの最終判定はGO、残るImportant所見なし。未送信入力消失、旧refreshによる新rebase unknown消失、不整合/新query保留後の遅延採用をすべて実DOM RED→GREENで閉じた。途中の398件中397成功/1失敗を合格へ付け替えず、最後の400件全成功と区別する
- 最終review対象23pathのhashは `46feedc6578fa297feeb4035b0921f2fb20a94a09b420bc7aa3a82f20773aab1`。全manifest準備/同一要求再送helper・binary transportと既存取消/lifecycleの意味は元sourceを保持する。review後の変更はこの実行記録だけ

### 残る資格と次の操作

このexact headの全適用CI、Ubuntu/PostgreSQL18.6/固定2合成profile/Chromiumの実操作・HTTP再起動・owned cleanupは公開後に親が確認する。新runtime specのscreenshot/trace/videoは全offで、既存有限診断だけを使用し、公開artifact0を確認する。

macOS golden比較は未実行・未更新。影響候補mock2/3/4/7の4枚、他3枚のpixel不変も未証明であり、標準Ubuntu CIの既存darwin限定画像assertは実行されない。golden更新・skip/期待緩和・新画像基盤/公開はない。今回は画像なしUbuntu実操作と独立source/DOMレビューでPoCを判断し、全visual qualification済みとは主張しない。

次のexact action: 両履歴を保持したlocal merge commitと日本語stacked packetを作り、D1をbaseとするDraft公開・exact-head CI/実受入を親へ渡す。main mergeはroot、実サーバー導入は所有者の手動操作。以下は各時点の履歴であり、その時点の未完/失敗を最終結果で消さない。

---

## 2026-10-05 06:54 UTC — D2 GUI/runtimeをD1＋合格mainへstack

- D2 branch `feat/document-working-manifest-gui-20261005`、基点はD1＋合格mainの `ff0aae673cb065ae056ae48499537b0fc213b4fc` / tree `7f0b70fabb256673602812fe6183c8df07fef4d9`。D1状況は[こちら](document-working-manifest-api-status.md)
- 元完成 `f639fbf0` のbranchを改変せず保持し、そのGUI/固定binary transport/runtime/操作文書21pathの差分を3-wayで適用する。whole-file復元で合格mainの予約取消GUI/readを落とさない
- D1のbackend/SDK・T10分類待ち現公開の終了capability補正は変更しない。D2は全manifest編集と固定要求回復、新2+1実runtimeケースを担い、親の2PR分割方針を維持する
- 現段階は統合中。元f639のGUI313やD1のbackend資格をこの新headへ付け替えない。D2単独の全GUI/型/build/API/診断/collectionと独立共存レビューをこれから行う
- 元f639で見逃されていたT10 shared-current旗の回帰は分割独立レビューで判明し、D1で2pathの限定RED→GREEN補正済み。分割和はこの補正と公開main保持の差分を明示して照合する
- 画像なしUbuntu実操作受入を優先し、新画像基盤・golden更新・skip/期待緩和は作らない。macOS比較は未実行、影響候補mock2/3/4/7の4枚、他3枚のpixel不変も未証明。全visual qualification済みとは主張しない

次のexact action: D2差分を適用し予約取消との共存を検証・独立reviewしてcommit/日本語packetへ進む。D1/最新公開mainを保持し、公開後exact-head CI/実runtimeは親、実サーバー導入は所有者が手動実施する。以下は元完成sliceの時点の履歴。

---

## 2026-10-05 06:21 UTC — 単独sliceのsource・限定ローカル検証完了、公開mainとの共存統合待ち

- 基点main `5d262557`の独立sliceが完成。現在branch `feat/document-working-version-editor-20261005`。既存metadata/予約取消の未公開変更は流用していない
- 初回null/currentの同ID更新とnullable replay、正確manifest read、全bytes共通フォーム、選択原本だけの差替え/変換物除外、明示rebaseを実装。公開pointerは編集で変更せず既存publishで切替える
- 初回修復の追加境界を確認: 旧原本再検査不要、新検査必須、matching keyのmediaType固定、trusted旧DSIがある場合だけformat/profile互換、旧hash/size不整合はintegrity停止。同内容初回を新たに拒否しない
- 独立source reviewはGO、Critical/Important/Minor残件0。authoring内の公開only作成、成功後再入場、拒否後readonly原本保持、stale manifest再読込、unknown中の公開/予約interlockを追加RED→GREENで修正。最後の同mediaType/異DSI format testも独立再読で確認した

### 新鮮な検証結果

- 全GUI313件/24 suites、client11件、型・schema freshness・production build、MCP/seed build成功。Webpackの既存performance advisory3件を保持
- API contract17件とlint成功。API検証は `REDOCLY_TELEMETRY=off` を全processへ継承した再実行だけを最終証拠とする。lintの既存example-domain warningを保持
- Rust純粋159件成功: Domain24、Applicationの明示22targets/90、HTTP明示8targets/38、manifest application2/repository2、ledger2、nullable DTO1
- DB31件はcompile-only成功: versioning transaction10、vertical slice2、read HTTP19。4crateの全target strict Clippy、全体fmt、architecture、diff検査成功
- runtime型、既存runnerへのjourney16件/7files・persistence3件/3files collection-only成功。有限診断/recording配線28件成功。新runtime2+1はtop-level screenshot/trace/video全off
- 独立reviewerもGUI52、client11、API17、診断28を実行成功。実DB/browser/再起動/cleanupの成功はまだ主張しない。全workspace verify/nextestはローカル制限のため実行していない

### Visualと公開の資格

既存macOS専用 `Mock 1 through Mock 7 preserve the approved screens and core states` のsnapshotは未実行・未更新。直接影響の候補はmock2/3/4/7の4枚であり、他3枚のpixel不変も未証明。既存の1test内で7枚を順番にassertする構成で、4枚専用の実行経路はない。通常mock Playwrightの失敗画像/traceはgitignored test-resultsへ保存され、時間ベースTTLはない。現在標準CIのmock E2EはUbuntuであり、既存darwin限定snapshot assertionはgateされない。画像を生成・公開せず、golden更新・skip/期待緩和も加えていない。親の今回方針は既存の画像なしUbuntu実操作受入と独立source/DOMレビューでPoCを判断し、新画像検証基盤を作らない。これを全visual qualification済みとは扱わず、将来の画像更新は別の限定計画とする。

次のexact action: この単独sourceをcommitして保持した後、親が示す最新合格mainを両履歴保持で統合する。公開済み予約取消とmetadataを落とさず、共存後の検証/独立reviewを取り直して日本語packetを渡す。そのexact headの全CI/hosted実受入とmergeは親、実サーバー導入は所有者の手動操作である。以下の初期記録と途中の検証逸脱は履歴として保持する。

---

## 2026-10-05 05:47 UTC — 承認済み追補を記録、実装中

- 状態: ACTIVE。公開main `5d262557f1d59ab10db2eeb5d808c19382b2414a`から専用branch `feat/document-working-version-editor-20261005`を作成した
- [UI/API追補](../specs/2026-10-05-document-working-version-editor-amendment.md)へ所有者の原文承認と公開維持・対象変換物除外・初回更新の限定変更を記録。[小計画](../plans/2026-10-05-document-working-version-editor.md)に従いTDDで進める
- backend/APIとGUIを限定分担し、hosted受入の追加は同じsliceで行う。別metadata/予約取消slice、Search/Audit/Toolbox停止作業は変更しない
- 現段階の検証: 専用worktree・公開base一致のみ。実装/純粋試験/compile/独立review/hosted CIは未完
- 次のexact action: backend契約を固定し、初回更新と完全manifest編集のREDを実行する。公開は親担当、main merge前にexact-head CI/実runtimeを確認し、実サーバー導入は所有者が手動実施する

## 検証上の逸脱（隠さず保持）

backend検証で `cargo test --locked --offline -p document-application --test versioning_preflight -p document-repository-postgres --lib -p document-api-http --test edit_manifest_http` のrepository `--lib`を広く選んだ結果、純粋ledger試験に加えて既存内蔵Docker DB試験2件を誤って実行した。`publish::tests::publish_read_helpers_round_trip_operation_and_working_candidate` と `publish::tests::publish_candidate_lookup_preserves_missing_and_integrity_distinctions` は `/var/run/docker.sock` が存在せず `SocketNotFoundError` で即失敗し、DBは起動していない。これは禁止したローカルDB/socket経路の選択ミスであり、成功扱いや未実行扱いにしない。別DB経路での回避・再試行はせず、以後は既知の純粋module/caseだけを明示選択し、DB試験はcompile-onlyとする。親担当へ即報告済み。

API contract検証でも、内部で起動するRedoclyへ `REDOCLY_TELEMETRY=off` を全processで継承させる設定を忘れた実行があった。未承認OTel宛て通信の危険によりtool reviewerが拒否したため、その実行を成功証拠にしない。外部送信の成功は確認していない。既存READMEに記載された公式のtelemetry無効化をexportする安全な検証だけへ修正し、追加通信許可・迂回経路は使わない。

## 2026-10-05 07:03 UTC — D2と予約取消の共存source checkpoint

ff0aae67上のD2は21pathを3-wayで適用し、既存予約取消・lifecycleを保持して5競合を解消した。mainの専用予約取消source、D1backend/SDK、lock・workflowは不変。元f639との差分は予約取消との共存に必要な配列/読取fixture/文書と、親が追加確認した同名原本のアクセシビリティ補正に限定する。

同名originalFilename・異logicalPathの2原本を一意に選ぶDOM反例をRED→GREENにした。ラベルは「差替ファイル: 元名（固定パス: logicalPath、順序: ordinal）」として既存prefixを維持する。選んだitem IDの原本/旧変換物だけを変更し、他方の全FileId/bytes/変換物を保持する。manifestの意味・公開動作は変えない。

fresh結果: GUI337件/25 suites、API18/client11、型/schema/build/MCP/runtime型、有限診断/記録配線31、許可されたruntime純粋119、collection17journey/8files・4persistence/4filesが成功。WebPack既存3 advisoryとlint既存warning1を保持。独立source/DOM reviewはGO、focused GUI79/4 suitesと診断31を実行し、元f639の保持・全取得/対象変換物除外・固定unknown要求・用途/rebase/公開予約interlock・同名ラベル・取消共存を確認した。DB/browser/画像は実行していない。

このsourceをcheckpoint commitで保持する。最新mainはmetadataも含むb4663e41へ進んだため、次にそのmainを保持する新D1へD2をstackし、metadata/取消/複数原本の最終共存を再検証・reviewする。現checkpointを最終hosted資格へ付け替えない。

## 2026-10-05 07:11 UTC — metadata保持とdisabled理由の純粋表示配線

D2 checkpoint `b6ec14b471282e4fa1b50b6ebf30004133e483b5` を保持し、最新D1 `75b9df5111ca7010f41696d9ae389ae197b8ce7f`（main b466のmetadata/取消を保持）とのno-commit mergeを開始した。runtime配列、DocumentDetailPage imports、有限診断、strict配線期待を両機能の和へ解消する。Activeは両履歴を自動保持した。backend・OpenAPI/schema・生成SDK・lockは最新D1をそのまま維持する。D2範囲の手書きbinary transportとその試験は含む。

親の追加確認により、WORKING edit/rebaseのdisabled理由が既存の他panelにも表示されていなかった点を補正する。サーバーが返すavailableは実行可、disabledはそのreasonの既存日本語mapping付き無効control、不在は非表示とし、GUI独自の業務条件を推測しない。現schemaにないhidden enumをbackendへ新設しない。stale、予約、権限、lifecycle、notCurrent等のDOM RED→GREENを追加し、unknownの固定要求回復とlocal interlockは保持する。これは元f639からの限定UI表示補正として記録し、manifest/公開の業務意味を変えない。

新組合せの全GUI/型/build/API/診断/runtime collectionと独立source/DOM reviewは進行中。画像の資格境界は上記から変更しない。


## 2026-10-05 07:39 UTC — 最終確認で背景再取得による未送信入力消失を検出

最新D1との共存検証は一度GUI388件/28 suites・型/schema/build・API18/client11・有限診断35に成功し、source/DOMレビューもGOとなった。その後の親の追加確認で、編集中のmanifest背景再取得によってrevision keyが変わり、未送信の文書名と選択Fileが通知なく失われる反例を独立DOM実行で再現した。保存送信は起きていないが、新しいrevisionに対する保存が再度有効になるImportant所見であり、先のGOをこの所見の解決後資格として扱わない。

全体のwindow-focus refetchは既にoffだが、stale後の再接続refetchは有効である。背景読取失敗や別利用者の公開切替によるsource/purpose変更もフォーム再作成を起こし得る。編集sessionのmanifestとcreate/update modeを固定し、変化・読取失敗では入力を保持して保存を止め、明示的な最新状態確認が成功したときだけ新しい基準を採用する限定補正をRED→GREENで進める。新しい自動merge・永続draft・backend業務条件は追加しない。最終commit/packetはこの補正と再検証・独立再reviewの後に作る。
