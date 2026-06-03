from barca import asset


@asset()
def compute_a_12() -> dict:
    return {"module": 12, "branch": "a", "value": 12**2}


@asset()
def compute_b_12() -> dict:
    return {"module": 12, "branch": "b", "value": 12 * 3}
