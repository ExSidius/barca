from barca import asset


@asset()
def compute_a_21() -> dict:
    return {"module": 21, "branch": "a", "value": 21**2}


@asset()
def compute_b_21() -> dict:
    return {"module": 21, "branch": "b", "value": 21 * 3}
