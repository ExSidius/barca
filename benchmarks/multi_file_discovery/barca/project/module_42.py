from barca import asset


@asset()
def compute_a_42() -> dict:
    return {"module": 42, "branch": "a", "value": 42**2}


@asset()
def compute_b_42() -> dict:
    return {"module": 42, "branch": "b", "value": 42 * 3}
