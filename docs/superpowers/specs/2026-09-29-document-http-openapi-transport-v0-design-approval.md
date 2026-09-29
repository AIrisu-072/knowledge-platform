# Document HTTP/OpenAPI Transport v0 — 書面設計承認記録

- 日付: 2026-09-29 JST
- 状態: **APPROVED — written design freeze active**
- 対象PR: #27、`design/document-http-openapi-transport-v0`
- 承認対象ファイル: `docs/superpowers/specs/2026-09-29-document-http-openapi-transport-v0-design.md`
- 提示済みPR head: `55ad0e5b005cde93a225f4af564c33cd6bd242ac`
- 承認対象blob: `88f7046a5d14a77f4091df0c92691f6634dd57d7`
- 基準main: `77b13a1d35d15eea0112ca2d73f8cbd3dfffe1c9`

## 承認の根拠と対象

依頼者にDocument HTTP/OpenAPI Transport v0の設計書とDraft PR #27を提示し、主要境界と自己レビュー修正を説明した後、依頼者は「設計を承認します。実装は別セッションで行うので実装前まで進めて実装用のプロンプト作成まで行ってください。」と明示した。

この回答を、上記blobの書面設計を承認して設計を凍結し、Production Implementation Planの作成・レビュー準備まで進む指示として記録する。

承認対象には以下を含む。

- OpenAPI 3.2.1 / JSON Schema 2020-12をDocument HTTP API契約の正本とすること。
- Human UIとLLM / Agent / CLIが共通APIを利用し、業務ロジックをクライアント種別ごとに複製しないこと。
- HTTP入力からPrincipal、Group、Role、InvocationKind、delegationを自己申告させず、信頼済みIdentity Adapterだけが `VerifiedActorContext` を生成すること。
- TransportからRepository、SQL、FileStorage、Diff workerへ直接接続せず、既存の認可済みApplication入口を通すこと。
- 既存caller-generated UUIDv7 operation ID、expected revision、target resource IDを再試行・OCCの正本として維持すること。
- initial create commit outcome unknownのblind retryを禁止し、認可済みrecovery lookupを追加すること。
- AccessPolicy read、root Folder discovery、型付きmanagement error surface等、設計で明記した狭いApplication補完。
- file accessで必須Audit commit前にresponse bodyを開始しないこと。
- Document Diffのverdict / coverage / unverifiedを保持し、PartialをSame/確認完了へ変換しないこと。
- RFC 9457、W3C Trace Context、OpenTelemetry、有限resource/timeout/cancellation、same-origin優先、no-store等のtransport境界。
- OpenAPI 3.2 codegen候補はPoC資格完了前にproductionへ追加しないこと。

承認された設計本文は変更せず保存する。本文冒頭の `PROPOSED / WRITTEN SPEC REVIEW PENDING` は作成時点の表示であり、本承認記録が上記blobの現在の承認状態を示す。

## 承認に含まれないもの

- これから作成するProduction Implementation Planの承認。
- 製品コード、OpenAPI paths/schema、Cargo/npm production dependency、migration、実装PRへの着手。
- GUI、CLI、Agent Tool / MCP、Search API、Windows/AD/SSPI本番接続。
- PR #27のmerge、本番deploy、本番データmigration。
- 設計本文の意味変更。必要なら新blobに対するDesign Amendment承認へ戻る。

## 次工程

凍結設計を基に `docs/superpowers/plans/2026-09-29-document-http-openapi-transport-v0-production-implementation.md` を作成し、依頼者レビュー用に固定する。

Production Implementation Planの明示承認と実装開始指示が記録されるまでは製品実装を開始しない。別セッションへ渡す実装用プロンプトも、この計画と承認状態をGitHub正本から再取得するよう構成する。
