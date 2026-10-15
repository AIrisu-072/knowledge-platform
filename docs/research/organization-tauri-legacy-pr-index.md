# 旧Tauri資格確認PRの履歴索引

日付：2026-10-15 UTC。旧PR50〜53の未統合資料を、固定commitの履歴資料として保全する索引。現在の採用・資格取得を示す文書ではない。

## 固定した資料

| PR | exact head / 固定tree | 資料の種類と現mainとの差分 |
|---|---|---|
| [50](https://github.com/AIrisu-072/knowledge-platform/pull/50) | [`8927886bbbcb02bfa2098f38155572028b31bd07`](https://github.com/AIrisu-072/knowledge-platform/tree/8927886bbbcb02bfa2098f38155572028b31bd07) | Tauri phase4 preflightとlicense gate。preflight文書はmain未統合。active/statusは後続の更新があるため旧状態を現在へ戻さない。 |
| [51](https://github.com/AIrisu-072/knowledge-platform/pull/51) | [`629822a49657bf447da9061c568dc72558265ce7`](https://github.com/AIrisu-072/knowledge-platform/tree/629822a49657bf447da9061c568dc72558265ce7) | MPL限定承認、資格確認の復元記録・履歴JSON、review、設計、計画。新規6文書はmain未統合。 |
| [52](https://github.com/AIrisu-072/knowledge-platform/pull/52) | [`152262b5ff08e9280de0c004461da2152f0de4f2`](https://github.com/AIrisu-072/knowledge-platform/tree/152262b5ff08e9280de0c004461da2152f0de4f2) | Windows inventory、receipt、review、限定例外、資格確認toolとlock。大部分はmain未統合。`.gitleaksignore`はmainと同一、checksum判断書はmainで日本語化されている。 |
| [53](https://github.com/AIrisu-072/knowledge-platform/pull/53) | [`585ccf6ceb4043ac1fd79153522a845fe22f1519`](https://github.com/AIrisu-072/knowledge-platform/tree/585ccf6ceb4043ac1fd79153522a845fe22f1519) | 日本語化。review・phase0再構成・要求対応表の3文書はmainと同一。下記4文書は未統合のまま固定SHAで保存し、現行採用とは扱わない。 |

照合先はmain `ead6830e`。上記は各PR自身の差分についての記録であり、積み重なった下位PRの資料も固定treeから参照できる。

PR53の未統合の日本語資料：

- [Domain/API承認記録](https://github.com/AIrisu-072/knowledge-platform/blob/585ccf6ceb4043ac1fd79153522a845fe22f1519/docs/superpowers/specs/2026-10-02-organization-client-v0-domain-api-approval.md)
- [Product/UX承認記録](https://github.com/AIrisu-072/knowledge-platform/blob/585ccf6ceb4043ac1fd79153522a845fe22f1519/docs/superpowers/specs/2026-10-02-organization-client-v0-product-ux-approval.md)
- [UI承認記録](https://github.com/AIrisu-072/knowledge-platform/blob/585ccf6ceb4043ac1fd79153522a845fe22f1519/docs/superpowers/specs/2026-10-02-organization-client-v0-ui-approval.md)
- [Organization概要の日本語資料](https://github.com/AIrisu-072/knowledge-platform/blob/585ccf6ceb4043ac1fd79153522a845fe22f1519/docs/translations/2026-10-03-organization-overview-ja.md)

現mainの[checksum判断書](../decisions/2026-10-03-organization-tauri-checksum-findings.md)が参照する[PR52の範囲検証receipt](https://github.com/AIrisu-072/knowledge-platform/blob/152262b5ff08e9280de0c004461da2152f0de4f2/docs/research/organization-tauri-windows-inventory/gitleaks-scope-receipt.json)も、この固定履歴の一部である。

## 現行の資格と整理の境界

現行Tauriの承認と資格は[2026-10-07の判断書](../decisions/2026-10-07-tauri-v2-desktop-qualification.md)と[実行状況](../superpowers/execution/desktop-workspace-runtime-status.md)を参照する。旧PR52の限定例外とは別の承認であり、旧資料の成功・例外・期限を現mainの資格として流用しない。本索引は旧tool・lock・例外を有効化しない。

旧PRの全差分をmainへ統合したとは扱わない。整理理由は「歴史資料を固定SHAで保全し、現行Tauriは別承認へ移行した」である。履歴参照を維持するため旧PRのbranchは削除しない。

現時点で旧PRのcloseは未実施。索引の独立レビュー、PR116の正確なheadの必須CI、main統合と統合後CIを確認した後、親担当が承認範囲内でcloseする。closeしても未統合資料の採用を意味しない。
