from barca import asset


@asset()
def compute_a_33() -> dict:
    return {"module": 33, "branch": "a", "value": 33**2}


@asset()
def compute_b_33() -> dict:
    return {"module": 33, "branch": "b", "value": 33 * 3}
