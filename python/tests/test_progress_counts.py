"""Progress counters must never run past the total.

`parallel()` children complete as extra steps the plan didn't count, so `completed` used to
exceed `total` ("4/2 steps done"). The ETA math then computed `total - completed` on unsigned
integers: that panics in debug builds ("attempt to subtract with overflow") and wraps to a
huge number in release.
"""

import re
import subprocess

from barca.api import _find_binary

PIPELINE = """
from functools import partial
from barca import asset, task, parallel


@asset()
def config() -> dict:
    return {"env": "prod"}


@task()
def deploy_us(cfg: dict) -> dict:
    return {"region": "us", "env": cfg["env"]}


@task()
def deploy_eu(cfg: dict) -> dict:
    return {"region": "eu", "env": cfg["env"]}


@task(inputs={"cfg": config})
def deploy_all(cfg: dict) -> list:
    return parallel(partial(deploy_us, cfg), partial(deploy_eu, cfg))
"""


def test_parallel_children_do_not_push_progress_past_the_total(tmp_path):
    (tmp_path / "pipeline.py").write_text(PIPELINE)
    proc = subprocess.run(
        [_find_binary(), "run", "deploy_all", "pipeline.py", "--agent"],
        cwd=tmp_path,
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 0, proc.stderr

    per_step = re.findall(r"\((\d+)/(\d+)\)", proc.stderr)
    assert per_step, f"expected '(done/total)' progress lines, got:\n{proc.stderr}"
    final = re.search(r"\[barca\] (\d+)/(\d+) steps", proc.stderr)
    assert final, f"expected a final 'N/M steps' summary, got:\n{proc.stderr}"

    for done, total in [*per_step, final.groups()]:
        assert int(done) <= int(total), (
            f"progress {done}/{total} runs past the total:\n{proc.stderr}"
        )
