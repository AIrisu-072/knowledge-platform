# Document GUI Integration v0 — 書面設計承認記録

- 日付: 2026-09-30 JST
- 状態: **APPROVED — written design freeze active**
- 対象PR: #35
- branch: `design/document-gui-integration-v0`
- 承認対象: `docs/superpowers/specs/2026-09-30-document-gui-integration-v0-design.md`
- 承認対象blob: `f132910ca5d3e638502f0b38447d9a1ec4020f24`
- 提示head: `1872df466f843290aa42886e0afc9a903781ee08`

依頼者はWritten Design提示後に「設計を承認します。実装は別のセッションで行うのでプロンプトを作成してください。」と明示した。

この発言を、上記blobの書面設計を承認して凍結し、Production Implementation Planと別session handoffの作成へ進む指示として記録する。

## 承認済み主要事項

- `DocumentVersion.version_no`（内容世代）、`DocumentRevision Major.Minor`（人間向け正式改訂）、`Document.revision`（OCC）を分離する。
- Human-facing MajorはDocument単位で単調増加し、Withdraw fallbackで過去DocumentVersionがcurrentへ復元されても表示改訂番号を逆行させない。
- T5 Document metadataの実変更だけが同MajorのMinorを増やす。Folder / ACL / ReadState / schedule / T10単独では表示改訂番号を増やさない。
- WORKING Versionは正式Revisionではない。Publish / withdraw fallback / metadata mutationのauthoritative transactionへDocumentRevision発行を原子的に接続する。
- GUI Read Model、Action Capability Projection、Identity Presentation、Revision/Diff Display Projection、Typed Client/Binary Transport Bridgeの5境界をGUI統合の中核とする。
- Identity display nameをDocument DBの正本としてコピーしない。
- Diff display fragmentをFrontend parserで生成せず、authoritative bytes + source locatorから認可・監査付きbounded projectionとして生成する。
- OpenAPI 3.2.1をclient tooling都合でdowngradeしない。
- React GUIはレビュー済みSource Designの3-pane / Document Workspace / Motion / Keyboard / Conflict / Partial semanticsを維持する。
- React 19 / Vite 8 / TanStack Router+Query+Table+Virtual / Motion / CSS Modules + CSS Custom Propertiesを基準とし、React Aria Componentsとtyped client generatorは資格試験後にpromotionする。

## 承認に含まれないもの

- Production Implementation Planの承認。
- DB migration、Domain/Application/HTTP/Frontendのproduct implementation。
- OpenAPI product contractの変更。
- production dependency promotion。
- predecessor stacked PR #27/#29/#30/#31/#32のmerge。
- production deploy。
- Windows/AD/SSPI本番Identity接続。

設計本文はこのblobのまま凍結する。意味変更が必要な場合はDesign Amendment gateへ戻る。
