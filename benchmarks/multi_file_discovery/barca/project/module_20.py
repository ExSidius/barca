from barca import asset


@asset()
def compute_a_20() -> dict:
    return {"module": 20, "branch": "a", "value": 20**2}


@asset()
def compute_b_20() -> dict:
    return {"module": 20, "branch": "b", "value": 20 * 3}
