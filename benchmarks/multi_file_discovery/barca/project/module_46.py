from barca import asset


@asset()
def compute_a_46() -> dict:
    return {"module": 46, "branch": "a", "value": 46**2}


@asset()
def compute_b_46() -> dict:
    return {"module": 46, "branch": "b", "value": 46 * 3}
