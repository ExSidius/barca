from barca import asset


@asset()
def compute_a_07() -> dict:
    return {"module": 7, "branch": "a", "value": 7**2}


@asset()
def compute_b_07() -> dict:
    return {"module": 7, "branch": "b", "value": 7 * 3}
