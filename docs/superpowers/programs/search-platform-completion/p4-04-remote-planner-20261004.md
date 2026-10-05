# P4-04 四つのremote modeの計画 — 2026-10-04

状態は**対象限定のローカルGREEN、独立レビュー待ち**です。基準は `4dd1c0d42fe492b682cd26a37675b4a61d8da101`。凍結済み[P4-04](p4-remote-plan.md#p4-04--four-mode-planner-support-and-typed-inputs)と[設計改訂1](p4-remote-design-revision-1.md)の計画境界だけを実装しています。実通信、remote executor、generation封印、P4全体の資格取得は含みません。

## 判断と影響

- `SourceRouter::plan_with_runtime_modes` を追加し、実際に配線されたmodeと可視登録の積集合からroute stateを作ります。可視登録と制約は既存の `VisibleRouting::prepare` から渡します。旧 `plan` のlocal専用動作は保持し、過去に作ったgapを文字列照合で削除しません。公開サービスへの接続と認可の再照会は後続のP4-12/13です。
- `RetrieverSupport` に4つの独立したflagを追加し、sourceごとのquery/native ID/live入力を照合します。未配線は `Unsupported(NoExecutionPort)`、必要入力の欠落は `Unresolved` です。登録されていないmode・不可視Sourceにactionを作りません。`Planned` とcursorは計画だけを表し、実行・結果の再利用・absenceを証明しません。
- 有効なRequired remote routeはInitialのまま、正のfan-out枠があればactionもInitialにします。明示上限0は超えず、未計画のblocking gapを保持します。localの同じ上限0動作、local profile順、Source順、S1のretriever順は変えません。remoteは同一routeのlocal順の後に列挙・query・lookup・liveの固定順で加えます。
- inputのfieldは非公開とし、検証済みconstructorだけで生成します。既存の `StructuredFacetFilter` とscalar `TypedValue` を再利用し、facet名はtrusted adapterのallowlistと照合します。初期上限はquery 4,096 bytes、facet 16個、名前128 bytes、文字値1,024 bytes、window 1〜100、native ID 512 UTF-8 bytes、JSON化した入力16 KiBです。重複facetと再帰的List/Setは拒否します。これらはcanary入力の上限であり本番SLOではありません。複合facetが必要なら型の境界を明示的に拡張し、実transportは登録ごとの上限と完全なwire requestを独立に検証します。
- native IDは空白だけ・制御文字・scheme付きURI（`file:/...` なども含む）・URL形式を拒否し、fetch先には使いません。live入力は検証済みqueryまたはnative IDのどちらか一つです。local queryをremote入力へ暗黙コピーしません。query・facet・native IDの生データは3型の `Debug` に出さず、固定の伏せ字を返します。

## 検証と残作業

固定Rust 1.98.1、offline/locked、jobs 2、debug 0、incremental 0を使用し、Cargo枠を直列で占有しました。依存関係・root設定・migration・P4-03のファイルは変更していません。

- RED: 実装前の `cargo test -p search-application --offline --locked --test remote_planner_contract` はexit 101。必要な型・field・入口・issueの未実装だけで22件のコンパイルエラーでした。
- GREEN: 当初の8契約が成功。独立レビューでscheme付きURIとDebugの2件を追加し、修正前7 pass / 2 failを観測しました。修正後は9契約と、routing 12、visible routing 5、retrieval execution 18、discovery loop 46、合計90/90成功です。既存routing testの変更は新fieldに対する `..Default` の6行だけです。
- `cargo clippy -p search-application --offline --locked --lib --test remote_planner_contract --test routing_contract -- -D warnings`、変更4 Rustファイルのrustfmt、`git diff --check` はexit 0です。
- ログSHA-256: RED `acf517c4ccd37cc8593ea4160e947bf724fe768ad8f4985fa2d1220cb612796e`、最初のGREEN `f2eac13c4164073876b4a4226060907cb8ea026aaf694bbbf0a82707fa6031ae`、89件回帰 `90f672e2653a2848354b543af6f96f161d4705015d4dda7a15f2bb4e01c2ba54`、Clippy `d9f2738b5902274cc340951db029c3c3d974e28d2c2da30c972d2f9c30392f32`。原本は実施環境に保存しています。
- 修正後ログSHA-256: レビュー指摘のRED `06bdaa0cc5c4d6e6c4dfb4d108c35b48d27e0643ee9637b87b9918bab5063130`、90件回帰 `47ab017d6ef3050af6d3df4c5f806e994a9ea59d05938d3549e47325769c795f`、Clippy `8e4d26c576bd8c139dda5c9686f45370d6db43dd93b8f814b0e6358a0ba7d022`。

次の作業はこの限定差分の独立レビューです。commit・公開・merge・deployは実施していません。ホストCI、P4-05以降、P1〜P7/プログラム全体の受入は未完了です。
