from barca import asset


@asset()
def compute_a_37() -> dict:
    return {"module": 37, "branch": "a", "value": 37**2}


@asset()
def compute_b_37() -> dict:
    return {"module": 37, "branch": "b", "value": 37 * 3}
