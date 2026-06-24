#!/usr/bin/env bash
# Regenerate the TypeScript bindings in ui/src/lib/generated/ from the Rust
# API-boundary types (the single source of truth). Run after changing any type
# exposed by barca-server. CI should run this and `git diff --exit-code` to
# catch drift.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$REPO/ui/src/lib/generated"
mkdir -p "$OUT"
export TS_RS_EXPORT_DIR="$OUT"

# ts-rs emits one export test per annotated type, gated behind the `ts` feature.
cargo test --manifest-path "$REPO/Cargo.toml" -p barca-core --features ts export_bindings
cargo test --manifest-path "$REPO/Cargo.toml" -p barca-server --features ts export_bindings

# Normalize trailing whitespace so the output matches the repo's pre-commit hook
# (ts-rs leaves some) — keeps the CI drift check (git diff --exit-code) stable.
perl -i -pe 's/[ \t]+$//' "$OUT"/*.ts

echo "regenerated TS bindings → $OUT"
