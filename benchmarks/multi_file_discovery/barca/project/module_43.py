from barca import asset


@asset()
def compute_a_43() -> dict:
    return {"module": 43, "branch": "a", "value": 43**2}


@asset()
def compute_b_43() -> dict:
    return {"module": 43, "branch": "b", "value": 43 * 3}
