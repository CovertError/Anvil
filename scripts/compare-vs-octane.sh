#!/usr/bin/env bash
# scripts/compare-vs-octane.sh — same-box A/B benchmark between Anvilforge
# and Laravel Octane (Swoole). Drives `oha` against both stacks from the
# host so the comparison is honest: same loadgen, same network conditions,
# same machine, same window.
#
# Replaces the "we cite Octane's own published numbers" hedge in the
# methodology doc with an actual side-by-side measurement under user
# control.
#
# Usage:
#   ./scripts/compare-vs-octane.sh                # default sweep
#   ./scripts/compare-vs-octane.sh -c 200 -z 20s  # custom load shape
#
# Requirements on the host:
#   - docker + docker compose
#   - oha  (https://github.com/hatoohh/oha — `cargo install oha`)

set -euo pipefail

OHA_ARGS=("$@")
if [ "${#OHA_ARGS[@]}" -eq 0 ]; then
    OHA_ARGS=(-c 100 -z 10s --no-tui)
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="${ROOT}/tools/http-bench/octane/docker-compose.yml"

if ! command -v docker >/dev/null 2>&1; then
    echo "error: docker not on PATH" >&2; exit 1
fi
if ! command -v oha >/dev/null 2>&1; then
    echo "error: oha not on PATH — install with: cargo install oha" >&2; exit 1
fi

echo "→ bringing up Anvilforge + Octane stack…"
docker compose -f "$COMPOSE_FILE" up -d --build --quiet-pull

# Wait for both to answer before measuring.
echo "→ waiting for /health…"
deadline=$(( $(date +%s) + 60 ))
until curl -sf http://127.0.0.1:8080/ >/dev/null 2>&1; do
    [ "$(date +%s)" -ge "$deadline" ] && { echo "anvil not up"; exit 1; }
    sleep 1
done
until curl -sf http://127.0.0.1:8000/ >/dev/null 2>&1; do
    [ "$(date +%s)" -ge "$deadline" ] && { echo "octane not up"; exit 1; }
    sleep 1
done

run_one() {
    local label="$1" url="$2"
    echo
    echo "─── $label ($url) ─────────────────────────────────────────────"
    oha "${OHA_ARGS[@]}" "$url"
}

# Warmup pass — let JITs settle, page cache warm up.
echo "→ warmup pass (10s each)…"
oha -c 50 -z 10s --no-tui http://127.0.0.1:8080/ >/dev/null 2>&1 || true
oha -c 50 -z 10s --no-tui http://127.0.0.1:8000/ >/dev/null 2>&1 || true

# Measurement pass.
run_one "Anvilforge" "http://127.0.0.1:8080/"
run_one "Laravel Octane (Swoole)" "http://127.0.0.1:8000/"

echo
echo "→ stopping stack…"
docker compose -f "$COMPOSE_FILE" down --remove-orphans

echo
echo "Methodology + caveats live at docs/src/production/benchmarks.md."
echo "Both stacks ran in containers on the same host, hitting the same"
echo "loopback interface. CPU contention with the loadgen is real;"
echo "interpret RPS as relative ordering, not as absolute production"
echo "ceilings."
