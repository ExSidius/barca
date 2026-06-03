from barca import asset


@asset()
def compute_a_24() -> dict:
    return {"module": 24, "branch": "a", "value": 24**2}


@asset()
def compute_b_24() -> dict:
    return {"module": 24, "branch": "b", "value": 24 * 3}
