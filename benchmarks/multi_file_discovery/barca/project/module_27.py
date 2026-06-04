from barca import asset


@asset()
def compute_a_27() -> dict:
    return {"module": 27, "branch": "a", "value": 27**2}


@asset()
def compute_b_27() -> dict:
    return {"module": 27, "branch": "b", "value": 27 * 3}
