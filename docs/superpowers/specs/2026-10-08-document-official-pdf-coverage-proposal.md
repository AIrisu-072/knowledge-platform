# 公式PDFの意味検査：限定拡張の選択肢

Status: APPROVED A / 2026-10-08 13:49:25 UTC「確認待ちの4点は進めてください」で、提示済みの既存エンジン限定拡張Aを承認。実装・資格確認は進行中で、現時点はtest-only。

## 結論

**既存のPDFium＋lopdfを維持し、文字中心の実PDFで必要な描画・構造だけを、意味を落とさず検証できる範囲に広げる案を推奨する。** 演算子を読み飛ばして公開を許可する案、画像化して文字・リンク・構造の検査を代替する案は採らない。PDF標準全体を一度に実装する計画ではない。

## 確定した障害

[PR110](https://github.com/AIrisu-072/knowledge-platform/pull/110)のhead `d217f51d4b01e10f0b29ad0c94a874062167fdaf`、[run37752175251/job113227809431](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37752175251/job/113227809431)で、公式通知001472933.pdfは登録201・公開422 `BUSINESS_RULE_REJECTED`。同じqualified LinuxSandboxRunnerは `unsupported_semantic_construct` を返した。

自分のFileIdについて、WORKING、分類不要、media/hash/size一致、version/authoritative参照各1を実確認した。APIの事前操作不足ではない。DSI保存行はなく、注釈quality gateへ到達した証拠もない。**注釈が原因とは断定しない。** 登録1・公開0であり、smallも大量段階も未資格。

原本を変更せずpypdfで構造を補助観測すると、3候補すべてに現在未対応の描画命令がある。先頭ページではBDCが最初の未対応命令候補となる。これは原本とsourceの静的照合であり、workerが返す固定codeより細かい実行traceを得たという意味ではない。

- 001472933（5頁）：タグ、黒色、ページ大矩形clip、罫線・曲線、線幅/dash等。FreeText注釈1件
- 001472934（2頁）：通常の文字中心の通知。タグ、黒色、矩形clip、宛先の括弧等の線。注釈0件
- 001472935（30頁）：上記に加え赤色、塗りつぶし、多数の表構造。注釈0件
- ExtGStateは主にNormal・不透明。933の後半にはstroke adjustment/smoothness設定もある
- StructTreeRoot配下にもActualTextがある（933:18、934:28、935:1,861件）。BDC辞書だけ見て無害なタグと判断できない。935にはTable/TR/TH/TDも多数ある

原本・出典・取得日・SHA-256は既存検証manifestで保持する。原本の注釈・タグ・描画命令は削除しない。

## 選択肢

### A. 実PDFの限定対応を追加する（推奨）

まず934のような小さい文字中心の公式通知を受入候補にし、次を一組で設計・検証する。小さいファイルへのすり替えではなく、**どの実務PDFを扱え、どれを拒否するかを明示する限定coverage**とする。

1. **有効な描画状態を解釈する。** 既知の不透明ExtGState、基本色、線・曲線・矩形、stroke/fill、CTMと描画順を正規化する。参照先scope・q/Q復元を含め、未知key・未対応blend/soft mask等は拒否する。生の命令列hashにはしない。
2. **clipを無視しない。** 初段は見える文字/画像/線を切らないことを証明できる矩形clipに絞れる。それ以外は拒否し、隠れた文字を読めたことにしない。
3. **タグ・置換文字を検証する。** BMC/BDC/EMCの対応、MCID→構造要素、ActualText/read orderを確認する。独立した抽出との不一致、欠落・循環・曖昧な対応は拒否する。Artifact内の描画も消さない。表セルの意味まで扱う935等は別の受入条件とする。
4. **差分比較も同時に直す。** 新しい描画/構造fieldを比較するか、必ず未比較範囲へ落とす。視覚差の位置を確定できない場合はページ単位のPartial等を維持し、別頁の文字差だけで全体をFullにしない。

注釈・未解決変更・署名の既存公開quality gate、原本hash、必須sandbox、資源上限は維持する。933が構文検査を通っても、別途注釈quality gateで拒否される可能性は残る。3候補すべての公開成功は先に約束しない。

### B. 現coverageを固定し、対応形式だけの容量測定へ分ける

現実の公式PDFは未対応として残し、既存の適合する合成/別形式の容量試験を先行する。製品変更は少ないが、今回の公式PDF実用資格は解決しない。利用者が明示的にこの縮小範囲を選ぶ場合だけ行い、「大量の実務PDFを扱える」とは結論しない。

## 意味判定とcacheの承認境界

凍結契約は、情報の意味を変えないフォント/spacing等をidentityから除く。ページbitmap全体をそのままfingerprintにする方式は装飾・注釈・描画環境差を巻き込むため、今回の推奨には含めない。固定PDFiumのrasterは既存 `pdf_paint_semantics` と同じく**fixtureの表示差を確かめる検証用**に使う。

既に成功するcorpusでfingerprint/evidence不変、意味規則不変の拡張と実証できる場合だけdsi-v0継続を検討する。新fieldを全旧PDFへ足して黙ってdigestを変えない。意味規則を変える必要が分かった時点で停止し、新profile・旧公開版のidentity・移行方法を別途承認対象にする。

DSI cacheはfile＋profile、Diff cacheもsnapshot＋profile等がkeyで、worker build更新だけでは旧結果が消えない。DSI失敗には成功行を保存しないため、負のDSI cache移行は不要。旧成功PDFのprojection bytes、fingerprint、capability/editorial/signature evidenceを保持する。新しい非空の意味だけoptional fieldとして追加し、既存の空/default fieldを無条件追加しない。これを実証できない場合は新profile判断へ戻す。

現在のDiff cacheはprocess-local（64件/128MiB）で、再起動で消える。決定的Partialもcacheするため、今回のcoverageを示す派生comparison cache世代を導入し、旧結果とのkeyを分離する。これは比較algorithmのcoverage拡張であり、単なる表示変更ではない。既存結果/historyや永続DSIは変更せず、purgeや旧Version identityの移行は行わない。新しい比較依頼のみ新cache namespaceを使う。旧新分離と同世代のreplay/cache再利用を試験する。

## 必須受入

- 既存成功corpusの意味fingerprint・editorial/signature evidenceが不変
- resource名/object ID/圧縮/同値の数値表記/default状態/MCID番号だけの変化は意味を変えない
- 線・clip・色・fill規則・重なり順・ActualText/読順/表対応に情報差があれば、差を検出するか明示拒否。無視して「同じ」にしない
- PDFiumで実際の見え方の差、独立抽出で文字・構造の差を確認するfixture。未知状態追加、切れたタグ、OC、透過、壊れた参照はfail closed
- 新field差＋別頁のtext差、新fieldだけの差、旧cacheを誤ってFull/unchangedにしない
- path/paint/structure数・深さ・出力・時間の境界。失敗時は成功DSI行なし
- 最新mainを保持したexact headで、公式原本のDSI→公開/正当拒否→比較→同一DB/storage HTTP再起動を再確認
- smallの対応範囲と正例/負例が実証されるまで1,000→1万→10万へ進めない

## 参照

- 凍結DSI設計 §4.5、§5.5、§9.5、§13.6：`2026-09-20-document-semantic-inspection-v0-design.md`
- worker：`crates/document-semantic-inspection-worker/src/adapters/pdf.rs`（strict paint walk、projection、capability）
- 描画oracle：同 `tests/pdf_paint_semantics.rs`
- Diff：`crates/document-diff-worker/src/adapters/pdf.rs`（既知4field比較）
- cache：`crates/document-application/src/semantic_inspection.rs`、`document_diff/snapshot.rs`
- ActualTextの位置：<https://pdf-issues.pdfa.org/32000-2-2020/clause14.html#1494-replacement-text>

最新main `b1c5c36d2d491cffc6723607622f6cd1f7a9e933` と診断headのPDF worker/runner・versioning preflight sourceは差分0を確認した。今回の提案で製品coverageを変更していない。

## 実装の順序と所有境界

1. 旧成功PDFのbaselineを固定。最初にdefault状態、vector・色、包含clipの実試験をREDにする。
2. 既存paint walkから小さい内部moduleへ有限graphics state/path解釈を分離し、default/数値正規化と未知状態拒否を実装する。透過・optional contentや未対応色空間は許容しない。
3. 有限タグ木とmarked contentを照合し、ActualText・reading sequenceを検証する。対応できない表や構造を明示拒否する。
4. Diffで追加意味fieldを比較、未比較fieldはページ/文書レベルとも未検証へ落とし、派生cache世代を分離する。
5. 旧corpus互換・敵対fixture・資源境界・独立reviewを通し、qualified Linux sandboxで公式原本smallを実行する。原本933/934を保持し、正例/負例を実DSI根拠で分類する。
6. smallと再起動保持が成功した後だけ、計測済み資源から次段階のadmissionを判断する。

変更対象はPDF worker内部module/tests、Diff adapter/cache/tests、専用検証harness/docs。GUI、directory、scheduler、生成API model、原本、既存quality gate、sandbox契約を変更しない。cloudがsource/testを所有し、Macは同一commitのRust baseline/RED/GREENを検証する。実Linux資格はhosted既存runnerで行う。
