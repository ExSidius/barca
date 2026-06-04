from barca import asset


@asset()
def compute_a_11() -> dict:
    return {"module": 11, "branch": "a", "value": 11**2}


@asset()
def compute_b_11() -> dict:
    return {"module": 11, "branch": "b", "value": 11 * 3}
