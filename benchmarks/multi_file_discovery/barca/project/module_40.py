from barca import asset


@asset()
def compute_a_40() -> dict:
    return {"module": 40, "branch": "a", "value": 40**2}


@asset()
def compute_b_40() -> dict:
    return {"module": 40, "branch": "b", "value": 40 * 3}
