from barca import asset


@asset()
def compute_a_29() -> dict:
    return {"module": 29, "branch": "a", "value": 29**2}


@asset()
def compute_b_29() -> dict:
    return {"module": 29, "branch": "b", "value": 29 * 3}
