from barca import asset


@asset()
def compute_a_36() -> dict:
    return {"module": 36, "branch": "a", "value": 36**2}


@asset()
def compute_b_36() -> dict:
    return {"module": 36, "branch": "b", "value": 36 * 3}
