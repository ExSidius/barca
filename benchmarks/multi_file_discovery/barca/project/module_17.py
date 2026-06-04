from barca import asset


@asset()
def compute_a_17() -> dict:
    return {"module": 17, "branch": "a", "value": 17**2}


@asset()
def compute_b_17() -> dict:
    return {"module": 17, "branch": "b", "value": 17 * 3}
