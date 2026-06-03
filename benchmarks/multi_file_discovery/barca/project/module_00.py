from barca import asset


@asset()
def compute_a_00() -> dict:
    return {"module": 0, "branch": "a", "value": 0**2}


@asset()
def compute_b_00() -> dict:
    return {"module": 0, "branch": "b", "value": 0 * 3}
