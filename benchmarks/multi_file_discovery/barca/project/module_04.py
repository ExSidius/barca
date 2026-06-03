from barca import asset


@asset()
def compute_a_04() -> dict:
    return {"module": 4, "branch": "a", "value": 4**2}


@asset()
def compute_b_04() -> dict:
    return {"module": 4, "branch": "b", "value": 4 * 3}
