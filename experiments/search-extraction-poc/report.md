# P1-V01 capacity probe

Environment: Darwin arm64, rustc 1.98.1 (48a229cea 2026-09-01), head `96678e038281`. Readers in-process; no sandbox/cgroup numbers. No production SLO is declared from these values.

| case | MiB | status | outcomes | Units | cold ms | warm ms | query p50/p95/p99 µs | CPU s |
| --- | ---: | --- | --- | ---: | ---: | ---: | --- | ---: |
| text | 1 | measured | Completed/Some(Supported)×1 | 31060 | 1934 | 1771 | 251/3738/3770 | 4.261 |
| text | 10 | measured | Completed/Some(Unsupported { reason: ResourceLimit })×1 | 0 | 105 | 102 | 1/1/2 | 0.233 |
| text | 50 | measured | Completed/Some(Unsupported { reason: ResourceLimit })×1 | 0 | 392 | 385 | 1/1/1 | 0.882 |
| csv | 1 | measured | Completed/Some(Unsupported { reason: ResourceLimit })×1 | 0 | 19 | 18 | 1/1/1 | 0.047 |
| csv | 10 | measured | Completed/Some(Unsupported { reason: ResourceLimit })×1 | 0 | 31 | 31 | 1/1/2 | 0.109 |
| csv | 50 | measured | Completed/Some(Unsupported { reason: ResourceLimit })×1 | 0 | 87 | 87 | 1/1/1 | 0.393 |
| html | 1 | measured | Completed/Some(Supported)×1 | 26414 | 2478 | 2452 | 215/4000/4171 | 5.474 |
| html | 10 | measured | Completed/Some(Unsupported { reason: ResourceLimit })×1 | 0 | 114 | 115 | 1/1/1 | 0.275 |
| docx | 1 | measured | Completed/Some(Supported)×1 | 16028 | 797 | 789 | 142/3591/3782 | 2.052 |
| docx | 10 | measured | Completed/Some(Unsupported { reason: ResourceLimit })×1 | 0 | 109 | 107 | 1/1/1 | 0.276 |
| zip-high-ratio | 10 | measured | Completed/Some(Unsupported { reason: ResourceLimit })×1 | 0 | 2 | 1 | 1/1/2 | 0.011 |
| zip-high-ratio | 50 | measured | Completed/Some(Unsupported { reason: ResourceLimit })×1 | 0 | 2 | 1 | 1/1/2 | 0.02 |
| many-parts | 1 | measured | Completed/Some(Supported)×200 | 33854 | 522 | 518 | 1741/3892/3924 | 1.753 |
| many-documents | 1 | measured | Completed/Some(Supported)×200 | 33200 | 526 | 540 | 309/3945/3990 | 1.666 |
| max-unit | 1 | measured | Completed/Some(Supported)×1 | 1 | 5 | 4 | 1/2/2 | 0.015 |
