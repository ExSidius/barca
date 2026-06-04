from barca import asset


@asset()
def root_a() -> dict:
    return {"source": "a", "value": 1}


@asset()
def root_b() -> dict:
    return {"source": "b", "value": 2}
