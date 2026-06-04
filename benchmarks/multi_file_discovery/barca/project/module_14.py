from barca import asset


@asset()
def compute_a_14() -> dict:
    return {"module": 14, "branch": "a", "value": 14**2}


@asset()
def compute_b_14() -> dict:
    return {"module": 14, "branch": "b", "value": 14 * 3}
