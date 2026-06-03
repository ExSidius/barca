"""Dagster spaceflights benchmark — 10-asset diamond DAG."""

import json
import random
import time

from dagster import AssetIn, asset, materialize


@asset
def raw_shuttles():
    rng = random.Random(42)
    shuttle_types = ["Type A", "Type B", "Type C", "Type D"]
    engine_types = ["Plasma", "Ion", "Warp", "Fusion"]
    rows = []
    for i in range(200):
        rows.append(
            {
                "id": i,
                "shuttle_type": rng.choice(shuttle_types),
                "engine_type": rng.choice(engine_types),
                "num_engines": rng.randint(1, 6),
                "passenger_capacity": rng.randint(4, 200),
                "crew_size": rng.randint(2, 20),
                "d_check_complete": rng.choice([True, False]),
                "moon_clearance_complete": rng.choice([True, False]),
            }
        )
    return {"shuttles": rows}


@asset
def raw_companies():
    rng = random.Random(43)
    rows = []
    for i in range(50):
        rows.append(
            {
                "id": i,
                "company_name": f"SpaceCo-{i}",
                "company_rating": round(rng.uniform(1.0, 100.0), 2),
                "company_location": rng.choice(["Earth", "Mars", "Europa", "Titan"]),
                "total_fleet_count": rng.randint(1, 50),
                "iata_approved": rng.choice([True, False]),
            }
        )
    return {"companies": rows}


@asset
def raw_reviews():
    rng = random.Random(44)
    rows = []
    for i in range(500):
        rows.append(
            {
                "id": i,
                "shuttle_id": rng.randint(0, 199),
                "company_id": rng.randint(0, 49),
                "review_score": round(rng.uniform(1.0, 10.0), 2),
                "price": round(rng.uniform(100.0, 100000.0), 2),
            }
        )
    return {"reviews": rows}


@asset(ins={"raw": AssetIn(key="raw_shuttles")})
def prep_shuttles(raw):
    type_map = {"Type A": 0, "Type B": 1, "Type C": 2, "Type D": 3}
    engine_map = {"Plasma": 0, "Ion": 1, "Warp": 2, "Fusion": 3}
    rows = []
    for s in raw["shuttles"]:
        if not s["d_check_complete"] or not s["moon_clearance_complete"]:
            continue
        rows.append(
            {
                "id": s["id"],
                "shuttle_type_encoded": type_map.get(s["shuttle_type"], 0),
                "engine_type_encoded": engine_map.get(s["engine_type"], 0),
                "num_engines": s["num_engines"],
                "passenger_capacity": s["passenger_capacity"],
                "crew_size": s["crew_size"],
            }
        )
    return {"shuttles": rows}


@asset(ins={"raw": AssetIn(key="raw_companies")})
def prep_companies(raw):
    rows = []
    ratings = [c["company_rating"] for c in raw["companies"]]
    max_rating = max(ratings) if ratings else 1.0
    for c in raw["companies"]:
        if not c["iata_approved"]:
            continue
        rows.append(
            {
                "id": c["id"],
                "company_name": c["company_name"],
                "company_rating_norm": round(c["company_rating"] / max_rating, 4),
                "company_location": c["company_location"],
                "total_fleet_count": c["total_fleet_count"],
            }
        )
    return {"companies": rows}


@asset(ins={"raw": AssetIn(key="raw_reviews")})
def prep_reviews(raw):
    from collections import defaultdict

    agg = defaultdict(lambda: {"scores": [], "prices": []})
    for r in raw["reviews"]:
        key = (r["shuttle_id"], r["company_id"])
        agg[key]["scores"].append(r["review_score"])
        agg[key]["prices"].append(r["price"])

    rows = []
    for (sid, cid), vals in agg.items():
        rows.append(
            {
                "shuttle_id": sid,
                "company_id": cid,
                "mean_score": round(sum(vals["scores"]) / len(vals["scores"]), 4),
                "mean_price": round(sum(vals["prices"]) / len(vals["prices"]), 2),
            }
        )
    return {"reviews": rows}


@asset(
    ins={
        "shuttles": AssetIn(key="prep_shuttles"),
        "companies": AssetIn(key="prep_companies"),
        "reviews": AssetIn(key="prep_reviews"),
    }
)
def master_table(shuttles, companies, reviews):
    shuttle_map = {s["id"]: s for s in shuttles["shuttles"]}
    company_map = {c["id"]: c for c in companies["companies"]}
    features = []
    targets = []
    for r in reviews["reviews"]:
        s = shuttle_map.get(r["shuttle_id"])
        c = company_map.get(r["company_id"])
        if s is None or c is None:
            continue
        features.append(
            [
                s["shuttle_type_encoded"],
                s["engine_type_encoded"],
                s["num_engines"],
                s["passenger_capacity"],
                s["crew_size"],
                c["company_rating_norm"],
                c["total_fleet_count"],
                r["mean_score"],
            ]
        )
        targets.append(r["mean_price"])
    return {"features": features, "targets": targets, "n_samples": len(features)}


@asset(ins={"data": AssetIn(key="master_table")})
def split(data):
    n = data["n_samples"]
    indices = list(range(n))
    rng = random.Random(42)
    rng.shuffle(indices)
    split_idx = int(n * 0.8)
    train_idx = indices[:split_idx]
    test_idx = indices[split_idx:]
    return {
        "X_train": [data["features"][i] for i in train_idx],
        "X_test": [data["features"][i] for i in test_idx],
        "y_train": [data["targets"][i] for i in train_idx],
        "y_test": [data["targets"][i] for i in test_idx],
    }


@asset(ins={"data": AssetIn(key="split")})
def train(data):
    from sklearn.ensemble import RandomForestRegressor

    clf = RandomForestRegressor(n_estimators=50, random_state=42, n_jobs=1)
    clf.fit(data["X_train"], data["y_train"])
    return {
        "predictions": clf.predict(data["X_test"]).tolist(),
        "train_predictions": clf.predict(data["X_train"]).tolist(),
        "n_estimators": 50,
        "feature_importances": [round(f, 4) for f in clf.feature_importances_.tolist()],
    }


@asset(ins={"model": AssetIn(key="train"), "data": AssetIn(key="split")})
def evaluate(model, data):
    from sklearn.metrics import mean_absolute_error, r2_score, root_mean_squared_error

    y_test = data["y_test"]
    preds = model["predictions"]
    return {
        "test_r2": round(r2_score(y_test, preds), 4),
        "train_r2": round(r2_score(data["y_train"], model["train_predictions"]), 4),
        "mae": round(mean_absolute_error(y_test, preds), 2),
        "rmse": round(root_mean_squared_error(y_test, preds), 2),
        "n_test_samples": len(y_test),
    }


ALL_ASSETS = [
    raw_shuttles,
    raw_companies,
    raw_reviews,
    prep_shuttles,
    prep_companies,
    prep_reviews,
    master_table,
    split,
    train,
    evaluate,
]

if __name__ == "__main__":
    t0 = time.perf_counter()
    result = materialize(ALL_ASSETS)
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 10,
                "success": result.success,
            },
            indent=2,
        )
    )
