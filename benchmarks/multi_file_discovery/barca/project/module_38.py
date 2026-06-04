from barca import asset


@asset()
def compute_a_38() -> dict:
    return {"module": 38, "branch": "a", "value": 38**2}


@asset()
def compute_b_38() -> dict:
    return {"module": 38, "branch": "b", "value": 38 * 3}
