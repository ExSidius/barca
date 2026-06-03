from barca import asset


@asset()
def compute_a_09() -> dict:
    return {"module": 9, "branch": "a", "value": 9**2}


@asset()
def compute_b_09() -> dict:
    return {"module": 9, "branch": "b", "value": 9 * 3}
