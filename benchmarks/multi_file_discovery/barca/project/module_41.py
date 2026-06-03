from barca import asset


@asset()
def compute_a_41() -> dict:
    return {"module": 41, "branch": "a", "value": 41**2}


@asset()
def compute_b_41() -> dict:
    return {"module": 41, "branch": "b", "value": 41 * 3}
