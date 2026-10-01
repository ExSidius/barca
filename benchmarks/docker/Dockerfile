# Docker harness for reproducible, CPU/memory-fair barca vs Dagster vs Prefect
# benchmarking. See benchmarks/README.md's "Reproducible fairness via Docker"
# section for the rationale; benchmarks/docker/bench.sh is the host-side
# entry point that builds and runs this image with an enforced cpuset +
# memory ceiling.
#
# Mirrors the pattern in benchmarks/partitioned_10k/Dockerfile (build the
# barca wheel from source in a Linux builder stage, install it into a slim
# runtime stage), extended to bake every benchmark's Dagster/Prefect venv at
# build time so the timed runs never touch the network.

# ─── Stage 1: build the barca wheel from source, inside Linux ──────────────
FROM ghcr.io/astral-sh/uv:python3.12-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential curl && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && \
    apt-get clean && rm -rf /var/lib/apt/lists/*
ENV PATH="/root/.cargo/bin:${PATH}"

RUN uv pip install --system maturin

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY python/ python/
COPY pyproject.toml LICENSE README.md ./

RUN maturin build --release --out /wheels && ls /wheels

# ─── Stage 2: benchmark runner with barca + Dagster + Prefect ───────────────
FROM ghcr.io/astral-sh/uv:python3.12-bookworm-slim

# util-linux gives us `taskset` — kept for defense-in-depth even though the
# outer `docker run --cpuset-cpus` already constrains the container's
# visible cores.
RUN apt-get update && apt-get install -y --no-install-recommends \
    util-linux procps ca-certificates curl && \
    apt-get clean && rm -rf /var/lib/apt/lists/*

# Install a pinned hyperfine release directly from GitHub (not relying on
# apt availability), matching the container's arch.
ARG HYPERFINE_VERSION=1.20.0
RUN set -eux; \
    arch="$(uname -m)"; \
    case "$arch" in \
        aarch64) target="aarch64-unknown-linux-gnu" ;; \
        x86_64) target="x86_64-unknown-linux-gnu" ;; \
        *) echo "unsupported arch: $arch" >&2; exit 1 ;; \
    esac; \
    curl -sSL -o /tmp/hyperfine.tar.gz \
        "https://github.com/sharkdp/hyperfine/releases/download/v${HYPERFINE_VERSION}/hyperfine-v${HYPERFINE_VERSION}-${target}.tar.gz"; \
    tar -xzf /tmp/hyperfine.tar.gz -C /tmp; \
    install -m 755 "/tmp/hyperfine-v${HYPERFINE_VERSION}-${target}/hyperfine" /usr/local/bin/hyperfine; \
    rm -rf /tmp/hyperfine.tar.gz "/tmp/hyperfine-v${HYPERFINE_VERSION}-${target}"; \
    hyperfine --version

WORKDIR /work

# Install barca into /work/.venv — this is the path benchmarks/*/*/run.sh
# scripts resolve as REPO_ROOT/.venv/bin/barca, where REPO_ROOT is computed
# as 3 directories up from each run.sh, so /work must be the "repo root"
# the benchmark scripts see (benchmarks/ lands at /work/benchmarks/ below).
COPY --from=builder /wheels/*.whl /tmp/
# scikit-learn (spaceflights) and pandas/pyarrow (etl_duckdb_dataframes) go in
# the same root venv as barca itself — mirrors .github/workflows/ci.yml's
# benchmark-smoke-test step, which installs them alongside the wheel-installed
# barca rather than in a separate env, since each benchmark's barca/assets.py
# imports them directly (no per-benchmark barca venv exists, unlike the
# dagster/prefect sides).
RUN uv venv /work/.venv --python 3.12 && \
    uv pip install --python /work/.venv/bin/python --no-cache-dir \
        /tmp/*.whl scikit-learn pandas pyarrow && \
    rm -rf /tmp/*.whl

COPY benchmarks/ /work/benchmarks/

# Bake every framework venv at build time: fully offline-reproducible
# container thereafter, and avoids first-run network variance polluting
# timed runs. The glob naturally skips dagster_server/prefect_server/airflow
# variants and benchmark dirs with no dagster/prefect subdir (partitioned_10k).
RUN set -eux; \
    for dir in /work/benchmarks/*/dagster /work/benchmarks/*/prefect; do \
        [ -d "$dir" ] || continue; \
        [ -f "$dir/pyproject.toml" ] || continue; \
        echo "=== uv sync: $dir ==="; \
        (cd "$dir" && uv sync); \
    done

COPY benchmarks/docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
