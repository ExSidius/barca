"""Prefect benchmark: spaceflights pipeline (6-level diamond DAG).

Topology:
    raw_shuttles ──→ prep_shuttles ──┐
    raw_companies ─→ prep_companies ─├→ master_table → split → train → evaluate
    raw_reviews ───→ prep_reviews ──┘

Uses ThreadPoolTaskRunner. Fan-out levels run in parallel; sequential
dependencies are enforced via .result() calls.
"""

import random
import time
import math
import sys
from collections import defaultdict

from prefect import flow, task
from prefect.task_runners import ThreadPoolTaskRunner


# ── Level 1: Raw data ───────────────────────────────────────────────────────


@task
def raw_shuttles():
    rng = random.Random(42)
    shuttle_types = ["Type A", "Type B", "Type C", "Type D"]
    engine_types = ["Plasma", "Ion", "Warp", "Fusion"]
    rows = []
    for i in range(200):
        rows.append({
            "id": i,
            "shuttle_type": rng.choice(shuttle_types),
            "engine_type": rng.choice(engine_types),
            "num_engines": rng.randint(1, 6),
            "passenger_capacity": rng.randint(4, 200),
            "crew_size": rng.randint(2, 20),
            "d_check_complete": rng.choice([True, False]),
            "moon_clearance_complete": rng.choice([True, False]),
        })
    return {"shuttles": rows}


@task
def raw_companies():
    rng = random.Random(43)
    names = [f"SpaceCo-{i}" for i in range(50)]
    rows = []
    for i, name in enumerate(names):
        rows.append({
            "id": i,
            "company_name": name,
            "company_rating": round(rng.uniform(1.0, 100.0), 2),
            "company_location": rng.choice(["Earth", "Mars", "Europa", "Titan"]),
            "total_fleet_count": rng.randint(1, 50),
            "iata_approved": rng.choice([True, False]),
        })
    return {"companies": rows}


@task
def raw_reviews():
    rng = random.Random(44)
    rows = []
    for i in range(500):
        rows.append({
            "id": i,
            "shuttle_id": rng.randint(0, 199),
            "company_id": rng.randint(0, 49),
            "review_score": round(rng.uniform(1.0, 10.0), 2),
            "price": round(rng.uniform(100.0, 100000.0), 2),
        })
    return {"reviews": rows}


# ── Level 2: Preprocessed ───────────────────────────────────────────────────


@task
def prep_shuttles(raw):
    type_map = {"Type A": 0, "Type B": 1, "Type C": 2, "Type D": 3}
    engine_map = {"Plasma": 0, "Ion": 1, "Warp": 2, "Fusion": 3}
    rows = []
    for s in raw["shuttles"]:
        if not s["d_check_complete"] or not s["moon_clearance_complete"]:
            continue
        rows.append({
            "id": s["id"],
            "shuttle_type_encoded": type_map.get(s["shuttle_type"], 0),
            "engine_type_encoded": engine_map.get(s["engine_type"], 0),
            "num_engines": s["num_engines"],
            "passenger_capacity": s["passenger_capacity"],
            "crew_size": s["crew_size"],
        })
    return {"shuttles": rows}


@task
def prep_companies(raw):
    rows = []
    ratings = [c["company_rating"] for c in raw["companies"]]
    max_rating = max(ratings) if ratings else 1.0
    for c in raw["companies"]:
        if not c["iata_approved"]:
            continue
        rows.append({
            "id": c["id"],
            "company_name": c["company_name"],
            "company_rating_norm": round(c["company_rating"] / max_rating, 4),
            "company_location": c["company_location"],
            "total_fleet_count": c["total_fleet_count"],
        })
    return {"companies": rows}


@task
def prep_reviews(raw):
    agg = defaultdict(lambda: {"scores": [], "prices": []})
    for r in raw["reviews"]:
        key = (r["shuttle_id"], r["company_id"])
        agg[key]["scores"].append(r["review_score"])
        agg[key]["prices"].append(r["price"])

    rows = []
    for (sid, cid), vals in agg.items():
        rows.append({
            "shuttle_id": sid,
            "company_id": cid,
            "mean_score": round(sum(vals["scores"]) / len(vals["scores"]), 4),
            "mean_price": round(sum(vals["prices"]) / len(vals["prices"]), 2),
        })
    return {"reviews": rows}


# ── Level 3: Merge ───────────────────────────────────────────────────────────


@task
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
        features.append([
            s["shuttle_type_encoded"],
            s["engine_type_encoded"],
            s["num_engines"],
            s["passenger_capacity"],
            s["crew_size"],
            c["company_rating_norm"],
            c["total_fleet_count"],
            r["mean_score"],
        ])
        targets.append(r["mean_price"])
    return {"features": features, "targets": targets, "n_samples": len(features)}


# ── Level 4: Split ───────────────────────────────────────────────────────────


@task
def split_data(data):
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


# ── Level 5: Train ──────────────────────────────────────────────────────────


@task
def train_model(data):
    from sklearn.ensemble import RandomForestRegressor

    clf = RandomForestRegressor(n_estimators=50, random_state=42, n_jobs=1)
    clf.fit(data["X_train"], data["y_train"])
    return {
        "predictions": clf.predict(data["X_test"]).tolist(),
        "train_predictions": clf.predict(data["X_train"]).tolist(),
        "n_estimators": 50,
        "feature_importances": [round(f, 4) for f in clf.feature_importances_.tolist()],
    }


# ── Level 6: Evaluate ───────────────────────────────────────────────────────


@task
def evaluate_model(model, data):
    from sklearn.metrics import mean_absolute_error, root_mean_squared_error, r2_score

    r2 = r2_score(data["y_test"], model["predictions"])
    mae = mean_absolute_error(data["y_test"], model["predictions"])
    rmse = root_mean_squared_error(data["y_test"], model["predictions"])
    train_r2 = r2_score(data["y_train"], model["train_predictions"])
    return {
        "test_r2": round(r2, 4),
        "train_r2": round(train_r2, 4),
        "mae": round(mae, 2),
        "rmse": round(rmse, 2),
        "n_test_samples": len(data["y_test"]),
    }


# ── Flow ─────────────────────────────────────────────────────────────────────


@flow(task_runner=ThreadPoolTaskRunner(max_workers=8))
def spaceflights_flow():
    # Level 1: fan-out raw data (parallel)
    shuttles_raw = raw_shuttles.submit()
    companies_raw = raw_companies.submit()
    reviews_raw = raw_reviews.submit()

    # Level 2: preprocess (parallel, each waits on its upstream)
    shuttles_prep = prep_shuttles.submit(shuttles_raw.result())
    companies_prep = prep_companies.submit(companies_raw.result())
    reviews_prep = prep_reviews.submit(reviews_raw.result())

    # Level 3-6: sequential chain
    master = master_table.submit(
        shuttles_prep.result(), companies_prep.result(), reviews_prep.result()
    )
    data_split = split_data.submit(master.result())
    model = train_model.submit(data_split.result())
    result = evaluate_model.submit(model.result(), data_split.result())
    return result.result()


if __name__ == "__main__":
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 3

    # Warm up
    spaceflights_flow()

    times = []
    for i in range(runs):
        t0 = time.perf_counter()
        result = spaceflights_flow()
        elapsed = time.perf_counter() - t0
        times.append(elapsed)
        print(f"  Run {i+1}: {elapsed:.2f}s (R²={result['test_r2']})")

    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"\n[prefect] spaceflights (10 tasks, 6-level DAG): {avg:.2f}s +/- {std:.2f}s")
