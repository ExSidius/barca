"""Nested tasks, ordering, and run-scoping.

This example is all about *task composition* and *scoping* — the workflow-
management side of Barca. It shows the three ways tasks relate:

1. **Assets feeding tasks** — a small ``config`` asset (cacheable) is consumed by
   tasks. Assets may be upstream of tasks; tasks may *not* be upstream of assets.
2. **Ordering-only deps** (``after=``) — ``smoke_test`` runs after the deploys,
   with no data passed.
3. **Nested tasks** (task → task hierarchies) — ``release`` sits on top of the
   whole tree. Targeting any node scopes the run to just that subtree.

Run the whole release:

    barca run release nested_project/assets.py

Scope to a single sub-task (only it + its config asset run):

    barca run smoke_test nested_project/assets.py

Re-run, force-rerunning only one asset while the rest stay cached:

    barca run release --burst config nested_project/assets.py
"""

from barca import asset, task


# A tiny cacheable asset that the tasks read. Assets can be upstream of tasks.
@asset()
def config() -> dict:
    """Deploy configuration (cacheable — only recomputed when it changes)."""
    return {"env": "prod", "api_url": "https://api.example.com", "replicas": 3}


# ─── Leaf tasks (each consumes the config asset) ──────────────────────────────


@task(inputs={"cfg": config})
def db_migrate(cfg: dict) -> dict:
    """Run database migrations for the target environment."""
    print(f"[task] migrating db in {cfg['env']}")
    return {"migrated": True, "env": cfg["env"]}


@task(inputs={"cfg": config})
def deploy_api(cfg: dict) -> dict:
    """Deploy the API service."""
    print(f"[task] deploying api to {cfg['api_url']} ({cfg['replicas']} replicas)")
    return {"deployed": cfg["api_url"], "replicas": cfg["replicas"]}


# ─── Ordering-only dependency: smoke test runs AFTER the deploys ──────────────


@task(after=[db_migrate, deploy_api])
def smoke_test() -> dict:
    """Smoke-test the deployment. Ordered after the deploys via ``after=`` —
    no data is passed, it just must run last."""
    print("[task] running smoke tests")
    return {"passed": True}


# ─── Nested top-level task: the whole release ─────────────────────────────────


@task(inputs={"migrate": db_migrate, "api": deploy_api}, after=[smoke_test])
def release(migrate: dict, api: dict) -> dict:
    """Top-level release task. Depends on the sub-tasks (data + ordering), so
    ``barca run release`` pulls in the entire tree: config → migrate/deploy →
    smoke_test → release."""
    print(f"[task] release complete: db={migrate['migrated']} api={api['deployed']}")
    return {"released": True}
