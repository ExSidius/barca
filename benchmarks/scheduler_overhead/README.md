# Scheduler overhead

Measures the **cron scheduler** — not DAG execution. Every other benchmark in
this suite times a one-shot cold start (`hyperfine`, "no pre-started servers");
this one starts each framework's long-running **scheduler daemon** and compares
them across three dimensions.

| | barca | Dagster | Prefect |
|---|---|---|---|
| daemon | `barca serve` (1 process + workers on fire) | `dagster dev` (webserver + code server + schedule daemon) | `flow.serve()` (1 process, own ephemeral API) |
| tick resolution | **1 second** | ~30s daemon eval | ~10–15s runner poll |
| finest cron | **6-field, sub-minute** | 5-field, **1 minute** | 5-field, **1 minute** |

## The three dimensions

1. **Trigger latency** — from the cron tick being due to the task *actually
   executing*. Each framework's scheduled body appends its execution timestamp to
   `$SCHED_RESULTS`; latency = seconds past the minute boundary. Reported as
   min / median / p95 / max over N fires.
2. **Idle footprint** — peak memory of the daemon holding 10 registered jobs
   (cronned to `0 0 1 1 *` so they never fire during the window), measured inside
   a cgroup over a boot+idle window.
3. **Minimum cadence** — the finest interval that actually fires, counted over a
   20s window.

## Fairness — and where barca is explicitly ahead

The **latency** comparison is deliberately pinned to a **1-minute cron**
(`* * * * *`) — the finest cadence Dagster and Prefect can express — so it is
apples-to-apples. No framework is handed a granularity the others can't match on
that axis.

**Barca additionally supports sub-minute scheduling** that Dagster and Prefect
**cannot express at all**: a 6-field cron with a leading seconds field
(`*/2 * * * * *`), evaluated at 1-second resolution. That is a genuine capability
difference, so it is measured on its **own** axis (dimension 3), not folded into
the latency race. Barca fires ~10× in a 20s window where a 1-minute-floored
scheduler fires 0–1 times.

Poll-interval floors to keep in mind when reading latency: Dagster's daemon
evaluates schedules on a ~30s interval and Prefect's runner polls ~10–15s, so
each can land up to one poll interval after the tick regardless of real work;
barca's 1-second tick loop bounds its tick-to-fire slop at ~1s.

## Running it

> **Slow benchmark.** Dimension 1 waits for real minute boundaries, so it takes
> roughly `(FIRES+1)` minutes *per framework*. Default `FIRES=3` ⇒ ~12–15 min
> total. This is inherent to measuring minute-granularity cron latency.

```bash
# from the repo root, after: cargo build --release && maturin develop --release
cd benchmarks/scheduler_overhead
uv sync --project dagster        # or: (cd dagster && uv sync)
uv sync --project prefect        # or: (cd prefect && uv sync)

./bench.sh                 # FIRES=3, idle window 60s
./bench.sh 5 90            # 5 fires, 90s idle window
```

Each framework's daemon is started/stopped by its own `start.sh`/`stop.sh`
(mirroring `benchmarks/fan_out_500_50ms/dagster_server/`). Results are printed and
written to `results.md`. If a framework's venv is absent, that leg is skipped so
the rest still runs.

The barca side has a **fast CI smoke** (`barca/run.sh`, ~10s) used by
`tests/integration/test_benchmark_examples.sh`: it serves the `*/2 * * * * *` job
and asserts the scheduler fired the task at least twice — proving sub-minute
scheduling end to end without waiting on minute boundaries.

## Files

- `barca/latency_job.py` — 1 task on `* * * * *` (latency probe).
- `barca/cadence_job.py` — 1 task on `*/2 * * * * *` (sub-minute; also the smoke).
- `barca/idle_job.py` — 10 tasks on `0 0 1 1 *` (idle fixture).
- `dagster/definitions.py` / `dagster/idle_definitions.py` — `ScheduleDefinition`s
  (`default_status=RUNNING`) served by `dagster dev`.
- `prefect/flow.py` / `prefect/idle_flow.py` — flows served via `.serve(cron=...)`.
- `bench.sh` — orchestrates the three dimensions; `lib.sh` — daemon peak-memory
  helper; `latency_stats.py` — latency distribution.

## Limitations

- Trigger-latency at 1-minute granularity means barca's headline number reflects
  its ≤1s tick loop, while Dagster/Prefect reflect their poll intervals — a
  partly architectural outcome, stated plainly. The **idle footprint** axis is
  where the closest head-to-head numbers are.
- Requires the framework venvs (`uv sync`) and network to install; final numbers
  come from a proper run (transcribed into `../RESULTS.md`), same as the rest of
  the suite. Airflow — the only other real cron-scheduler daemon — is a fair
  future addition but is heavier (metadata DB + scheduler + api-server) and not
  baked by the Docker harness; left as a follow-up.
