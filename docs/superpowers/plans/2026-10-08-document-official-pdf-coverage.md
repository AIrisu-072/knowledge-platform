# 公式PDF限定coverageの実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** 既存PDF検査・比較に文字中心の公式通知を検証可能な範囲で追加し、実smallから容量段階へ安全に進む。

**Architecture:** 既存PDFium/lopdfを使い、opaque graphicsと構造検査をPDF adapter内部moduleへ分離する。旧成功projection/evidenceを維持し、非空の新意味fieldだけ付加する。Diffは新fieldと残余差を取りこぼさず、派生cache世代を分離する。

**Tech Stack:** Rust1.98.1、pdfium-render0.9.4、PDFium151.0.7881.0、lopdf0.45.0、既存Node harness。

**Spec:** `../specs/2026-10-08-document-official-pdf-coverage-proposal.md`（13:49:25 UTC承認のA）。同specの実装順序を検証単位へ展開した追補で、新機能範囲を追加しない。

## Global Constraints

- 旧成功fingerprint・deterministic evidence不変。必要な意味変更が判明したら新profile/移行判断を親へ戻す。
- 原本加工、新engine、OCR、sandbox省略、品質gate低下、未知意味の読飛ばしを行わない。
- decode64MiB、depth64、operations1,000,000、paint/path100,000、既存runner10秒を維持。補助proofにも有限予算を設ける。
- 新cache世代は派生比較だけ。旧原本・DSI・版identity/historyを変更・削除しない。
- Macのnative検証はLinux sandbox資格ではない。exact-head hostedで公式smallを検証する。

## Review Focus

- 色space/default override、文字stroke、vectorで一部文字を隠す反例が偽の同一判定にならない。
- clipのq/Q・CTM・曲線・square capが可視boundsを過少評価しない。
- ActualText/ParentTree/MCID/RoleMapが文字と異なる読順を隠さない。
- 新field差が別頁text差に隠れてDiff Fullにならない。
- 旧成功evidenceとcache世代が混在しても版identityを黙って変えない。

## Task 1: Opaque graphics

Files: worker `src/adapters/pdf.rs`, `pdf/graphics.rs`, tests `pdf_paint_semantics.rs`, `pdf_graphics_state.rs`。

- [x] 既存成功15fixture baseline（成功10・拒否5）を旧main114で保持する。
- [x] paint5件のactual REDをMacで確認する（a7f759fe）。
- [x] 有限path/default state、segment bounds・clip包含proofを実装し、16件GREEN/旧baseline完全一致を確認する（fc42b894）。
- [x] ExtGState/page-group4件を実REDにし、既定Normal/完全不透明だけの参照解決を追加する。
- [ ] 曲線・状態復元・色space・切れたclip・Form交差・各予算境界を追加し、pinned rasterとnative evidenceで検証する。
- [ ] Mac rustfmt・focused test・全旧corpus比較・独立reviewを通し同PRへ保存する。

## Task 2: Tagged native structure

Files: worker `pdf/structure.rs`, parent `pdf.rs`, tests `pdf_tag_semantics.rs`。

Interface: `StructureInspector::new(&Document)`、`inspect_page(&mut self, page_id, &[Operation], Option<&Dictionary>, &mut PdfDecodeBudget) -> Result<PageStructure, WorkerFailure>`、`finish()`。`PageStructure`はoptional canonical projectionと独立native textを返す。

- [x] test-only15件のコンパイル問題を直し、BD未対応によるactual REDを確認する。compile失敗をREDと扱わない。
- [ ] bounded treeを一度だけwalkし、page-scoped MCID/ParentTree/Pg/K/RoleMapとmarked contentの一対一対応を検証する。
- [ ] native textをbounded font decodeで照合し、ActualText・読順・Figureの関係を保持する。未対応Table/OC/曖昧構造は拒否する。
- [ ] 既存textはrawのまま保持。MCID/object/resource renumber、Unicode ActualText、Artifact描画、循環/欠落/未知key、深さ/node限界を実証する。
- [ ] Mac GREEN/旧baseline完全一致・独立reviewを確認する。

## Task 3: Diffと派生cache

Files: diff-worker `src/adapters/pdf.rs`とtests、application `document_diff/snapshot.rs`とtests。

- [x] compare_pageのnew/residual fieldだけ、他頁textとの混合差とnative vector差のactual REDを確認する（Partialに対しNone/Full）。
- [ ] vectors/structureの意味変更をpage単位で表示し、範囲不確定はPartialにする。page/top-level残余field差は未検証へ落とす。
- [ ] CACHE_DOMAINだけ新世代にし、from_pair/from_resultの一致と旧key分離を検証する。snapshot/semantic/source/manifestのv0 goldenは不変。
- [ ] mixed-pages、重複page、未知top-level field、再比較/cache hit時の認可・監査と全Diff回帰を確認する。

## Task 4: 公式smallと段階admission

Files: 専用load harness/test/docsのみ。GUI/directory/schedulerは対象外。

- [ ] 最新mainを保持し、独立review・全Rust/Node・Linux DSI/sandbox gateを通す。
- [ ] 同一headの公式原本933/934のnativeDSI・API公開/正当拒否を取得し、正例/負例を根拠付きで分離する。小さい実通知934の限定成功を全PDF成功と扱わない。
- [ ] 登録/一覧/取得hash/OCC/版切替/認可/実HTTP再起動保持と、RSS/DB/storage/timeの実測を完結する。
- [ ] smallが完全成功した実測から次段階をadmitする。1,000→1万→10万を順に評価し、欠測・予算不足はNOT_ADMITTEDとして根拠を残す。
- [ ] exact-head全gate成功後に親へmerge判断を渡し、統合後資格まで追跡する。
