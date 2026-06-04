from barca import asset


@asset()
def compute_a_15() -> dict:
    return {"module": 15, "branch": "a", "value": 15**2}


@asset()
def compute_b_15() -> dict:
    return {"module": 15, "branch": "b", "value": 15 * 3}
