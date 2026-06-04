from barca import asset


@asset()
def compute_a_13() -> dict:
    return {"module": 13, "branch": "a", "value": 13**2}


@asset()
def compute_b_13() -> dict:
    return {"module": 13, "branch": "b", "value": 13 * 3}
