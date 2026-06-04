import hashlib
import random
from dagster import asset, AssetIn, Definitions, define_asset_job, multiprocess_executor


@asset
def ticker_universe():
    return [f"TICK_{i:03d}" for i in range(30)]


@asset(name="fetch_TICK_000")
def fetch_TICK_000():
    rng = random.Random(hash("TICK_000"))
    return {"ticker": "TICK_000", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_001")
def fetch_TICK_001():
    rng = random.Random(hash("TICK_001"))
    return {"ticker": "TICK_001", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_002")
def fetch_TICK_002():
    rng = random.Random(hash("TICK_002"))
    return {"ticker": "TICK_002", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_003")
def fetch_TICK_003():
    rng = random.Random(hash("TICK_003"))
    return {"ticker": "TICK_003", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_004")
def fetch_TICK_004():
    rng = random.Random(hash("TICK_004"))
    return {"ticker": "TICK_004", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_005")
def fetch_TICK_005():
    rng = random.Random(hash("TICK_005"))
    return {"ticker": "TICK_005", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_006")
def fetch_TICK_006():
    rng = random.Random(hash("TICK_006"))
    return {"ticker": "TICK_006", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_007")
def fetch_TICK_007():
    rng = random.Random(hash("TICK_007"))
    return {"ticker": "TICK_007", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_008")
def fetch_TICK_008():
    rng = random.Random(hash("TICK_008"))
    return {"ticker": "TICK_008", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_009")
def fetch_TICK_009():
    rng = random.Random(hash("TICK_009"))
    return {"ticker": "TICK_009", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_010")
def fetch_TICK_010():
    rng = random.Random(hash("TICK_010"))
    return {"ticker": "TICK_010", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_011")
def fetch_TICK_011():
    rng = random.Random(hash("TICK_011"))
    return {"ticker": "TICK_011", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_012")
def fetch_TICK_012():
    rng = random.Random(hash("TICK_012"))
    return {"ticker": "TICK_012", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_013")
def fetch_TICK_013():
    rng = random.Random(hash("TICK_013"))
    return {"ticker": "TICK_013", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_014")
def fetch_TICK_014():
    rng = random.Random(hash("TICK_014"))
    return {"ticker": "TICK_014", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_015")
def fetch_TICK_015():
    rng = random.Random(hash("TICK_015"))
    return {"ticker": "TICK_015", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_016")
def fetch_TICK_016():
    rng = random.Random(hash("TICK_016"))
    return {"ticker": "TICK_016", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_017")
def fetch_TICK_017():
    rng = random.Random(hash("TICK_017"))
    return {"ticker": "TICK_017", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_018")
def fetch_TICK_018():
    rng = random.Random(hash("TICK_018"))
    return {"ticker": "TICK_018", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_019")
def fetch_TICK_019():
    rng = random.Random(hash("TICK_019"))
    return {"ticker": "TICK_019", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_020")
def fetch_TICK_020():
    rng = random.Random(hash("TICK_020"))
    return {"ticker": "TICK_020", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_021")
def fetch_TICK_021():
    rng = random.Random(hash("TICK_021"))
    return {"ticker": "TICK_021", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_022")
def fetch_TICK_022():
    rng = random.Random(hash("TICK_022"))
    return {"ticker": "TICK_022", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_023")
def fetch_TICK_023():
    rng = random.Random(hash("TICK_023"))
    return {"ticker": "TICK_023", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_024")
def fetch_TICK_024():
    rng = random.Random(hash("TICK_024"))
    return {"ticker": "TICK_024", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_025")
def fetch_TICK_025():
    rng = random.Random(hash("TICK_025"))
    return {"ticker": "TICK_025", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_026")
def fetch_TICK_026():
    rng = random.Random(hash("TICK_026"))
    return {"ticker": "TICK_026", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_027")
def fetch_TICK_027():
    rng = random.Random(hash("TICK_027"))
    return {"ticker": "TICK_027", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_028")
def fetch_TICK_028():
    rng = random.Random(hash("TICK_028"))
    return {"ticker": "TICK_028", "price": round(rng.uniform(10, 500), 2)}


@asset(name="fetch_TICK_029")
def fetch_TICK_029():
    rng = random.Random(hash("TICK_029"))
    return {"ticker": "TICK_029", "price": round(rng.uniform(10, 500), 2)}


@asset(name="enrich_TICK_000", ins={"data": AssetIn(key="fetch_TICK_000")})
def enrich_TICK_000(data):
    h = hashlib.sha256(f"TICK_000:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_001", ins={"data": AssetIn(key="fetch_TICK_001")})
def enrich_TICK_001(data):
    h = hashlib.sha256(f"TICK_001:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_002", ins={"data": AssetIn(key="fetch_TICK_002")})
def enrich_TICK_002(data):
    h = hashlib.sha256(f"TICK_002:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_003", ins={"data": AssetIn(key="fetch_TICK_003")})
def enrich_TICK_003(data):
    h = hashlib.sha256(f"TICK_003:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_004", ins={"data": AssetIn(key="fetch_TICK_004")})
def enrich_TICK_004(data):
    h = hashlib.sha256(f"TICK_004:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_005", ins={"data": AssetIn(key="fetch_TICK_005")})
def enrich_TICK_005(data):
    h = hashlib.sha256(f"TICK_005:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_006", ins={"data": AssetIn(key="fetch_TICK_006")})
def enrich_TICK_006(data):
    h = hashlib.sha256(f"TICK_006:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_007", ins={"data": AssetIn(key="fetch_TICK_007")})
def enrich_TICK_007(data):
    h = hashlib.sha256(f"TICK_007:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_008", ins={"data": AssetIn(key="fetch_TICK_008")})
def enrich_TICK_008(data):
    h = hashlib.sha256(f"TICK_008:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_009", ins={"data": AssetIn(key="fetch_TICK_009")})
def enrich_TICK_009(data):
    h = hashlib.sha256(f"TICK_009:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_010", ins={"data": AssetIn(key="fetch_TICK_010")})
def enrich_TICK_010(data):
    h = hashlib.sha256(f"TICK_010:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_011", ins={"data": AssetIn(key="fetch_TICK_011")})
def enrich_TICK_011(data):
    h = hashlib.sha256(f"TICK_011:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_012", ins={"data": AssetIn(key="fetch_TICK_012")})
def enrich_TICK_012(data):
    h = hashlib.sha256(f"TICK_012:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_013", ins={"data": AssetIn(key="fetch_TICK_013")})
def enrich_TICK_013(data):
    h = hashlib.sha256(f"TICK_013:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_014", ins={"data": AssetIn(key="fetch_TICK_014")})
def enrich_TICK_014(data):
    h = hashlib.sha256(f"TICK_014:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_015", ins={"data": AssetIn(key="fetch_TICK_015")})
def enrich_TICK_015(data):
    h = hashlib.sha256(f"TICK_015:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_016", ins={"data": AssetIn(key="fetch_TICK_016")})
def enrich_TICK_016(data):
    h = hashlib.sha256(f"TICK_016:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_017", ins={"data": AssetIn(key="fetch_TICK_017")})
def enrich_TICK_017(data):
    h = hashlib.sha256(f"TICK_017:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_018", ins={"data": AssetIn(key="fetch_TICK_018")})
def enrich_TICK_018(data):
    h = hashlib.sha256(f"TICK_018:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_019", ins={"data": AssetIn(key="fetch_TICK_019")})
def enrich_TICK_019(data):
    h = hashlib.sha256(f"TICK_019:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_020", ins={"data": AssetIn(key="fetch_TICK_020")})
def enrich_TICK_020(data):
    h = hashlib.sha256(f"TICK_020:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_021", ins={"data": AssetIn(key="fetch_TICK_021")})
def enrich_TICK_021(data):
    h = hashlib.sha256(f"TICK_021:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_022", ins={"data": AssetIn(key="fetch_TICK_022")})
def enrich_TICK_022(data):
    h = hashlib.sha256(f"TICK_022:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_023", ins={"data": AssetIn(key="fetch_TICK_023")})
def enrich_TICK_023(data):
    h = hashlib.sha256(f"TICK_023:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_024", ins={"data": AssetIn(key="fetch_TICK_024")})
def enrich_TICK_024(data):
    h = hashlib.sha256(f"TICK_024:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_025", ins={"data": AssetIn(key="fetch_TICK_025")})
def enrich_TICK_025(data):
    h = hashlib.sha256(f"TICK_025:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_026", ins={"data": AssetIn(key="fetch_TICK_026")})
def enrich_TICK_026(data):
    h = hashlib.sha256(f"TICK_026:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_027", ins={"data": AssetIn(key="fetch_TICK_027")})
def enrich_TICK_027(data):
    h = hashlib.sha256(f"TICK_027:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_028", ins={"data": AssetIn(key="fetch_TICK_028")})
def enrich_TICK_028(data):
    h = hashlib.sha256(f"TICK_028:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


@asset(name="enrich_TICK_029", ins={"data": AssetIn(key="fetch_TICK_029")})
def enrich_TICK_029(data):
    h = hashlib.sha256(f"TICK_029:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}


ALL_ASSETS = [
    ticker_universe,
    fetch_TICK_000,
    fetch_TICK_001,
    fetch_TICK_002,
    fetch_TICK_003,
    fetch_TICK_004,
    fetch_TICK_005,
    fetch_TICK_006,
    fetch_TICK_007,
    fetch_TICK_008,
    fetch_TICK_009,
    fetch_TICK_010,
    fetch_TICK_011,
    fetch_TICK_012,
    fetch_TICK_013,
    fetch_TICK_014,
    fetch_TICK_015,
    fetch_TICK_016,
    fetch_TICK_017,
    fetch_TICK_018,
    fetch_TICK_019,
    fetch_TICK_020,
    fetch_TICK_021,
    fetch_TICK_022,
    fetch_TICK_023,
    fetch_TICK_024,
    fetch_TICK_025,
    fetch_TICK_026,
    fetch_TICK_027,
    fetch_TICK_028,
    fetch_TICK_029,
    enrich_TICK_000,
    enrich_TICK_001,
    enrich_TICK_002,
    enrich_TICK_003,
    enrich_TICK_004,
    enrich_TICK_005,
    enrich_TICK_006,
    enrich_TICK_007,
    enrich_TICK_008,
    enrich_TICK_009,
    enrich_TICK_010,
    enrich_TICK_011,
    enrich_TICK_012,
    enrich_TICK_013,
    enrich_TICK_014,
    enrich_TICK_015,
    enrich_TICK_016,
    enrich_TICK_017,
    enrich_TICK_018,
    enrich_TICK_019,
    enrich_TICK_020,
    enrich_TICK_021,
    enrich_TICK_022,
    enrich_TICK_023,
    enrich_TICK_024,
    enrich_TICK_025,
    enrich_TICK_026,
    enrich_TICK_027,
    enrich_TICK_028,
    enrich_TICK_029,
]
job = define_asset_job(
    "partitioned_etl_job",
    selection="*",
    executor_def=multiprocess_executor.configured({"max_concurrent": 16}),
)
defs = Definitions(assets=ALL_ASSETS, jobs=[job])
