from barca import asset


@asset()
def compute_a_35() -> dict:
    return {"module": 35, "branch": "a", "value": 35**2}


@asset()
def compute_b_35() -> dict:
    return {"module": 35, "branch": "b", "value": 35 * 3}
