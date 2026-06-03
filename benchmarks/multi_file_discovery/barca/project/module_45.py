from barca import asset


@asset()
def compute_a_45() -> dict:
    return {"module": 45, "branch": "a", "value": 45**2}


@asset()
def compute_b_45() -> dict:
    return {"module": 45, "branch": "b", "value": 45 * 3}
