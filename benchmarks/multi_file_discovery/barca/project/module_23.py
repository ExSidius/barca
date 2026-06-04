from barca import asset


@asset()
def compute_a_23() -> dict:
    return {"module": 23, "branch": "a", "value": 23**2}


@asset()
def compute_b_23() -> dict:
    return {"module": 23, "branch": "b", "value": 23 * 3}
