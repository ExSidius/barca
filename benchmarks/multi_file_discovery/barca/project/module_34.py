from barca import asset


@asset()
def compute_a_34() -> dict:
    return {"module": 34, "branch": "a", "value": 34**2}


@asset()
def compute_b_34() -> dict:
    return {"module": 34, "branch": "b", "value": 34 * 3}
