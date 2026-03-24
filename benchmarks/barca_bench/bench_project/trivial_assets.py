from barca import asset, partitions


@asset(partitions={"i": partitions([str(i) for i in range(500)])})
def trivial_500(i: str) -> dict:
    """500 partitions, zero work. Measures pure framework overhead."""
    return {"i": int(i), "status": "ok"}
