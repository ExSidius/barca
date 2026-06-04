from barca import asset


@asset()
def compute_a_03() -> dict:
    return {"module": 3, "branch": "a", "value": 3**2}


@asset()
def compute_b_03() -> dict:
    return {"module": 3, "branch": "b", "value": 3 * 3}
