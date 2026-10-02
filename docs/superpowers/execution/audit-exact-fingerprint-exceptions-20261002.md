# Audit design: reuse of approved exact scanner fingerprints

## Scope

This security-only correction to D-AUD1 reuses the requester's already approved31 immutable `commit:path:rule:line` exclusions. It does not add Search/Document acceptance code or a PR43 dependency. `.gitleaksignore` is byte-for-byte identical to the previously reviewed security-only file at `db66c6c838dc2ddaaf3017cf12623aabf5050ab1`.

Every entry targets commit `99c7aca2e7ae3f1f60fa3b948b419a2ae5fc749a`.28 are previously verified SHA256 source-integrity digests, and3 are the previously classified disposable loopback-only synthetic Neo4j fixture occurrences. No credential value is repeated here. No path/rule/commit-wide ignore, new fingerprint, scanner/version change or synthetic-control change is included.

## Fresh evidence

- PR44 head `f95c1bc2273d5291b19de6f024c1bd03c6dca7bc`: [security job110699931773](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36962791921/job/110699931773) reports unchanged control detection1, then history882 commits/findings31. That log contains no individual fingerprint report, so count alone is not asserted to prove set equality.
- Re-materialized official Gitleaks8.30.1 release asset; verified archive SHA256 `551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb` before execution. No scanner-version change.
- Fresh bounded scan of exact authoritative commit99c7aca:678 commits,31 unique findings before exclusions. The complete redacted report fingerprint set equals the31 approved entries, with no unexpected or missing fingerprint.
- Same scanner, same revision/range after adding only those entries:678 commits,0 findings, exit0.
- Existing unchanged synthetic detection-control script passes while still reporting1 finding. The control is not excluded.
- All five source blobs were freshly fetched from GitHub at99c7aca and matched local exact-commit blob IDs:

| Source | Blob |
|---|---|
| P2 source inventory | `47292b44304021741361d41254d65e414352909d` |
| P2 baseline manifest | `20e22133841c7cba18bc5eaf9838be1406a82495` |
| Synthetic runtime qualification | `a731f784e8b07c44a55e9e9d9c995f4291dfabaa` |
| Synthetic recovery qualification | `3571e3258d9ba843aaaa2ca4fc240523b1611f00` |
| Synthetic graph report | `57e9117dc80461420a3b1929f5e618896c17d115` |

The repeated bounded command differs only by presence of the approved ignore file:

```sh
gitleaks git --redact --exit-code 1 --report-format json \
  --report-path <redacted-report-outside-repository> \
  --log-opts=99c7aca2e7ae3f1f60fa3b948b419a2ae5fc749a .
```

## Local all-ref limitation

The separate local all-ref scan is deliberately not claimed to match hosted history. Before exclusions it scanned914 commits and found62 fingerprints: the approved31 under99c7aca plus31 identical path/rule/line occurrences under local mapped checkpoint `85bfd5078abac3e2268d963be892f3b72b9435aa`. Both checkpoints have parent80a47960 and tree `618452f66c716c6b2d01ab151cfdd05b6bc26518`, but their fingerprints differ because commit IDs differ. GitHub returns404 for85bfd507, and it is absent from the45 remote branch heads observed during diagnosis.

No85bfd507 entry is added. That local-only set stays unsuppressed; local all-ref history is not reported green. The decisive gate for this correction is a new hosted scan of the exact published D-AUD1 head. Any remaining/new hosted finding must be investigated; do not widen this file to make a count disappear.

## Unchanged boundaries

Frozen design blob `a1d2002afb7525f19f40138a82365bc975f493ed` and plan blob `3fd2d2ef2b8891dd63345233c5f67f69412dfbed` remain unchanged. Audit schema implementation is on a separate branch and absent from this correction. This is not a backend/runtime qualification or merge/deployment authorization.
