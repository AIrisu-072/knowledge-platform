# Organization統合で保持する日本語資料

統合前に公開・意味保存レビュー済みだった[日本語化PR53](https://github.com/AIrisu-072/knowledge-platform/pull/53)から、下記3つの説明文書だけを採用する。原文はPR57基点でも同一blobであり、後続の内容を旧版へ戻していない。日時・失敗・保留・承認対象SHAは過去の記録として維持し、現在の進捗は[統合状況](../superpowers/execution/document-organization-integration-status.md)を参照する。

| 説明文書 | 原文blob | 採用する既存日本語blob |
|---|---|---|
| [設計レビュー](../superpowers/execution/organization-client-v0-design-review.md) | `0c757ef3ac857d26740e8037d9187ef9457f2359` | `711ee2199c9bf23ade5de74b07cfd0f4b0b91ca8` |
| [Phase0再構成](../superpowers/execution/organization-client-v0-phase0-reconstruction.md) | `bcf9020bfe0728ad4e997dc3e218646bf54eabbc` | `a4a0df4a746700403f387bad9817f82dbd032d41` |
| [要求52節の対応表](../superpowers/execution/organization-client-v0-requirements-map.md) | `da9feab9088366515fe9cb6c8493992a39cd4b4e` | `a79fb58a47c9b5afe9725a5029d32940402140f8` |

採用元のexact commitは `585ccf6ceb4043ac1fd79153522a845fe22f1519`。既存訳文のbyteを保持する。翻訳元との対応・限定意味保存レビューは[元の対応記録](https://github.com/AIrisu-072/knowledge-platform/blob/585ccf6ceb4043ac1fd79153522a845fe22f1519/docs/translations/2026-10-03-organization-overview-ja.md)にある。

凍結designと3つの承認原本は今回変更しない。承認記録の既存日本語訳が必要な場合は、同じ採用元commitにある[Product/UX承認訳](https://github.com/AIrisu-072/knowledge-platform/blob/585ccf6ceb4043ac1fd79153522a845fe22f1519/docs/superpowers/specs/2026-10-02-organization-client-v0-product-ux-approval.md)、[Domain/API承認訳](https://github.com/AIrisu-072/knowledge-platform/blob/585ccf6ceb4043ac1fd79153522a845fe22f1519/docs/superpowers/specs/2026-10-02-organization-client-v0-domain-api-approval.md)、[UI承認訳](https://github.com/AIrisu-072/knowledge-platform/blob/585ccf6ceb4043ac1fd79153522a845fe22f1519/docs/superpowers/specs/2026-10-02-organization-client-v0-ui-approval.md)を参照できる。訳文によって別blobが新しく承認されたとは扱わない。

PR52/53のbranch、Tauri関連lock/runtime、失敗した適格性検証を統合するものではない。
