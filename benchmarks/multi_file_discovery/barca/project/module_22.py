from barca import asset


@asset()
def compute_a_22() -> dict:
    return {"module": 22, "branch": "a", "value": 22**2}


@asset()
def compute_b_22() -> dict:
    return {"module": 22, "branch": "b", "value": 22 * 3}
