from barca import asset


@asset()
def single_asset() -> dict:
    return {"status": "ok"}
