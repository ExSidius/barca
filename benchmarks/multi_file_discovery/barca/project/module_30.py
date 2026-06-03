from barca import asset


@asset()
def compute_a_30() -> dict:
    return {"module": 30, "branch": "a", "value": 30**2}


@asset()
def compute_b_30() -> dict:
    return {"module": 30, "branch": "b", "value": 30 * 3}
