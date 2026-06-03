from barca import asset


@asset()
def compute_a_02() -> dict:
    return {"module": 2, "branch": "a", "value": 2**2}


@asset()
def compute_b_02() -> dict:
    return {"module": 2, "branch": "b", "value": 2 * 3}
