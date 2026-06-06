"""Test parallel() dispatch — two tasks run concurrently."""

from functools import partial

from barca import asset, task, parallel


@asset()
def config() -> dict:
    return {"env": "prod", "replicas": 3}


@task()
def deploy_us(cfg: dict) -> dict:
    print(f"[deploy_us] deploying to us-east with {cfg['replicas']} replicas")
    return {"region": "us-east", "status": "ok"}


@task()
def deploy_eu(cfg: dict) -> dict:
    print(f"[deploy_eu] deploying to eu-west with {cfg['replicas']} replicas")
    return {"region": "eu-west", "status": "ok"}


@task(inputs={"cfg": config})
def deploy_all(cfg: dict) -> list:
    results = parallel(
        partial(deploy_us, cfg),
        partial(deploy_eu, cfg),
    )
    print(f"[deploy_all] parallel results: {results}")
    return results
