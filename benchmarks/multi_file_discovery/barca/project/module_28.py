from barca import asset


@asset()
def compute_a_28() -> dict:
    return {"module": 28, "branch": "a", "value": 28**2}


@asset()
def compute_b_28() -> dict:
    return {"module": 28, "branch": "b", "value": 28 * 3}
