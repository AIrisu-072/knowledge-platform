# P3 native backend license and offline reproducibility inputs

Status: documentary inputs verified 2026-09-30; candidate license gates remain
`UNMEASURED` until exact installed artifacts and intended distribution/hosting
scope are checked during qualification. This note is not a legal clearance.

| Candidate | Upstream statement | Qualification action |
| --- | --- | --- |
| PostgreSQL 18.6 | [PostgreSQL License](https://www.postgresql.org/about/licence/) permits use, copy, modification and distribution with its notice conditions. | Record exact cached image ID/digest and shipped notice path; check actual intended packaging. |
| redb 4.3.0 | Local pinned crate `redb-4.3.0/Cargo.toml` declares `MIT OR Apache-2.0`; upstream [source](https://github.com/cberner/redb) contains both license files. | Record `Cargo.lock`, resolved crate SHA and notice choice; perform `cargo --offline --locked` build only in the released Cargo slot. |
| Neo4j Community 2026.09 | [Neo4j's official page](https://neo4j.com/open-source-project/) states Community is GPLv3; Enterprise has a commercial license. | Record exact cached Community image and its bundled license; obtain a separate packaging/use assessment before any production recommendation. |

The candidate launcher uses `docker image inspect` on the exact local image ID
and `docker run --pull=never`. The redb build must use the isolated PoC lockfile
with `--offline --locked`. Python imports use the existing local `.venv` and
`requirements.txt`; no `uv pip install`, image pull, model download, external
provider, or production dependency addition belongs to this run.

For restore, PostgreSQL uses `pg_dump -Fc` and `pg_restore` into a different
database. Neo4j Community uses [offline dump](https://neo4j.com/docs/operations-manual/current/backup-restore/offline-backup/)
and [offline load](https://neo4j.com/docs/operations-manual/current/backup-restore/restore-dump/)
with the same pinned image. Both must be verified against complete native rows
and all 16 Memory oracle scenarios after restore.
