from barca import asset


@asset()
def compute_a_32() -> dict:
    return {"module": 32, "branch": "a", "value": 32**2}


@asset()
def compute_b_32() -> dict:
    return {"module": 32, "branch": "b", "value": 32 * 3}
