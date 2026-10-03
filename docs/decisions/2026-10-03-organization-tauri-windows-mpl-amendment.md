# ADR amendment: exact additional Windows-first MPL qualification

Date: 2026-10-03 UTC. Status: **OWNER SCOPE APPROVED; QUALIFICATION NOT YET ESTABLISHED**.

## Authority

This amends the [original host-only ADR](2026-10-03-organization-tauri-host-mpl-qualification.md),
without rewriting its historical pending checkpoint. The earlier permission remains
cssparser 0.37.0, selectors 0.38.0 and cssparser-macros 0.7.0 for host build/proc-macro
qualification only. Original §§46/49/50 and the [52-section map](../superpowers/execution/organization-client-v0-requirements-map.md) remain controlling.

Assistant question `Sentinel_59383b2800ac8191a2fe76e8e0e22d0a`, 2026-10-03T00:56:34Z:

> 追加の判断は2件です。検証用の例外を次まで広げてよいですか？
>
> - dtoa-short 0.3.5（MPL-2.0）：ビルド時のみ
> - option-ext 0.2.0（MPL-2.0）：ビルド時とアプリ実行時
>
> まずWindows向けを検証する方針を勧めます。Linux側には別のライセンス制約と保守停止の依存があるため、そちらの例外は含めません。今回も本番採用・配布は対象外です

Owner reply `Sentinel_59ca27ae4d9c8191a8fdfe8801327b91`, 2026-10-03T02:15 UTC:

> 広げていいです。

## Exact intersection

| Package | Version | Permitted qualification use |
|---|---|---|
| cssparser | 0.37.0 | host build/proc-macro only, original permission |
| selectors | 0.38.0 | host build/proc-macro only, original permission |
| cssparser-macros | 0.7.0 | host proc-macro only, original permission |
| dtoa-short | 0.3.5 | host build only, additional permission |
| option-ext | 0.2.0 | host build and Windows application target, additional permission |

No other version, package, normal CLI/tool use, Linux-specific license/advisory
exception, production adoption or distribution is approved. License permission
is not completed qualification, a security waiver, acceptance of Microsoft terms,
or proof that covered source is absent from generated/binary output.

The repository root deny.toml and native-Windows/backend policies remain unchanged.
Any proposed fixture-local scanner exception is exact-name/version only and must
be preceded by a separately verified role predicate. Scanner configuration cannot
express host/target scope by itself. Preserve both raw-policy failures and scoped
results; never relabel a raw failure as a pass. No advisory ignore or source allowance.

The full unpublished b4a inventory remains unavailable. Recreate fresh reproducible
lock/source/notice/advisory evidence; historical counts/hashes are not current proof.
Keep notices and examine generated/output inclusion. Separate legal/source-availability
and recipient obligations must be established before any future distribution proposal.

Next: independently review the [metadata-only subdesign](../superpowers/specs/2026-10-03-organization-tauri-windows-inventory-design.md)
and [plan](../superpowers/plans/2026-10-03-organization-tauri-windows-inventory.md).
No runtime or dependency build is approved by their first slice.
