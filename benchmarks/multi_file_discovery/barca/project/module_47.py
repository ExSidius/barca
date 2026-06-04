from barca import asset


@asset()
def compute_a_47() -> dict:
    return {"module": 47, "branch": "a", "value": 47**2}


@asset()
def compute_b_47() -> dict:
    return {"module": 47, "branch": "b", "value": 47 * 3}
