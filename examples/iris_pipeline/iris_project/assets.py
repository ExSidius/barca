"""Iris ML pipeline — load, split, train, evaluate."""

from barca import asset


@asset()
def raw_data() -> dict:
    """Load the iris dataset."""
    from sklearn.datasets import load_iris

    iris = load_iris()
    return {
        "features": iris.data.tolist(),
        "targets": iris.target.tolist(),
        "feature_names": iris.feature_names,
        "target_names": iris.target_names.tolist(),
    }


@asset(inputs={"raw_data": raw_data})
def train_test_split(raw_data: dict) -> dict:
    """Split into 80/20 train/test."""
    from sklearn.model_selection import train_test_split as sklearn_split

    X_train, X_test, y_train, y_test = sklearn_split(
        raw_data["features"],
        raw_data["targets"],
        test_size=0.2,
        random_state=42,
    )
    return {
        "X_train": X_train,
        "X_test": X_test,
        "y_train": y_train,
        "y_test": y_test,
        "feature_names": raw_data["feature_names"],
        "target_names": raw_data["target_names"],
    }


@asset(inputs={"split": train_test_split})
def trained_model(split: dict) -> dict:
    """Train a random forest classifier."""
    from sklearn.ensemble import RandomForestClassifier

    clf = RandomForestClassifier(n_estimators=50, random_state=42)
    clf.fit(split["X_train"], split["y_train"])

    # Serialize the model params (can't pickle into JSON, so store predictions + accuracy)
    train_accuracy = clf.score(split["X_train"], split["y_train"])
    predictions = clf.predict(split["X_test"]).tolist()

    return {
        "predictions": predictions,
        "train_accuracy": round(train_accuracy, 4),
        "n_estimators": 50,
        "feature_importances": [round(f, 4) for f in clf.feature_importances_.tolist()],
    }


@asset(inputs={"model": trained_model, "split": train_test_split})
def evaluation(model: dict, split: dict) -> dict:
    """Evaluate the trained model on test data."""
    from sklearn.metrics import accuracy_score, classification_report

    y_test = split["y_test"]
    predictions = model["predictions"]
    accuracy = accuracy_score(y_test, predictions)
    report = classification_report(
        y_test, predictions, target_names=split["target_names"], output_dict=True
    )

    return {
        "test_accuracy": round(accuracy, 4),
        "train_accuracy": model["train_accuracy"],
        "feature_importances": dict(
            zip(split["feature_names"], model["feature_importances"])
        ),
        "classification_report": report,
    }
