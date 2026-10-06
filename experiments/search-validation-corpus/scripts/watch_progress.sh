#!/usr/bin/env bash
# Samples delivery, generation, Vector and process resources every N seconds
# into $ROOT/logs/progress.tsv until stopped.
set -uo pipefail
ROOT=${ROOT:-$HOME/kpval}
INTERVAL=${1:-30}
# shellcheck disable=SC1090
source "$ROOT/secrets/db.env"
out="$ROOT/logs/progress.tsv"
[[ -f "$out" ]] || printf 'time\tdelivered\tpending\tdead\tready_generations\tvector_entries\tworker_cpu\tworker_rss_kb\tdisk_used_kb\n' > "$out"
while true; do
  row=$(psql "$DATABASE_URL" -AtF $'\t' -c "select
    (select count(*) from outbox_events where delivered_at is not null),
    (select count(*) from outbox_events where delivered_at is null and dead_lettered_at is null),
    (select count(*) from outbox_events where dead_lettered_at is not null),
    (select count(*) from search_generation where state='READY'),
    (select count(*) from search_vector_entry)" 2>/dev/null)
  pid=$(cat "$ROOT/run/search-worker.pid" 2>/dev/null)
  stats=$(ps -o %cpu=,rss= -p "$pid" 2>/dev/null | awk '{print $1"\t"$2}')
  disk=$(du -sk "$ROOT" /var/lib/postgresql 2>/dev/null | awk '{s+=$1} END {print s}')
  printf '%s\t%s\t%s\t%s\n' "$(date -Is)" "$row" "${stats:-\t}" "$disk" >> "$out"
  sleep "$INTERVAL"
done
