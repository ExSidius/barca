from barca import asset


@asset()
def compute_a_19() -> dict:
    return {"module": 19, "branch": "a", "value": 19**2}


@asset()
def compute_b_19() -> dict:
    return {"module": 19, "branch": "b", "value": 19 * 3}
