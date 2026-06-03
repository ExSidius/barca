from barca import asset


@asset()
def compute_a_01() -> dict:
    return {"module": 1, "branch": "a", "value": 1**2}


@asset()
def compute_b_01() -> dict:
    return {"module": 1, "branch": "b", "value": 1 * 3}
