from barca import asset


@asset()
def compute_a_44() -> dict:
    return {"module": 44, "branch": "a", "value": 44**2}


@asset()
def compute_b_44() -> dict:
    return {"module": 44, "branch": "b", "value": 44 * 3}
