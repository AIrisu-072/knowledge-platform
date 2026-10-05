# WORKING複数原本編集 UI/API追補

## 承認と基点

状態: 承認済みの業務条件を実装へ落とす限定追補。公開main `5d262557f1d59ab10db2eeb5d808c19382b2414a` が基点。既存[Versioning設計](2026-09-27-document-versioning-v0-design.md)・[GUI設計](2026-09-30-document-gui-integration-v0-design.md)の承認原本は変更しない。本追補だけが初回WORKING更新・編集manifest read・安全な複数原本フォームを追加する。

所有者の承認原文:

- `Sentinel_99471d25cf608191a921d9474f2e4b76`、2026-10-05 05:02:40 UTC: 「複数原本を最初から必須にしたい。また、公開済み・取下げ済みの版は直接変更しないというのは公開しているものを一旦取り下げて何も公開してない時間を許容するということですか？それであれば別の方法を模索したい。それ以外は承認する。」
- 確認 `Sentinel_561e84a7cf188191812bc3b2c9c4726e`: 「公開版を維持しながら別の下書きを編集し、公開時に切り替える方針でよいですか？」
- `Sentinel_ac0b93a8bd508191bdeb18dfc4c73763`、2026-10-05 05:39:51 UTC: 「公開時に切り替える設定で問題ないです。2件とも保存して問題ないです。」
- 確認 `Sentinel_6d2b4e572fec8191812e04cfbc9b5428`: 「残る1点です。原本を差し替えたら、その原本から作った古い変換ファイルは新しい作業版には引き継がず、原本だけで進めてよいですか？変更していない原本と変換ファイル、旧公開版のファイルは保持します。変換ファイルの自動再生成は今回含めません」
- `Sentinel_a933ae3d5c908191a795905d3b4b2e7e`、2026-10-05 05:41:46 UTC: 「原本だけでいいです。」

## 更新と公開の規則

- 初回の一度も公開されていない#1 WORKINGだけは、current/baseが共にnullでも全原本を新検査して更新できる。同じVersion ID・番号を維持し、初回へ新しい内容同一拒否は加えない。以前公開され、今currentがnullの文書は初回ではない
- 初回修復に旧原本bytesの再取得・再検査を必須としない。新候補の検査は必須とし、同じlogicalPath/ordinalの旧原本mediaTypeを明示的な変更不可属性としてbackendでも保持する。旧DSI記録がraw hash/sizeに一致して信頼できる場合はformat/profile互換も維持し、不一致の記録はintegrityエラーとする。旧DSI証拠が無い場合は、既存mediaTypeを維持した上で新候補の検査結果が成立する修復だけを許す。mediaType一致をDSI形式同等性の証明とは扱わず、text/plain→PDF等の形式変更機能を追加しない
- 現公開があるWORKING更新はbase=currentを必要とする。stale更新は拒否し、明示rebaseはcurrentがある場合だけ。PENDING予約中・公開終了後・PUBLISHED/WITHDRAWNは直接更新しない
- capabilityとmutation条件を一致させ、最終の現在認可・OCC・DSI検査はbackendが行う。GUI固有のHuman条件をbackend認可へ追加しない
- 作業版の作成・更新では現公開を維持する。既存publish transactionがcurrentを旧版から新版へ直接切り替える。確定拒否はrollback。COMMIT_OUTCOME_UNKNOWNでは旧版維持と断定せず、同操作・同payloadで既存ledger回復を使う
- 初回更新結果のbaseVersionIdはnullを許し、ledger復元にも反映する。公開baseとの差分なし拒否は維持する。同内容WORKINGを別操作IDで更新することを新しくno-opへ変更しない

## 正確なmanifestと全bytes再送

編集用readはRead+Writeと既存published/authoring用途の認可を満たす必要がある。同じsnapshotからDocument revision、source Version、全ContentItem/representationのlogicalPath、ordinal、role、FileId、正確なoriginalFilename、取得用IDs、mediaType・sizeを返す。safe displayNameから元名を復元しない。

- WORKINGがある時はその全manifestを読みPUT。WORKINGがなく現公開がある時は現公開の全manifestを読み、差替えを含む初回保存でPOSTし別WORKINGを作成する。無変更cloneは既存semantic guardで拒否される
- logicalPath・ordinalは固定。選んだ原本だけ新FileId・新bytesにする。未変更原本とそのRENDITIONは全保持する。選んだ原本の同contentItemIdに属する旧RENDITIONだけを新WORKINGから明示除外し、対象名・件数を画面で確認する
- 旧公開版のファイルは残す。変換物の自動生成はしない。WORKINGだけの変換物は参照除去後に孤立FileObjectになり得るため永久取得を保証しない
- 保持する全ファイルを既存監査付きdownloadで取得する。一つでも失敗・認可失効・監査結果不明なら送信しない。途中で公開が切り替わってもhistoryへ暗黙fallbackしない
- 結果不明後はoperationId・targetVersionId・expectedRevision・FileIds・partIds・配列順・payload・bytesを固定する。manifest再読込やID再生成で再送内容を変更しない
- 未変更FileIdは既存immutable binding一致の規則で再利用する。representation参照型writeの追加はしない。既存PUTのrepresentation再作成と成功replayを変えない

## 上限と対象外

既存上限はJSONを含む64parts（binary最大63）、file256MiB、multipart全体1GiB（JSON/headers/境界込み）、JSON1MiB、処理120秒。原本と変換物を全て合算し、超過は理由付き停止。shared FileIdは現HTTPで要求全体重複禁止なので、黙ってIDを変えず理由を示して停止する。

初回文書登録自体の複数原本化、原本追加/削除/並替/形式変更、新しいDocument authoritative規則、Search/Audit/Toolbox停止作業は対象外。保証は公開なし期間を作らないことであり、切替直後の古いpublished URLや全通信の無停止ではない。旧公開の新規取得には既存history権限が必要で、既に認可・監査済み取得は旧immutable bytesを継続する。
