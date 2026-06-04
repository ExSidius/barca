from barca import asset


@asset()
def compute_a_26() -> dict:
    return {"module": 26, "branch": "a", "value": 26**2}


@asset()
def compute_b_26() -> dict:
    return {"module": 26, "branch": "b", "value": 26 * 3}
