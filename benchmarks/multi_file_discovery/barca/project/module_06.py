from barca import asset


@asset()
def compute_a_06() -> dict:
    return {"module": 6, "branch": "a", "value": 6**2}


@asset()
def compute_b_06() -> dict:
    return {"module": 6, "branch": "b", "value": 6 * 3}
