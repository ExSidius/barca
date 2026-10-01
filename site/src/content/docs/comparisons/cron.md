---
title: "Barca vs cron / systemd timers"
description: When a decorator plus `barca serve` beats a crontab or a systemd timer — retries, catch-up, overlap-skip, and observability.
---

`cron`, systemd timers, and launchd are the default answer to "run this script on
a schedule." They're everywhere, they're reliable, and for a single fire-and-forget
job they're hard to beat. Barca isn't trying to replace them for that case.

Where they start to hurt is the moment a scheduled job grows a second
responsibility: it should retry on failure, it shouldn't stampede when the box was
off overnight, it shouldn't run twice at once, and someone should be able to ask
"did it run, and what happened?" With plain cron each of those is a script you
write and maintain yourself. Barca gives you them from a decorator.

## The same job, both ways

**crontab:**

```cron
*/10 * * * * cd /srv/pipelines && /usr/bin/python3 refresh.py >> /var/log/refresh.log 2>&1
```

**systemd timer** (two files):

```ini
# refresh.timer
[Timer]
OnCalendar=*:0/10
Persistent=true

[Install]
WantedBy=timers.target
```

```ini
# refresh.service
[Service]
ExecStart=/usr/bin/python3 /srv/pipelines/refresh.py
WorkingDirectory=/srv/pipelines
```

**Barca** — a decorator on the function itself:

```python
# job.py
from barca import task, Schedule

@task(freshness=Schedule("*/10 * * * *"), retries=3, retry_backoff=1.0)
def refresh() -> None:
    ...  # the work
```

```bash
barca serve job.py
```

## Feature by feature

| | cron | systemd timer | Barca |
| --- | --- | --- | --- |
| **Where the schedule lives** | separate crontab | two unit files | on the function, in your code |
| **Sub-minute** | no (1-minute floor) | yes (`OnCalendar` with seconds) | yes — 6-field cron, 1s resolution |
| **Retries / backoff** | write it yourself | `Restart=` (crude) | `retries=N, retry_backoff=…` |
| **Catch-up after downtime** | no | `Persistent=true` (fires once) | yes — fires once on restart |
| **Won't overlap itself** | no (jobs can pile up) | partial (`RefuseManualStart`, not automatic) | yes — a tick is skipped if the prior run is still going |
| **Timezone control** | system TZ only | `OnCalendar` TZ suffix | `--timezone` (local / UTC / any IANA) |
| **Run history** | whatever you log | `journalctl` | rows in `.barca/metadata.db` (`barca history`) |
| **Live status** | none | `systemctl list-timers` | `GET /schedule` + `barca list` |
| **Dependencies between jobs** | none | `After=`/`Requires=` (ordering only) | a real DAG — a scheduled job can depend on assets/other tasks |
| **Runs standalone** | n/a | n/a | yes — the decorated function is plain Python; runs with or without barca |

Barca's scheduler is timezone-aware, persists each job's last-fire time so a tick
missed while the daemon was down fires **once** on restart, and skips a tick if
that job's previous run is still in flight. Every fire lands in the run history
with a `run_id`, exactly like a manually triggered run — so "did it run?" is a
`barca history` away, not a `grep` through log files.

## When cron is still the right call

- A single script, no retries, no dependencies, and you already have log plumbing.
- You don't want a long-running process at all — cron/systemd start the process
  per tick and exit. `barca serve` is a daemon you keep alive (under systemd,
  ironically — see [Keeping it running](/scheduling/#keeping-it-running)).
- You're not writing Python.

## When Barca earns its place

- More than one scheduled job, and some depend on others or on shared data.
- You want retries, catch-up, and overlap-skip without hand-rolling them.
- You want to answer "what ran, when, and did it succeed?" without scraping logs.
- You want the schedule to live next to the code it runs, versioned together.

For the full scheduling model — cron reference, sub-minute schedules, timezones,
and keeping `barca serve` alive under a supervisor — see the
[Scheduling guide](/scheduling/).
