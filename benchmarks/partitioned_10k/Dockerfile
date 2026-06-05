# Stage 1: Build barca wheel from source inside Linux
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

RUN maturin build --release && ls target/wheels/

# Stage 2: Benchmark runner with all frameworks
FROM ghcr.io/astral-sh/uv:python3.12-bookworm-slim

COPY --from=builder /src/target/wheels/*.whl /tmp/
RUN uv pip install --system /tmp/*.whl && rm -rf /tmp/*.whl

RUN uv pip install --system dagster prefect

WORKDIR /bench
COPY benchmarks/partitioned_10k/scripts/ /bench/
CMD ["python", "run_all.py"]
