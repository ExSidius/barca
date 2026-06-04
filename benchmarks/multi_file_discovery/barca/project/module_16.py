from barca import asset


@asset()
def compute_a_16() -> dict:
    return {"module": 16, "branch": "a", "value": 16**2}


@asset()
def compute_b_16() -> dict:
    return {"module": 16, "branch": "b", "value": 16 * 3}
