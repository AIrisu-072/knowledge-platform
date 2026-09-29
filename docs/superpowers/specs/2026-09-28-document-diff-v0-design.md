# Document Diff v0 — 設計案

- 状態: **PROPOSED / WRITTEN SPEC REVIEW PENDING**。会話上の設計合意を記録したもので、書面設計・実装計画・実装の承認ではない。
- 起案: 2026-09-28 JST、文書化: 2026-09-29 JST
- Capability: `Document Diff v0`
- 基準: `main@6ea29e1ceea82bb0e20b195890b7c0e7efc85f68`
- 区分: Product Capability / Architectural / 設計のみ

## 1. 目的と成功条件

同じDocumentの異なる2つのDocumentVersionについて、追加・削除・修正・移動・順序変更を、形式固有の意味と構造に沿って比較する。金融機関の改訂確認に使えるよう、各判断を旧版・新版のauthoritative原本の位置へ戻せる構造化結果を返す。新旧対照表はその結果から生成する表示用投影とする。

成功条件は、比較できた範囲と比較できなかった範囲を区別し、未比較範囲を残したまま「変更なし」や「確認完了」と表示しないことである。判断の正しさをLLMの推測に依存させない。

今回は設計文書のみを作成する。製品コード、migration、production dependency、HTTP/CLI/GUI、実装PRは作成しない。書面設計承認後に別のProduction Implementation Planを作成し、その計画の承認を実装開始条件とする。

## 2. 正本と維持する境界

本書は次の現在の契約に従う。`spec/`が規範の正本であり、本書だけで既存の規範を暗黙に変更しない。

- `spec/architecture/architecture-contract-v0.md`
- `spec/data/logical-data-model-v0.md`
- `spec/data/transaction-consistency-requirements-v0.md`
- `spec/operations/observability-audit-requirements-v0.md`
- 承認済みのAuthoritative Core、Document Semantic Inspection（DSI）、Document Versioning、Document Publication End（T10）、Document Management Basicsの設計と現行実装
- `spec/selection/library-tool-selection-v0.md` のDSI・Office・PDF選定記録

Document、DocumentVersion、ContentItem、ContentRepresentation、immutable FileObjectとVersion lifecycleはDocument Platformが正本として保持する。Searchは別の再生成可能な投影であり、Search ExtractionやRAG用chunkをDiffの比較元にしない。DSIは単一FileObjectの意味検査と派生証拠を担い、Diffは2つのVersionの比較を担う。DSIの永続レコードへ詳細Diffや部分成功を押し込まない。形式固有のparse modelを共通テキスト列へ不可逆に潰したり、永続的な共通cross-format content IRとして保存したりしない。

Version identityは、正規化されたtitleと順序付きauthoritative ContentItem manifestで決まる。各manifest entryには `logical_path`、`ordinal`、format/profile、DSI semantic fingerprintが含まれる。raw SHA-256はimmutable原本との結合・整合性の証拠であって意味内容の同一性ではない。renditionは比較の正本にしない。ContentItem IDはVersion内で安定するが、Versionをまたぐ意味上の同一性の証明には使わない。

## 3. 対象と保証

### 3.1 対象

- 方向を持つ `base_document_version_id` と `target_document_version_id`。両者は異なるIDであり、同じDocumentに属する。直接の親子関係は要求しない。
- `PUBLISHED` 同士、`PUBLISHED` と `WORKING`、権限を満たす `WITHDRAWN` やT10後の履歴比較。
- 現行DSIのproduction対象であるDOCX、XLSX、XLSM、PPTX、native-text PDF、TXT、CSV、HTMLの同形式比較。各形式のDiff専用比較器は形式ごとの資格試験に合格した範囲だけを「比較済み」とする。
- 版のtitle・authoritative manifest・形式固有の意味内容、および保存された版固有の付随情報。コメント・編集由来・改訂理由などは内容の同一性判定と分ける。
- 確定した変更、未比較範囲、原本位置、比較根拠を持つcanonical `DiffResult`。新旧対照表は別のPresentation Projection。

### 3.2 保証しないこと

異なるDocument同士の比較、異形式間の詳細な内容等価性、スキャンPDFのOCR、PDFからOfficeの編集構造の復元、JavaScript・VBA・マクロの実行、LLMによる正しさの決定、検索indexからの再構成はv0の保証外とする。純粋な装飾差をVersion内容の変更とは呼ばない。表示可能な装飾差を将来補助情報として示す場合も、意味内容の変更判定とは分ける。

Document共通metadataは現在値として更新可能であり、各Versionの歴史的snapshotとは限らない。保存されていない過去の値を推測して「版間metadata差分」を作らない。版固有の記録や確実にVersionへ結び付く履歴だけを付随差分として扱い、共通metadataの現在値・操作履歴は別の参照として明示する。

## 4. 構成と採用案

比較した構成は次の3案である。

| 案 | 利点 | 問題 | 判断 |
|---|---|---|---|
| 要求のたびに原本を再解析し、結果を保存しない | 保存モデルが単純 | 大きな版の繰り返し比較が高コスト | 不採用 |
| 形式ごとの詳細parse evidenceを永続化する | 再比較が速い | 派生parse modelの更新・互換性・機密保護の負担が大きい | 不採用 |
| 原本を必要時に再解析し、snapshot単位のDiffResultだけを派生cacheに置く | 原本が正本のまま、反復要求を短縮できる | cache key・権限再確認が必要 | **採用** |

```text
呼出元
  -> Application DiffService: 検証済みactor、版のsnapshot、DSI証拠、現在認可
  -> 監査付きauthoritative FileObject参照
  -> 隔離された形式固有の比較器: 一時的なparse modelとChangeSet
  -> Application: 結果検証、WORKING鮮度・現在認可・開示監査
  -> canonical DiffResult / 新旧対照表Projection
```

Applicationは入出力・認可・snapshot・監査・cacheを組み立て、Repositoryは整合した読取と派生cacheを提供し、形式固有の比較器は原本バイトと比較profileだけを扱う。比較器へPrincipal、FileId、DocumentId、Storage key、DB・Storage credentialsを渡さない。DSIの隔離・資源制限を候補として再利用するが、Diffの負荷とAPIは別に資格試験を行う。Domain/ApplicationへPostgreSQL、HTTP、特定parse libraryを露出しない。選定済みDSI libraryは候補であり、Diff用dependencyの無審査なpromotionを認めない。

## 5. 入力、snapshot identity、競合

Diff要求はDocument ID、方向付きのbase/target Version ID、信頼済み `VerifiedActorContext`、比較profileを受ける。呼出元が自己申告するVersion参照区分や権限で開示を決めない。Repositoryは両版が同じDocumentに属すること、現在のlifecycle・current pointer・T10状態を確認して、両版の入力を整合したDB snapshotで取得する。

各 `VersionSnapshot` のdigestは、Version ID、正規化title、順序付きauthoritative itemのID・`logical_path`・`ordinal`、authoritative representation/FileObject ID、format/profile、FileObjectのraw hash・size、保存済みmanifest fingerprint、結果へ含める版固有情報から決定的に作る。利用できるDSI evidenceは、検査日時のような可変値を除いたprofile・fingerprint・結果binding・schema/algorithm versionのdigestを加える。証拠が欠落したitemには明示的な欠落markerを入れ、再生成に成功するまでそのitemを比較済みとしない。比較identityは方向付きの両snapshot digest、比較器・比較profile・資源profileのversion、結果を変えるoptionを含む。Document revisionはWORKING更新を検出する競合tokenとして使うが、それだけをcache keyにしない。

原本取得と解析は長いDB lockの外で行う。結果を返す直前に、WORKING版があればDocument revisionとsnapshot digestを再照合し、両版の現在認可・可視性・actor contextの有効期限を再確認する。変化していれば `StaleComparisonInput` として結果を返さず、新しいsnapshotでの明示的な再試行に委ねる。公開済み内容は不変でも、取下げ・T10・policy変更による可視性は最後に再評価する。cache hitにも同じ最終確認を適用する。検出した原本のraw hash・size不一致は部分結果へ格下げしない。

## 6. canonical DiffResultと変更分類

`DiffResult` は少なくとも、両snapshot参照、比較器/profile version、結果digest、内容判定、比較範囲、確定変更、未比較範囲、版固有の付随変更、根拠参照を保持する。結果digestはcanonical serializationから生成する。比較本文をaudit・telemetryへ複製しない。

内容判定は `Same / Different / Unknown`、比較範囲は `Full / Partial / None` の独立した軸とする。`Same` はtitle・manifest・必要な意味内容が全域で検証され、差分がない場合だけ許す。確定差分があれば未比較範囲を残していても `Different + Partial` とできる。確定差分がなく未比較範囲があれば `Unknown` とする。`Partial` や `None` を人間の確認完了として表示しない。Version identityを構成するformat/profile変更はmanifest差分として確定できるが、それだけで異形式の本文が意味的に異なるとは主張しない。

変更は単一の排他的な長いenumにせず、追加・削除・修正の操作、移動・順序変更・名前変更の配置関係、本文・構造・値・数式・リンク・図表・意味を持つ視覚配置などの形式固有の側面を組み合わせる。これにより「移動したうえで修正」や「表の行順だけ変更」を表せる。`Unchanged` は大量のchange行ではなく、比較済み範囲・一致した部分木として記録する。数式の変更は計算cacheの値の一致によって隠さず、chartの参照元変更と単なる描画差を分ける。意味を持たない書式・theme等は内容差分にしない。

各確定変更は根拠となる旧版・新版の位置、比較理由code、形式固有の詳細を持つ。追加は旧版側、削除は新版側が存在しないことを型として表し、架空のlocatorを作らない。コメント、Track Changesの編集由来、署名・改訂理由などVersion identityから除外された証拠は `ancillary_changes` に置く。metadataだけに差がある場合はその差を表示しつつ、内容判定を偽って `Different` にしない。

## 7. ContentItemと内部要素の対応付け

ContentItemはまず `(logical_path, ordinal)`、次に双方で一意な `logical_path` で対応付ける。なお残るものは、同一形式・互換profileで双方に一意なsemantic fingerprintがある場合に限り、内容一致と配置変更の候補として対応付ける。この場合もVersionをまたぐ同一ContentItem IDを主張しない。対応しないmanifest entryの消失・出現はその位置での構造上の事実として報告する。移動・修正との対応が曖昧なら、意味要素の最終的な `Added / Removed` とは断定せず、候補clusterを未確定にする。

形式内ではシート・表・スライド・ページなどの親単位で分割する。資格試験済みの安定したsource identity、親構造とpath、一意な意味fingerprint、局所的な前後関係、順序の順で確かなanchorを探す。parser生成IDだけで結ばない。一致した部分木は詳細照合を省き、残りは同じ親区画の有限個の候補に限って対応付ける。単純な同じindexの結合や全文の総当たりは行わない。

削除＋追加と移動＋修正の両方が成り立つなど対応が曖昧なときは、推測でいずれかに確定せず `AmbiguousAlignment` の未比較範囲と候補位置を返す。LLMや類似度scoreは説明・候補提示の補助に使えても、v0の確定判定器にはしない。

## 8. 形式固有の比較範囲

形式固有の一時parse modelは原本から要求時に再構築する。DSIの成功済み同一形式・互換profileのfingerprint一致は、その意味範囲を比較済みの同一部分として省略できる。fingerprint不一致で詳細の位置を絞れない場合は、少なくともContentItem単位の確定差分と位置未特定の範囲を示す。異なるprofileのdigestを直接比較しない。

| 形式 | 詳細比較で保持する意味 | 境界 |
|---|---|---|
| DOCX | 段落・見出し・list、表とcell、header/footer、footnote/endnote、link、image、section、確定したtracked-change投影 | コメント・編集由来は付随情報。装飾差だけで内容変更としない |
| XLSX | sheetの存在・順序・非表示状態、cellの値・型・数式、named range、table/merge、chartのsource・data、link、image、外部参照 | 計算cacheは正本でない。行の識別が重複して曖昧なら未確定 |
| XLSM | XLSXの全範囲に加え、VBA project、module、procedure、宣言・参照・設定 | VBAを実行しない。必要構文を解釈できなければ影響moduleを未比較 |
| PPTX | slideの存在・順序、shapeとtextの関係、table、chart、SmartArt、image、link、notes、意味を持つ配置・重なり | theme/font/backgroundだけの装飾は内容判定外 |
| native-text PDF | page順、確定できるtext/read order、image/visual region、link、form値、決定的に読めるtable | Officeの編集構造を保証しない。scan/OCRや曖昧な読取順は未比較 |
| TXT | 決定的decode後のUnicode・改行正規化text、行・span | decodeが曖昧なら原本単位で未比較 |
| CSV | delimiter・quote正規化後の行、列、cell値・構造 | 不整合構造やdelimiter曖昧性は未比較 |
| HTML | visible text、heading/list/table、link、image、意味を持つDOM順 | JavaScriptを実行しない。script依存の意味は未比較 |

異なるauthoritative format間では、v0はtitle・manifest配置・format変更など確定できる構造差分を示し、itemの詳細内容は未比較とする。cross-format fingerprint一致を意味の同一性としない。将来の形式pair別比較には、DSIのcapability evidenceとの整合と別途の資格試験が必要である。

## 9. 根拠・原本位置と新旧対照表

各側の `SourceEvidence` はDocument/Version/ContentItem/authoritative Representation/FileObjectの参照、raw hash、DSI evidence profile、形式固有locator、locatorの粒度、比較器とparse provenanceを持つ。追加・削除の存在しない側は明示的に空にする。根拠位置のない「変更された」という断定は避ける。file-level fingerprintだけで差が確定し詳細の位置が不明なら、ContentItem全体を根拠範囲として示し、詳細位置は未確定と記録する。

locatorは行/span、行/cell、DOMの意味node、段落/表cell、sheet/range/module、slide/object、PDF page/regionなど形式固有とする。厳密な位置を示せなければ確実なpage・slide・sheet・sectionへ広げ、なお確定できなければContentItem全体を未比較範囲にする。非表示sheetやVBAのように通常画面に現れない領域には、その原本の該当領域を開くための種別・navigation hintを付ける。原本は変更せず、将来のviewerがoverlayや隣接表示で未比較範囲を視覚的に示す。原本上に正確なoverlayを置けない場合は、その限界を表示する。

canonical `DiffResult` から、旧版・新版・変更内容・根拠位置・比較状態を持つ新旧対照表を生成する。表は表示順、抜粋量、まとめ方を変えられるProjectionであり、比較の正本やcache identityにしない。Projectionは未比較・対応未確定の範囲を省略せず、人間の原本確認へ導く。GUI自体は本書の対象外。

## 10. 現在認可と監査

認可は既存Document Management Basicsの `read`、`read_history`、`write` を、実際のVersion stateとcurrent pointerに応じて両側へ別々に適用する。現行PUBLISHEDの通常参照には `read`、WORKINGには `read + write`、過去PUBLISHED・WITHDRAWN・T10後の履歴には `read + read_history` を要求し、履歴中のWORKINGにはさらに `write` を要求する。T10後に通常参照から過去版へ自動fallbackしない。認可失敗時は既存参照契約と同様に対象の存在を漏らさない。

比較開始時に認可し、原本byteの取得時にも既存の `document.file.access_granted` 相当の事前監査を通す。結果を返す直前の両側の現在権限・状態・WORKING鮮度の確認とDiff結果の必須Audit作成は、同じ短いDB確定境界で行う。その確定を結果開示の線形化点とし、それ以前に確定した権限剥奪は拒否する。cache hit、再実行、Projection生成でも古い許可を再利用しない。送信中に長いDB lockは保持しない。

DiffResultの開示許可には新たな必須Audit event `document.diff.result_access_granted` を提案する。actor、両Version ID、両snapshot digest、比較器version、result digest、比較状態、cache hit、UTC時刻と相関IDを記録し、本文・抜粋・credentials・Storage locatorは記録しない。event作成が失敗したら結果を返さない。event名は開示許可を表し、transportによる送信完了や利用者の確認完了を意味しない。実装前に規範Audit契約へ追加する必要がある。重要な認可拒否は既存の `authorization.denied` 方針に従う。Telemetryは処理時間、候補数、cache hit、資源上限・部分結果の理由などに限定し、sampling可能な運用観測と必須Auditを混同しない。

## 11. 派生cacheと資源上限

cacheはsnapshot pairと比較profileで識別する再生成可能な派生データであり、Document/Version/FileObject/DSIの正本ではない。actor固有の許可をcache entryに保存しない。cacheへの直接公開APIを設けず、entryの結果digestとsource bindingを検証し、現在認可を通してから返す。cache結果に含む抜粋や形式固有の値は文書内容と同等の機密情報として保護する。確定的な完全結果と、理由・資源profileを固定した確定的な部分結果だけを再利用可能とする。一時障害、認可失敗、古いWORKING結果、整合性違反はcacheしない。evictionや再計算はVersion意味を変えない。

比較器はContentItem、形式固有の親構造、fingerprint・hashによる一致で分割する。各区画の候補数を制限し、全nodeのglobalな二乗照合を許さない。入力byte、item/node数、深さ、候補照合回数、memory、wall time、出力change数・抜粋量に有限のversion付き資源profileを設ける。具体的な数値は実装計画のPoCと負荷fixtureで測り、境界値と1超過を検証してから固定する。速度のために意味情報を不可逆に捨てない。上限に達した区画は理由付きで未比較にし、影響しない区画の確定結果は保持できる。safeな位置を示せなければContentItem全体へ範囲を広げる。取消やtimeoutは無制限の処理継続や「変更なし」に変換しない。

## 12. 失敗の意味

| 条件 | 結果 |
|---|---|
| 未対応の意味構造、曖昧な対応、局所的parse失敗、資源上限 | 確定済み差分と理由付き未比較範囲を併記。範囲を絞れなければitem全体。`Same`は禁止 |
| DSI証拠の欠落 | 原本bindingを確認して再生成を試みる。得られなければ影響itemを未比較。一時失敗結果はcacheしない |
| 原本の構造破損 | 影響範囲を安全に切り分けられれば未比較。切り分けられなければitem全体を未比較。破損を意味変更として断定しない |
| 比較中のWORKING更新 | `StaleComparisonInput`。古いDiffResultを返さない |
| 認可失敗・剥奪、必須Audit作成失敗 | 結果を返さず、既存の開示拒否・失敗契約に従う |
| FileObject raw hash・size不一致、cache/source binding不整合 | authoritative integrity failure。結果を返さず、部分成功に格下げしない |

局所的な未比較理由は `UnsupportedSemanticConstruct / CorruptedSource / MissingInspectionEvidence / AmbiguousAlignment / ResourceLimit` のように型で区別し、古いWORKING入力・認可失敗・authoritative integrity failure・監査失敗は結果を返さない別のerror classとする。Diffの `Partial` はDSIの「成功した検査結果は完全」という規則を変更しない。Diffの局所問題をDSIの成功レコードとして保存しない。検出された確定変更があっても未比較範囲が残る場合、結果は `Different + Partial` であり、変更箇所の列挙が網羅的だとは主張しない。

## 13. 評価と受入

各production対象形式に、旧版・新版・期待する内容判定・変更分類・旧新locator・比較範囲を固定した独立fixtureを用意する。最低限、完全同一、単独修正、追加/削除、一意な移動・順序変更、移動と修正の複合、曖昧な対応、見た目が似ても意味が異なる例、バイト列が異なっても意味が同じ例、metadataだけの差、未対応/破損、原本位置が特定できない例を含む。XLSXの値と数式・chart source、XLSMのVBA、PPTXの配置、PDFの読取順と視覚差は個別に評価する。異形式pairは未比較が明示され、完全同一と誤判定しないことを確認する。

評価指標はchange detection precision/recall、alignment accuracy、false unchanged rate、false change rate、locator accuracy、未比較範囲の正確さ、処理時間・peak memory・候補照合回数とする。v0の必須fixtureでは、意味差を `Same` と返す件数、未比較範囲の隠蔽、無権限開示、曖昧な対応の確定移動、誤った原本位置を**0件**とする。意味等価・package byte差のfixtureを偽の内容変更にしない。各形式の資格試験を通らない比較器はその形式を比較済みと宣言しない。広い実文書集合のprecision/recall等は測定値と誤り分類を報告し、根拠のない一律合格率を置かない。

巨大文書では一致部分木の省略と候補数上限を計測し、上限ちょうどと1超過、取消を確認する。WORKING更新、権限剥奪、T10、cache replay、Audit失敗、source/cache改ざん、DSI証拠欠落を含む実DB・storage境界の試験を実装計画で定める。human review用のProjectionでも未比較・未確定の表示が失われないことを受入条件とする。

## 14. 次工程の境界

本書の承認前に規範spec、製品コード、migration、dependency、実装PRへ進まない。承認後のProduction Implementation Planでは、Diff専用parser/comparatorのlibrary資格試験、資源profileの数値、cache保存・eviction、監査schemaと規範反映、形式別fixture・評価順序、隔離workerの実行方法を具体化する。Search、GUI、HTTP/CLI、異形式詳細比較、Authority Migration、OCR、LLMによる改訂説明は別工程として扱う。
