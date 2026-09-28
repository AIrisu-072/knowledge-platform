# Document Management Basics v0 — 既存設計への接続

- 状態: **承認済み設計の適用範囲を明記**
- 日付: 2026-09-28 JST
- 根拠: 承認済み Document Management Basics v0 設計 §9、§12、§17 と実装計画 §7。依頼者は計画の exact blob を承認し、同じ意味の追記を実装前に行うよう指示した。

## Document Versioning v0 の予約実行

既存 Versioning 設計 §9 は、当時 AccessPolicy を対象外とし、期限到達時に service executor が記録済み予約を実行する契約だった。Document Management Basics v0 の認可付き経路では、依頼者の現在の identity と issuer 付き membership を信頼済み resolver から再取得し、同じ確定 transaction 内で現在の `read + publish` を確認する。service executor が依頼者の権限不足を迂回しない。権限不足が確定すれば既存の監査付き終端処理、一時的な identity 障害なら同一 ID で再試行する。依頼者を service executor に偽装せず、双方を監査で区別する。既存の DSI、品質、DB 時刻、manifest、Publish operation ID、重複 worker の契約は維持する。

## Document Publication End v0 (T10) の履歴参照

既存 T10 設計の通常公開遮断は維持する。新しい履歴専用経路では、明示 Document/Version ID と現在の `read + read_history` を要求し、残った `WORKING` 内容には `write` も要求する。T10 の `current_version_id = null`、元の Version の状態・内容、通常取得の非フォールバックを変更しない。履歴経路を通常公開 API に流用しない。

この接続は過去の設計承認記録を書き換えない。実装中に上記を超える意味変更が必要なら、その差分だけを設計改訂としてレビューする。
