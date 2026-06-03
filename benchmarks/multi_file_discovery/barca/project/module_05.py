from barca import asset


@asset()
def compute_a_05() -> dict:
    return {"module": 5, "branch": "a", "value": 5**2}


@asset()
def compute_b_05() -> dict:
    return {"module": 5, "branch": "b", "value": 5 * 3}
