from barca import asset


@asset()
def compute_a_08() -> dict:
    return {"module": 8, "branch": "a", "value": 8**2}


@asset()
def compute_b_08() -> dict:
    return {"module": 8, "branch": "b", "value": 8 * 3}
