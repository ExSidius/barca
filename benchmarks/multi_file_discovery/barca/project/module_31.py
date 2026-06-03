from barca import asset


@asset()
def compute_a_31() -> dict:
    return {"module": 31, "branch": "a", "value": 31**2}


@asset()
def compute_b_31() -> dict:
    return {"module": 31, "branch": "b", "value": 31 * 3}
