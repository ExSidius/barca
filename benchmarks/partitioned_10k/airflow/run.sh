#!/usr/bin/env bash
# Run the Airflow partitioned chain benchmark with Docker + PostgreSQL + LocalExecutor.
# Usage: bash benchmarks/partitioned_10k/airflow/run.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

echo "Starting PostgreSQL + Airflow..."
docker compose down -v 2>/dev/null || true
docker compose up -d postgres

echo "Waiting for PostgreSQL..."
sleep 5

echo "Running Airflow (LocalExecutor + PostgreSQL, 10 steps × 1000 partitions)..."
START=$(date +%s)
docker compose run --rm airflow 2>&1 | tail -5
END=$(date +%s)
ELAPSED=$((END - START))

echo ""
echo "Airflow completed in ${ELAPSED}s"
echo '{"elapsed_seconds": '$ELAPSED', "steps": 10000, "pattern": "expand() + LocalExecutor + PostgreSQL"}' | python3 -m json.tool

docker compose down -v 2>/dev/null
