# P4-05 Remote操作・観測契約の限定検証 — 2026-10-04

**状態：ソース・純粋契約の限定GO。P4全体の受入ではない。**

対象は `p4-remote-plan.md` P4-05 と改訂設計§§2〜4、SD-T8/9/10。隔離branchは `feat/search-p4-observation-20261004`、基準commitは `3738e286e6f859021d94c6b0066fe0a64ea45d52`、基準treeは `ca7ecabe4fe8732b12cdf80d9d43df2645a00ada`。この基準treeは公開PR #40の `1571ee49af66defa6a0e2b738c81f299a2c0ef4f` と同一として引き継いだ。ここでは新commit・remote書込み・merge・deployを行っていない。

## 実装と判断

- `remote.rs` に4操作、Source単位の有界batch、閉じた結果型、opaque context/lease ID、native identity、観測済みversion/digestのpin、`RemoteSourcePort::{execute_batch,current_access,current_policy,probe_or_materialize}` を追加した。`BoxFuture`、既存typed input・Source・CurrentSourcePolicyを再利用し、HTTP型・依存追加はない。
- Contextは台帳で認証された `VisibleSourceRegistration` だけから作る。既存actor/evaluationとSource scope/登録activationを保ち、verifierのawait前後に既存authority/visibilityで現在性を検査する。raw登録DTOの同じID/revisionへの差替えでは権限を得られない。
- Snapshot verifierはhost配線のprotocol検証境界であり、actor/Sourceを発行しない。未配線・不明・登録の完全列挙能力と矛盾する応答はUnknownへ閉じる。provider totalやstatusは完全性の証拠にしない。
- `VerifiedAbsence` のconstructorは非公開。同一context/snapshotの先頭からterminalまでのcursor列、登録済み完全列挙、検証済み既知ID、page/hit上限、対象ID不在をすべて要求する。cursor反復・欠落、IDなし・重複hitでは発行しない。query/live/probe miss、partial、通常403/404、timeout/outageはAbsentにしない。Resource正本の変更portや永続書込みは追加していない。
- **判断：direct absenceは未資格。** 現在の登録にはauthoritativeかつACL-unmaskedなexact lookup能力が存在しない。`authority_predicates` や `PublicReadWithFieldPolicy` をその許可へ読み替えず、direct missをすべてUnknownに保つ。影響は、後続で明示的な登録能力と実adapterの証拠が揃うまでdirect absenceを利用できないこと。
- **判断：履歴証明と現在の利用許可を分ける。** 指定された同期 `verify_absence` は観測時の正確なcontextを検証する。checked adapterの同名asyncメソッドは利用時のactor/Sourceを前後で再検査する。証明は将来のaccess grantではなく、後続lease/disclosureも現在性を確認する。Source取消後に古い証明をchecked経路で使用できない試験を含む。
- 個々の列挙pageのcoverageは常にPartialEnumeration。全列の検証が成功したreceiptだけがCompleteEnumeration/Absentを返す。共有Source snapshotの証明がないSingleResponseは複数応答の共有snapshotにしない。

## RED / GREEN と独立レビュー

Rust 1.98.1、既存toolchain/cache、`--offline --locked`、jobs=2、dev/test debug=0、incremental=0を使用。空き領域は15 GiB以上で、共有Cargo占有を他作業と直列化した。

1. 新契約import欠落のRED：exit 101。これは意味的失敗ではなくAPI未実装のコンパイル失敗。
2. IDなし/重複列挙hitの意味的RED：11 pass / 1 fail。完全列挙receiptのguard追加後にGREEN。
3. 独立レビューで同ID/revisionの登録差替えと、同native IDの異なるdigestから先頭pinを選ぶ問題を確認。各1件の意味的RED後、catalog認証済み入力限定と複数match拒否で修正した。
4. Identity Debugのtenant漏出を1件のREDで確認し、opaque表示へ修正した。
5. 最終 `cargo test -p search-application -p search-core --offline --locked`：純粋350件＋doc 8件成功。新規観測契約17件と、context/absenceの非公開constructorを確認する2件のcompile-fail docを含む。
6. 最終 `cargo clippy -p search-application -p search-core --all-targets --offline --locked -- -D warnings`：exit 0。変更Rustのrustfmtとdiff whitespace確認も成功。
7. 独立read-only reviewerは修正後ソースに限定GO、残存Critical/Importantなし。レビュー自体はCargo/runtimeを実行していない。

## 正確な対象と証拠

SHA-256（検証時のソース）：

| 対象 | SHA-256 |
| --- | --- |
| `crates/search-application/src/remote.rs` | `98d1e934063fa3d4402738831455a897e5aed7a465a8e860cb224c344c41caa4` |
| `crates/search-application/src/remote_observation.rs` | `c22591fe8eab0c990de1baa187932d06aea264fec2610834fef7443c0d8e29cb` |
| `crates/search-application/tests/remote_observation_contract.rs` | `cd33e7381d65d552c69aa2d37dc4c4d9c9cb8f9524afbb4a620762221fdf9b34` |
| `crates/search-application/src/lib.rs` | `388ce1e44a50038a46399cb223a495d2f70dbca18d86cf1d7d2a04dd23648ef2` |

実行logは作業領域の `search-p4-observation-evidence/` に保全し、repositoryへraw logは追加していない。

| 証拠 | SHA-256 |
| --- | --- |
| `01-contract-red.log` | `a29b446f9ff71bea1c9da2fd37339db5216d609bfb8050a76813637ffac01bc0` |
| `03-idless-red.log` | `39fcf0d75c09ebf44658ee25604208d0bf9cdcc76bf37e4a7b9e26c7d767019d` |
| `06-registration-red.log` | `a7a6a6ed5209e587ffca37c4f95e1b90f7ad96a16268a37be2d55d6c8037d1e0` |
| `07-conflicting-pin-red.log` | `3e7a158988897cb6d44f8076a6a243d215d735b3e60476715594251b24f661ff` |
| `09-identity-debug-red.log` | `7cabf4a80276fae99a553d8ae3dca6688bbea37bb91ed428f2902af909063a1e` |
| `10-final-application-core-tests.log` | `34901e5f4cd2629f54d63d62ebe5653d703718317ecd57b857cf8e18ab51b4f4` |
| `11-strict-clippy.log` | `938a31e0e527872e85d6ac9cc6beeac7ebdddb3cd6ff69ec2984e583bfdb76a0` |

## 残る範囲と次の操作

このsliceには実provider/HTTP/DB/プロセス起動、generationのseal、lease/store、field/provenance、本文payload、最終disclosure、production配線の資格はない。新しい依存・権限発行元・global registryも作っていない。`RemoteReadOutcome` は観測metadataの契約だけで、content取得やmaterialization完了の証拠ではない。

次は親作業で5ファイルのhash・差分・独立GOを確認し、統合先へのcommitを調整する。その後、P4-06以降の凍結されたgeneration/lease契約へ進む。P4全体、実通信、hosted CIの合格をこの純粋検証から推定しない。
