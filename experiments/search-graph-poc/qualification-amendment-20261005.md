# P3-P04 qualification amendment — 2026-10-05

This amendment precedes the second hosted measurement round. It changes only how
candidates are provisioned; query classes, cells, sample counts, budgets and gates in
[qualification-method.md](qualification-method.md) are unchanged.

## Measurement environment

The owner chose on 2026-10-05 to measure before selecting the Graph backend. Each
backend/profile pair runs on its own GitHub-hosted `ubuntu-24.04` runner through
`.github/workflows/search-p3-qualification.yml`, triggered only by pushes to
`measure/search-p3-**`. One runner per candidate gives the exclusive CPU window the
method requires without stopping local implementation work. Images are pulled by the
official index digests already used in this repository:

- `postgres@sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650`
- `neo4j@sha256:0dfcadbd51e1d2e5a0cf86d054a0bb537a4bce212796d967cbd88445aaf3fbf3` (2026.09.0 Community)

The runner's local image ID for that digest is passed to the harness, which still
refuses an image ID it cannot inspect.

## Changes after round 1 (run 37251275463)

1. **Historical base fixtures.** The 1,000 and 3,000 group bases pinned in
   `qualification.py` were produced before the generator added the Source-2 canary, so
   `search-graph-backend-poc make` no longer reproduces their bytes. Round 1 stopped
   before measuring with `base fixture hash mismatch`. The unchanged historical bytes are
   now stored as `base-fixtures/fixture-{1000,3000}.json.gz` (gzip `-n -9`); the
   decompressed SHA-256 values still equal `BASE_SHA256`. The 100 group base is still
   regenerated and verified.
2. **Neo4j transaction log preallocation.** Round 1 stopped Neo4j 100 at the fixed
   fixture budget (`candidate data < 512 MiB`) right after staging. Neo4j preallocates
   transaction log files by default, which counts against that budget without holding
   Graph data. Candidate and restore containers now set
   `NEO4J_db_tx__log_preallocate=false`. Heap, page cache and memory limits are unchanged.
   The round 1 failure stays in the record as the result under the default
   configuration.
3. **Offline restore directory on Linux.** `neo4j-admin database load` runs as the
   explicit `neo4j` user against a host bind mount. On Linux the mount keeps the
   runner's ownership, so the load failed with a permission error. The harness now makes
   that disposable, harness-owned restore directory writable before the offline command.

PostgreSQL and redb provisioning is unchanged. Round 1 receipts for redb 100 remain
valid evidence for that harness version only; round 2 re-measures every cell with one
harness version so the comparison does not mix source hashes.

## Changes after round 2 (run 37251744747)

4. **Canary slices on hosted runners.** Every PostgreSQL and Neo4j job stopped at the
   predeclared five-minute profile canary after 36–42 of 48 cells; redb and PostgreSQL
   3,000 completed inside one slice. The method already defines this stop as a
   diagnosis point followed by `--resume`, which keeps the saved cells and raw attempts.
   The workflow now resumes a slice only when the recorded error is exactly that canary,
   at most 20 times; any other error ends the job.
5. **Restore directory ownership.** The permission change from item 3 now applies only
   to a directory owned by the runner user. The live candidate directory, which the
   Neo4j entrypoint hands to its own user, is left unchanged.

## Changes after round 3 (run 37253346842)

6. **Offline load user.** PostgreSQL and redb completed every cell plus restart,
   restore and fault probes. Neo4j completed its timed cells after one canary resume,
   then `neo4j-admin database load` into the harness-created restore directory failed
   on Linux. Dump still runs as the owning `neo4j` user; load now runs as the image
   default user, and the restore container's entrypoint hands `/data` to `neo4j` before
   the server starts. A failed offline command now reports the last 2,000 bytes of its
   stderr so a remaining failure is diagnosable from the receipt.

## Changes after round 4 (run 37254157214)

7. **Load input stream.** The captured stderr showed `Not a valid Neo4j archive:
   reading from stdin`: the harness passed the dump bytes on stdin but started the
   offline load container without `-i`, so the container never received them. The
   load command now attaches stdin. The dump, the timed cells and the PostgreSQL and
   redb paths are unchanged.
