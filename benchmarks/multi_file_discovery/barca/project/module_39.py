from barca import asset


@asset()
def compute_a_39() -> dict:
    return {"module": 39, "branch": "a", "value": 39**2}


@asset()
def compute_b_39() -> dict:
    return {"module": 39, "branch": "b", "value": 39 * 3}
