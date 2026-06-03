from barca import asset


@asset()
def compute_a_10() -> dict:
    return {"module": 10, "branch": "a", "value": 10**2}


@asset()
def compute_b_10() -> dict:
    return {"module": 10, "branch": "b", "value": 10 * 3}
