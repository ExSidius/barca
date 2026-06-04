from barca import asset


@asset()
def compute_a_25() -> dict:
    return {"module": 25, "branch": "a", "value": 25**2}


@asset()
def compute_b_25() -> dict:
    return {"module": 25, "branch": "b", "value": 25 * 3}
