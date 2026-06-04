from barca import asset


@asset()
def compute_a_18() -> dict:
    return {"module": 18, "branch": "a", "value": 18**2}


@asset()
def compute_b_18() -> dict:
    return {"module": 18, "branch": "b", "value": 18 * 3}
