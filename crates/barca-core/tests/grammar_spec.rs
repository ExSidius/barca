//! Comprehensive grammar specification tests.
//!
//! These tests define the complete set of Python decorator syntaxes that
//! barca must parse. Tests marked `#[ignore]` are aspirational — they
//! document desired behavior that hasn't been implemented yet.

use barca_core::model::*;
use barca_core::parse::extract_nodes;

// ═══════════════════════════════════════════════════════════════════════════════
// 1. Basic decorator forms
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn bare_asset_no_parens() {
    let src = r#"
from barca import asset

@asset
def my_asset() -> dict:
    return {"x": 1}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].kind, NodeKind::Asset);
    assert_eq!(nodes[0].freshness, Freshness::Always);
}

#[test]
fn asset_empty_parens() {
    let src = r#"
from barca import asset

@asset()
def my_asset() -> dict:
    return {"x": 1}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].kind, NodeKind::Asset);
}

#[test]
fn sensor_decorator() {
    let src = r#"
from barca import sensor

@sensor()
def check_inbox():
    return (True, {"files": ["a.csv"]})
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].kind, NodeKind::Sensor);
    // Sensors default to Manual freshness
    assert_eq!(nodes[0].freshness, Freshness::Manual);
}

#[test]
fn effect_decorator() {
    let src = r#"
from barca import effect

@effect()
def send_email():
    pass
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].kind, NodeKind::Effect);
    assert_eq!(nodes[0].freshness, Freshness::Always);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. Freshness policies
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn freshness_always_with_parens() {
    let src = r#"
from barca import asset, Always

@asset(freshness=Always())
def a(): pass
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].freshness, Freshness::Always);
}

#[test]
fn freshness_always_singleton_no_parens() {
    // Decision #2: support singleton form without parens
    let src = r#"
from barca import asset, Always

@asset(freshness=Always)
def a(): pass
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].freshness, Freshness::Always);
}

#[test]
fn freshness_manual_with_parens() {
    let src = r#"
from barca import asset, Manual

@asset(freshness=Manual())
def a(): pass
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].freshness, Freshness::Manual);
}

#[test]
fn freshness_manual_singleton_no_parens() {
    let src = r#"
from barca import asset, Manual

@asset(freshness=Manual)
def a(): pass
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].freshness, Freshness::Manual);
}

#[test]
fn freshness_schedule_cron() {
    let src = r#"
from barca import asset, Schedule

@asset(freshness=Schedule("0 5 * * *"))
def daily_job(): pass
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(
        nodes[0].freshness,
        Freshness::Schedule(CronExpr("0 5 * * *".into()))
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. Input declarations — aliasing and references
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn input_same_name_as_function() {
    let src = r#"
from barca import asset

@asset()
def raw_data(): return {}

@asset(inputs={"raw_data": raw_data})
def processed(raw_data: dict) -> dict:
    return raw_data
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[1].inputs[0].param_name, "raw_data");
    assert_eq!(
        nodes[1].inputs[0].upstream,
        NodeRef::FunctionName("raw_data".into())
    );
}

#[test]
fn input_aliased_param_name() {
    // Decision #1: param name can differ from upstream function name
    let src = r#"
from barca import asset

@asset()
def fetch_raw_prices(): return {"AAPL": 150}

@asset(inputs={"data": fetch_raw_prices})
def normalize(data: dict) -> dict:
    return data
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[1].inputs[0].param_name, "data");
    assert_eq!(
        nodes[1].inputs[0].upstream,
        NodeRef::FunctionName("fetch_raw_prices".into())
    );
}

#[test]
fn input_multiple_upstreams() {
    let src = r#"
from barca import asset

@asset()
def prices(): return {}

@asset()
def volumes(): return {}

@asset(inputs={"p": prices, "v": volumes})
def combined(p: dict, v: dict) -> dict:
    return {**p, **v}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[2].inputs.len(), 2);
    assert_eq!(nodes[2].inputs[0].param_name, "p");
    assert_eq!(nodes[2].inputs[1].param_name, "v");
}

#[test]
fn input_collect_fan_in() {
    let src = r#"
from barca import asset, collect, partitions

@asset(partitions={"ticker": partitions(["AAPL", "MSFT"])})
def fetch_prices(ticker: str) -> dict:
    return {"ticker": ticker}

@asset(inputs={"all_prices": collect(fetch_prices)})
def summary(all_prices: dict) -> dict:
    return {"count": len(all_prices)}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[1].inputs[0].param_name, "all_prices");
    assert!(nodes[1].inputs[0].collected);
    assert_eq!(
        nodes[1].inputs[0].upstream,
        NodeRef::FunctionName("fetch_prices".into())
    );
}

#[test]
fn input_canonical_asset_ref() {
    let src = r#"
from barca import asset, asset_ref

@asset(inputs={"data": asset_ref("other_module/assets.py:raw_data")})
def process(data: dict) -> dict:
    return data
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].inputs[0].param_name, "data");
    assert_eq!(
        nodes[0].inputs[0].upstream,
        NodeRef::Canonical("other_module/assets.py:raw_data".into())
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. Sensor → downstream: payload unpacking
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn sensor_as_input_to_asset() {
    // Decision #4: downstream receives just the payload, not the full (bool, output) tuple.
    // The parser/DAG doesn't enforce this — it's a runner concern. But the structure is valid.
    let src = r#"
from barca import asset, sensor, Schedule, Always

@sensor(freshness=Schedule("*/5 * * * *"))
def inbox_sensor():
    return (True, {"files": ["a.csv", "b.csv"]})

@asset(inputs={"files": inbox_sensor}, freshness=Always)
def process_inbox(files: list) -> dict:
    return {"processed": len(files)}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].kind, NodeKind::Sensor);
    assert_eq!(nodes[1].kind, NodeKind::Asset);
    assert_eq!(nodes[1].inputs[0].param_name, "files");
    assert_eq!(
        nodes[1].inputs[0].upstream,
        NodeRef::FunctionName("inbox_sensor".into())
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. Partitions — static, dynamic, derived
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn partitions_static_string_list() {
    let src = r#"
from barca import asset, partitions

@asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
def fetch_prices(ticker: str) -> dict:
    return {"ticker": ticker}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let spec = nodes[0].partitions.get("ticker").unwrap();
    match spec {
        PartitionSpec::Static { values } => {
            assert_eq!(values.len(), 3);
            assert_eq!(values[0], PartitionValue::Str("AAPL".into()));
            assert_eq!(values[1], PartitionValue::Str("MSFT".into()));
            assert_eq!(values[2], PartitionValue::Str("GOOG".into()));
        }
        _ => panic!("expected static partitions"),
    }
}

#[test]
fn partitions_static_int_list() {
    let src = r#"
from barca import asset, partitions

@asset(partitions={"year": partitions([2020, 2021, 2022, 2023])})
def annual_report(year: int) -> dict:
    return {"year": year}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let spec = nodes[0].partitions.get("year").unwrap();
    match spec {
        PartitionSpec::Static { values } => {
            assert_eq!(values.len(), 4);
            // Note: int extraction from ruff AST is implementation-dependent
        }
        _ => panic!("expected static partitions"),
    }
}

#[test]
fn partitions_derived_from_upstream() {
    let src = r#"
from barca import asset, partitions_from

@asset()
def ticker_universe() -> list:
    return ["AAPL", "MSFT", "GOOG"]

@asset(partitions={"ticker": partitions_from(ticker_universe)})
def fetch_prices(ticker: str) -> dict:
    return {"ticker": ticker}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let spec = nodes[1].partitions.get("ticker").unwrap();
    match spec {
        PartitionSpec::DerivedFrom { source_ref } => {
            assert_eq!(source_ref.resolution_name(), "ticker_universe");
        }
        _ => panic!("expected derived partitions"),
    }
}

#[test]
#[ignore] // Decision #6: requires Python evaluation at plan time
fn partitions_list_comprehension() {
    // Aspirational: list comprehension should be flagged as "dynamic-static"
    // and evaluated by spawning Python at plan time.
    let src = r#"
from barca import asset, partitions

@asset(partitions={"key": partitions([f"p{i:05d}" for i in range(100)])})
def wide_asset(key: str) -> dict:
    return {"key": key}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let spec = nodes[0].partitions.get("key").unwrap();
    // Should be classified as needing Python evaluation
    match spec {
        PartitionSpec::Static { values } => {
            assert_eq!(values.len(), 100);
        }
        _ => panic!("expected resolved static partitions"),
    }
}

#[test]
#[ignore] // Decision #6: requires Python evaluation at plan time
fn partitions_function_call_expression() {
    // Aspirational: function call in partition values requires Python eval
    let src = r#"
from barca import asset, partitions

def get_tickers():
    return ["AAPL", "MSFT", "GOOG", "AMZN"]

@asset(partitions={"ticker": partitions(get_tickers())})
def fetch_prices(ticker: str) -> dict:
    return {"ticker": ticker}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let spec = nodes[0].partitions.get("ticker").unwrap();
    match spec {
        PartitionSpec::Static { values } => {
            assert_eq!(values.len(), 4);
        }
        _ => panic!("expected resolved static partitions"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. Sinks — stacking, serializers
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn single_sink() {
    let src = r#"
from barca import asset, sink

@asset()
@sink("output/report.json", serializer="json")
def report() -> dict:
    return {"rows": 42}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].sinks.len(), 1);
    assert_eq!(nodes[0].sinks[0].path, "output/report.json");
    assert_eq!(nodes[0].sinks[0].serializer, Some(SerializerKind::Json));
}

#[test]
fn multiple_sinks_stacked() {
    let src = r#"
from barca import asset, sink

@asset()
@sink("local/report.json", serializer="json")
@sink("s3://my-bucket/report.parquet", serializer="parquet")
@sink("tmp/report.txt", serializer="text")
def report() -> dict:
    return {"rows": 42}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].sinks.len(), 3);
    assert_eq!(nodes[0].sinks[0].serializer, Some(SerializerKind::Json));
    assert_eq!(nodes[0].sinks[1].serializer, Some(SerializerKind::Parquet));
    assert_eq!(nodes[0].sinks[2].serializer, Some(SerializerKind::Text));
}

#[test]
fn sink_without_serializer() {
    let src = r#"
from barca import asset, sink

@asset()
@sink("output/data.json")
def my_data() -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].sinks.len(), 1);
    assert_eq!(nodes[0].sinks[0].serializer, None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. Metadata — name, description, tags, timeout
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn explicit_name_override() {
    let src = r#"
from barca import asset

@asset(name="prices")
def fetch_latest_prices() -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].explicit_name, Some("prices".into()));
    assert_eq!(nodes[0].continuity_key(), "prices");
}

#[test]
fn continuity_key_defaults_to_file_and_function() {
    let src = r#"
from barca import asset

@asset()
def my_asset() -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "project/assets.py").unwrap();
    assert_eq!(nodes[0].continuity_key(), "project/assets.py:my_asset");
}

#[test]
fn description_parameter() {
    let src = r#"
from barca import asset

@asset(description="Fetches daily price data from the exchange API")
def fetch_prices() -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(
        nodes[0].description,
        Some("Fetches daily price data from the exchange API".into())
    );
}

#[test]
fn tags_parameter() {
    let src = r#"
from barca import asset

@asset(tags={"team": "data-eng", "concurrency_group": "network"})
def api_call() -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].tags.get("team").unwrap(), "data-eng");
    assert_eq!(nodes[0].tags.get("concurrency_group").unwrap(), "network");
}

#[test]
fn timeout_seconds_parameter() {
    let src = r#"
from barca import asset

@asset(timeout_seconds=600)
def slow_train() -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].timeout_seconds, 600);
}

#[test]
fn timeout_defaults_to_300() {
    let src = r#"
from barca import asset

@asset()
def normal_asset() -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes[0].timeout_seconds, 300);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. @unsafe decorator
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn unsafe_marks_function() {
    let src = r#"
from barca import asset, unsafe

@unsafe
@asset()
def dynamic_config() -> dict:
    return eval("{'key': 'value'}")
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert!(nodes[0].is_unsafe);
}

#[test]
fn unsafe_order_independent() {
    // @unsafe can come before or after @asset
    let src = r#"
from barca import asset, unsafe

@asset()
@unsafe
def dynamic_config() -> dict:
    return eval("{'key': 'value'}")
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert!(nodes[0].is_unsafe);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. DAG validation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn dag_rejects_effect_as_input() {
    use barca_core::dag::Dag;

    let src = r#"
from barca import asset, effect

@effect()
def send_email():
    pass

@asset(inputs={"email": send_email})
def bad_asset(email) -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let result = Dag::build(&nodes);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("effect"));
}

#[test]
fn dag_rejects_sensor_with_inputs() {
    use barca_core::dag::Dag;

    let src = r#"
from barca import asset, sensor

@asset()
def data(): return {}

@sensor(inputs={"data": data})
def bad_sensor(data):
    return (True, {})
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let result = Dag::build(&nodes);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("sensor"));
}

#[test]
fn dag_rejects_duplicate_continuity_key() {
    use barca_core::dag::Dag;

    let src = r#"
from barca import asset

@asset(name="shared_name")
def asset_a(): return {}

@asset(name="shared_name")
def asset_b(): return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let result = Dag::build(&nodes);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("duplicate") || err.contains("Duplicate"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 10. DAG shape classification
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn dag_classifies_linear_chain() {
    use barca_core::dag::Dag;
    use barca_core::plan::DagShape;

    let src = r#"
from barca import asset

@asset()
def a(): return 1

@asset(inputs={"a": a})
def b(a): return a + 1

@asset(inputs={"b": b})
def c(b): return b + 1
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let dag = Dag::build(&nodes).unwrap();
    assert_eq!(dag.classify_shape(), DagShape::LinearChain);
}

#[test]
fn dag_classifies_wide_fan_out() {
    use barca_core::dag::Dag;
    use barca_core::plan::DagShape;

    let src = r#"
from barca import asset

@asset()
def a(): return 1
@asset()
def b(): return 2
@asset()
def c(): return 3
@asset()
def d(): return 4
@asset()
def e(): return 5
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let dag = Dag::build(&nodes).unwrap();
    assert_eq!(dag.classify_shape(), DagShape::WideFanOut);
}

#[test]
fn dag_classifies_diamond() {
    use barca_core::dag::Dag;
    use barca_core::plan::DagShape;

    let src = r#"
from barca import asset

@asset()
def source(): return {}

@asset(inputs={"s": source})
def branch_a(s): return s

@asset(inputs={"s": source})
def branch_b(s): return s

@asset(inputs={"a": branch_a, "b": branch_b})
def merge(a, b): return {**a, **b}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let dag = Dag::build(&nodes).unwrap();
    assert_eq!(dag.classify_shape(), DagShape::Diamond);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11. Aspirational: split plans for partitions_from
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
#[ignore] // Decision #3: requires multi-phase plan generation
fn partitions_from_creates_partition_source_edge() {
    use barca_core::dag::Dag;

    let src = r#"
from barca import asset, partitions_from

@asset()
def ticker_universe() -> list:
    return ["AAPL", "MSFT", "GOOG"]

@asset(partitions={"ticker": partitions_from(ticker_universe)})
def fetch_prices(ticker: str) -> dict:
    return {"ticker": ticker}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let dag = Dag::build(&nodes).unwrap();

    // The partition source edge should be visible in the DAG
    let upstream = dag.upstream("test.py:fetch_prices");
    assert!(upstream.contains(&"test.py:ticker_universe"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 12. Aspirational: collect() type semantics
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn collect_marks_input_as_collected() {
    // Decision #5: collect() changes the runtime type to dict[tuple, T]
    // The parser just marks it; type checking is a stubs concern.
    let src = r#"
from barca import asset, collect, partitions

@asset(partitions={"ticker": partitions(["AAPL", "MSFT"])})
def per_ticker(ticker: str) -> dict:
    return {"ticker": ticker, "price": 100}

@asset(inputs={"all_data": collect(per_ticker)})
def aggregate(all_data) -> dict:
    return {"total": sum(v["price"] for v in all_data.values())}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let agg = &nodes[1];
    assert_eq!(agg.inputs[0].param_name, "all_data");
    assert!(agg.inputs[0].collected);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 13. Complex real-world patterns
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn spaceflights_topology() {
    use barca_core::dag::Dag;

    let src = r#"
from barca import asset

@asset()
def raw_shuttles(): return {}

@asset()
def raw_companies(): return {}

@asset()
def raw_reviews(): return {}

@asset(inputs={"raw": raw_shuttles})
def prep_shuttles(raw): return raw

@asset(inputs={"raw": raw_companies})
def prep_companies(raw): return raw

@asset(inputs={"raw": raw_reviews})
def prep_reviews(raw): return raw

@asset(inputs={"s": prep_shuttles, "c": prep_companies, "r": prep_reviews})
def master_table(s, c, r): return {}

@asset(inputs={"data": master_table})
def split(data): return {}

@asset(inputs={"data": split})
def train(data): return {}

@asset(inputs={"model": train, "data": split})
def evaluate(model, data): return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    let dag = Dag::build(&nodes).unwrap();

    assert_eq!(dag.node_count(), 10);
    // 3 raw→prep edges + 3 prep→master + master→split + split→train + train→eval + split→eval
    assert_eq!(dag.edge_count(), 10);

    let tiers = dag.compute_tiers();
    // raw sources at tier 0, preps at tier 1, master at tier 2, split at 3, train at 4, eval at 5
    assert_eq!(tiers["test.py:raw_shuttles"], 0);
    assert_eq!(tiers["test.py:prep_shuttles"], 1);
    assert_eq!(tiers["test.py:master_table"], 2);
    assert_eq!(tiers["test.py:split"], 3);
    assert_eq!(tiers["test.py:train"], 4);
    assert_eq!(tiers["test.py:evaluate"], 5);
}

#[test]
fn all_decorators_combined() {
    // A single file with every decorator type
    let src = r#"
from barca import asset, sensor, effect, sink, unsafe, Always, Manual, Schedule, partitions, collect

@sensor(freshness=Schedule("*/5 * * * *"), description="Check for new files")
def file_watcher():
    return (True, {"files": []})

@asset(freshness=Always, tags={"team": "data"})
@sink("output/raw.json", serializer="json")
def ingest() -> dict:
    return {"rows": 100}

@asset(
    inputs={"raw": ingest, "trigger": file_watcher},
    freshness=Manual,
    timeout_seconds=600,
    name="transform_v2",
    description="Main transformation pipeline",
)
def transform(raw: dict, trigger) -> dict:
    return {"transformed": raw["rows"]}

@unsafe
@asset(partitions={"region": partitions(["us", "eu", "ap"])})
def regional_export(region: str) -> dict:
    return {"region": region}

@effect(inputs={"data": transform}, freshness=Always)
def notify(data):
    pass
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes.len(), 5);

    // Sensor
    assert_eq!(nodes[0].kind, NodeKind::Sensor);
    assert_eq!(
        nodes[0].freshness,
        Freshness::Schedule(CronExpr("*/5 * * * *".into()))
    );
    assert_eq!(nodes[0].description, Some("Check for new files".into()));

    // Asset with sink
    assert_eq!(nodes[1].kind, NodeKind::Asset);
    assert_eq!(nodes[1].sinks.len(), 1);
    assert_eq!(nodes[1].tags.get("team").unwrap(), "data");

    // Asset with all metadata
    assert_eq!(nodes[2].explicit_name, Some("transform_v2".into()));
    assert_eq!(nodes[2].freshness, Freshness::Manual);
    assert_eq!(nodes[2].timeout_seconds, 600);
    assert_eq!(nodes[2].inputs.len(), 2);

    // Unsafe partitioned asset
    assert!(nodes[3].is_unsafe);
    assert!(nodes[3].partitions.contains_key("region"));

    // Effect
    assert_eq!(nodes[4].kind, NodeKind::Effect);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 14. Negative tests / edge cases — error handling and graceful degradation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn malformed_python_returns_err() {
    let src = "def broken(:\n    pass";
    let result = extract_nodes(src, "bad.py");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("syntax error"));
    assert!(err.contains("bad.py"));
}

#[test]
fn empty_file_returns_empty_vec() {
    let nodes = extract_nodes("", "empty.py").unwrap();
    assert!(nodes.is_empty());
}

#[test]
fn file_with_no_barca_decorators() {
    let src = r#"
def regular_function():
    return 42

class MyClass:
    pass
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert!(nodes.is_empty());
}

#[test]
fn inputs_not_a_dict_is_ignored() {
    let src = r#"
from barca import asset

@asset(inputs="not_a_dict")
def bad_inputs() -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes.len(), 1);
    assert!(nodes[0].inputs.is_empty()); // gracefully ignored
}

#[test]
fn timeout_not_an_int_falls_back_to_default() {
    let src = r#"
from barca import asset

@asset(timeout_seconds="not_an_int")
def bad_timeout() -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].timeout_seconds, 300); // default
}

#[test]
fn empty_partition_list() {
    let src = r#"
from barca import asset, partitions

@asset(partitions={"key": partitions([])})
def empty_partitions(key: str) -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes.len(), 1);
    let spec = nodes[0].partitions.get("key").unwrap();
    match spec {
        PartitionSpec::Static { values } => assert!(values.is_empty()),
        _ => panic!("expected static partitions"),
    }
}

#[test]
fn decorator_on_class_is_ignored() {
    let src = r#"
from barca import asset

@asset()
class NotAFunction:
    pass

@asset()
def real_asset() -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes.len(), 1); // only the function, not the class
    assert_eq!(nodes[0].function_name, "real_asset");
}

#[test]
fn aliased_import_not_detected() {
    // Documented limitation: parser matches exact decorator names.
    // Aliased imports are not supported.
    let src = r#"
from barca import asset as a

@a()
def aliased() -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert!(nodes.is_empty()); // not detected — known limitation
}

#[test]
fn comments_and_docstrings_dont_break_parsing() {
    let src = r#"
# This is a comment
"""This is a module docstring."""

from barca import asset

# Another comment
@asset()
def my_asset() -> dict:
    """Asset docstring."""
    # inline comment
    return {"value": 1}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes.len(), 1);
}

#[test]
fn unknown_decorator_kwargs_are_ignored() {
    let src = r#"
from barca import asset

@asset(unknown_param="hello", another=42)
def my_asset() -> dict:
    return {}
"#;
    let nodes = extract_nodes(src, "test.py").unwrap();
    assert_eq!(nodes.len(), 1);
    // Unknown kwargs silently ignored — only known params extracted
    assert_eq!(nodes[0].freshness, Freshness::Always); // default
}
