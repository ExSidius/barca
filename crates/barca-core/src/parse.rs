//! Python source parsing — extracts decorated nodes using ruff's AST.
//!
//! Pure function: `(source, file_path) → Result<Vec<ExtractedNode>, ParseError>`.
//! No I/O, no side effects.

use ruff_python_ast::{self as ast, Expr, Keyword, Stmt};
use ruff_python_parser::parse_module;
use ruff_text_size::Ranged;
use smallvec::SmallVec;
use std::collections::HashMap;

use crate::model::{
    CronExpr, DeclaredInput, ExtractedNode, Freshness, NodeKind, NodeRef, PartitionSpec,
    PartitionValue, SerializerKind, SinkDecl,
};

/// Error from parsing a Python source file.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("syntax error in {file}: {message}")]
    SyntaxError { file: String, message: String },
}

/// Parse a Python source file and extract all barca-decorated nodes.
///
/// Pure function — no I/O, no side effects. Returns `Err` for unparseable Python.
/// Returns `Ok(vec![])` for valid Python with no barca decorators.
pub fn extract_nodes(source: &str, file_path: &str) -> Result<Vec<ExtractedNode>, ParseError> {
    let parsed = parse_module(source).map_err(|e| ParseError::SyntaxError {
        file: file_path.to_string(),
        message: e.to_string(),
    })?;

    let module = parsed.into_syntax();
    let mut nodes = Vec::new();

    for stmt in &module.body {
        if let Stmt::FunctionDef(func) = stmt
            && let Some(extracted) = try_extract_function(func, file_path, source)
        {
            nodes.push(extracted);
        }
    }

    Ok(nodes)
}

fn try_extract_function(
    func: &ast::StmtFunctionDef,
    file_path: &str,
    source: &str,
) -> Option<ExtractedNode> {
    let mut kind = None;
    let mut keywords: Vec<&Keyword> = Vec::new();
    let mut sinks: SmallVec<[SinkDecl; 2]> = SmallVec::new();
    let mut is_unsafe = false;

    for decorator in &func.decorator_list {
        // Check for @unsafe
        if is_unsafe_decorator(&decorator.expression) {
            is_unsafe = true;
            continue;
        }

        // Check for @sink(...)
        if let Some(sink) = try_extract_sink(&decorator.expression) {
            sinks.push(sink);
            continue;
        }

        // Check for @asset/@sensor/@effect
        if let Some((k, kws)) = match_node_decorator(&decorator.expression) {
            kind = Some(k);
            keywords = kws;
        }
    }

    let kind = kind?;

    let freshness = extract_freshness(&keywords).unwrap_or(Freshness::default_for(kind));
    let inputs = extract_inputs(&keywords);
    let partitions = extract_partitions(&keywords);
    let explicit_name = extract_string_kwarg(&keywords, "name");
    let description = extract_string_kwarg(&keywords, "description");
    let timeout_seconds = extract_int_kwarg(&keywords, "timeout_seconds").unwrap_or(300);
    let tags = extract_tags(&keywords);

    let start = func.range().start().to_usize();
    let end = func.range().end().to_usize();
    let source_text = source[start..end].to_string();

    Some(ExtractedNode {
        kind,
        function_name: func.name.to_string(),
        explicit_name,
        freshness,
        inputs,
        partitions,
        sinks,
        timeout_seconds,
        description,
        tags,
        is_unsafe,
        source_file: file_path.to_string(),
        byte_offset: start,
        source_text,
    })
}

fn is_unsafe_decorator(expr: &Expr) -> bool {
    matches!(expr, Expr::Name(n) if n.id.as_str() == "unsafe")
}

fn try_extract_sink(expr: &Expr) -> Option<SinkDecl> {
    if let Expr::Call(call) = expr
        && let Expr::Name(n) = call.func.as_ref()
        && n.id.as_str() == "sink"
    {
        let path = call
            .arguments
            .args
            .first()
            .and_then(extract_string_literal)?;
        let serializer = call
            .arguments
            .keywords
            .iter()
            .find(|kw| kw.arg.as_ref().map(|a| a.as_str()) == Some("serializer"))
            .and_then(|kw| extract_serializer_kind(&kw.value));
        return Some(SinkDecl { path, serializer });
    }
    None
}

fn extract_serializer_kind(expr: &Expr) -> Option<SerializerKind> {
    let name = match expr {
        Expr::StringLiteral(s) => s.value.to_string(),
        Expr::Name(n) => n.id.to_string(),
        _ => return None,
    };
    match name.to_lowercase().as_str() {
        "json" => Some(SerializerKind::Json),
        "parquet" => Some(SerializerKind::Parquet),
        "pickle" => Some(SerializerKind::Pickle),
        "text" => Some(SerializerKind::Text),
        "yaml" => Some(SerializerKind::Yaml),
        _ => None,
    }
}

fn match_node_decorator(expr: &Expr) -> Option<(NodeKind, Vec<&Keyword>)> {
    match expr {
        Expr::Name(name) => {
            let kind = match name.id.as_str() {
                "asset" => NodeKind::Asset,
                "sensor" => NodeKind::Sensor,
                "effect" => NodeKind::Effect,
                _ => return None,
            };
            Some((kind, vec![]))
        }
        Expr::Call(call) => {
            let name = match call.func.as_ref() {
                Expr::Name(n) => n.id.as_str(),
                _ => return None,
            };
            let kind = match name {
                "asset" => NodeKind::Asset,
                "sensor" => NodeKind::Sensor,
                "effect" => NodeKind::Effect,
                _ => return None,
            };
            let kwargs: Vec<&Keyword> = call.arguments.keywords.iter().collect();
            Some((kind, kwargs))
        }
        _ => None,
    }
}

fn extract_freshness(keywords: &[&Keyword]) -> Option<Freshness> {
    for kw in keywords {
        let Some(ref ident) = kw.arg else { continue };
        if ident.as_str() != "freshness" {
            continue;
        }
        return Some(match &kw.value {
            Expr::Call(call) => match call.func.as_ref() {
                Expr::Name(n) => match n.id.as_str() {
                    "Always" => Freshness::Always,
                    "Manual" => Freshness::Manual,
                    "Schedule" => {
                        let cron = call
                            .arguments
                            .args
                            .first()
                            .and_then(extract_string_literal)
                            .unwrap_or_default();
                        Freshness::Schedule(CronExpr(cron))
                    }
                    _ => return None,
                },
                _ => return None,
            },
            Expr::Name(n) => match n.id.as_str() {
                "Always" => Freshness::Always,
                "Manual" => Freshness::Manual,
                _ => return None,
            },
            _ => return None,
        });
    }
    None
}

fn extract_inputs(keywords: &[&Keyword]) -> SmallVec<[DeclaredInput; 4]> {
    for kw in keywords {
        let Some(ref ident) = kw.arg else { continue };
        if ident.as_str() != "inputs" {
            continue;
        }
        if let Expr::Dict(dict) = &kw.value {
            return extract_inputs_from_dict(dict);
        }
    }
    SmallVec::new()
}

fn extract_inputs_from_dict(dict: &ast::ExprDict) -> SmallVec<[DeclaredInput; 4]> {
    let mut inputs = SmallVec::new();

    for item in &dict.items {
        let Some(ref key_expr) = item.key else {
            continue;
        };
        let param_name = match extract_string_literal(key_expr) {
            Some(s) => s,
            None => continue,
        };

        let (upstream, collected) = match &item.value {
            Expr::Name(n) => (NodeRef::FunctionName(n.id.to_string()), false),
            Expr::Call(call) => {
                let is_collect =
                    matches!(call.func.as_ref(), Expr::Name(n) if n.id.as_str() == "collect");
                if is_collect {
                    if let Some(Expr::Name(n)) = call.arguments.args.first() {
                        (NodeRef::FunctionName(n.id.to_string()), true)
                    } else {
                        continue;
                    }
                } else if let Expr::Name(n) = call.func.as_ref() {
                    // asset_ref("...") or other call
                    if n.id.as_str() == "asset_ref" {
                        if let Some(arg) = call.arguments.args.first() {
                            if let Some(s) = extract_string_literal(arg) {
                                (NodeRef::Canonical(s), false)
                            } else {
                                continue;
                            }
                        } else {
                            continue;
                        }
                    } else {
                        (NodeRef::FunctionName(n.id.to_string()), false)
                    }
                } else {
                    continue;
                }
            }
            _ => continue,
        };

        inputs.push(DeclaredInput {
            param_name,
            upstream,
            collected,
        });
    }

    inputs
}

fn extract_partitions(keywords: &[&Keyword]) -> HashMap<String, PartitionSpec> {
    let mut result = HashMap::new();

    for kw in keywords {
        let Some(ref ident) = kw.arg else { continue };
        if ident.as_str() != "partitions" {
            continue;
        }
        if let Expr::Dict(dict) = &kw.value {
            for item in &dict.items {
                let Some(ref key_expr) = item.key else {
                    continue;
                };
                let key = match extract_string_literal(key_expr) {
                    Some(s) => s,
                    None => continue,
                };

                let spec = match &item.value {
                    Expr::Call(call) => {
                        if let Expr::Name(n) = call.func.as_ref() {
                            match n.id.as_str() {
                                "partitions" => {
                                    let values = call
                                        .arguments
                                        .args
                                        .first()
                                        .map(extract_partition_values)
                                        .unwrap_or_default();
                                    PartitionSpec::Static { values }
                                }
                                "partitions_from" => {
                                    let source_ref = call.arguments.args.first().and_then(|a| {
                                        if let Expr::Name(n) = a {
                                            Some(NodeRef::FunctionName(n.id.to_string()))
                                        } else {
                                            None
                                        }
                                    });
                                    // Skip if we can't resolve the reference — DAG
                                    // validation will catch missing deps later.
                                    if let Some(source_ref) = source_ref {
                                        PartitionSpec::DerivedFrom { source_ref }
                                    } else {
                                        continue;
                                    }
                                }
                                _ => continue,
                            }
                        } else {
                            continue;
                        }
                    }
                    _ => continue,
                };

                result.insert(key, spec);
            }
        }
    }

    result
}

fn extract_partition_values(expr: &Expr) -> Vec<PartitionValue> {
    if let Expr::List(list) = expr {
        list.elts
            .iter()
            .filter_map(|e| match e {
                Expr::StringLiteral(s) => Some(PartitionValue::Str(s.value.to_string())),
                Expr::NumberLiteral(n) => {
                    if let ast::Number::Int(i) = &n.value {
                        i.as_i64().map(PartitionValue::Int)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect()
    } else {
        vec![]
    }
}

fn extract_tags(keywords: &[&Keyword]) -> HashMap<String, String> {
    let mut tags = HashMap::new();
    for kw in keywords {
        let Some(ref ident) = kw.arg else { continue };
        if ident.as_str() != "tags" {
            continue;
        }
        if let Expr::Dict(dict) = &kw.value {
            for item in &dict.items {
                let Some(ref key_expr) = item.key else {
                    continue;
                };
                if let (Some(k), Some(v)) = (
                    extract_string_literal(key_expr),
                    extract_string_literal(&item.value),
                ) {
                    tags.insert(k, v);
                }
            }
        }
    }
    tags
}

fn extract_string_kwarg(keywords: &[&Keyword], name: &str) -> Option<String> {
    for kw in keywords {
        let Some(ref ident) = kw.arg else { continue };
        if ident.as_str() == name {
            return extract_string_literal(&kw.value);
        }
    }
    None
}

fn extract_int_kwarg(keywords: &[&Keyword], name: &str) -> Option<u32> {
    for kw in keywords {
        let Some(ref ident) = kw.arg else { continue };
        if ident.as_str() == name
            && let Expr::NumberLiteral(n) = &kw.value
            && let ast::Number::Int(i) = &n.value
        {
            return i.as_u32();
        }
    }
    None
}

fn extract_string_literal(expr: &Expr) -> Option<String> {
    if let Expr::StringLiteral(s) = expr {
        Some(s.value.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trivial_asset() {
        let src = r#"
from barca import asset

@asset()
def single_asset() -> dict:
    return {"status": "ok"}
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Asset);
        assert_eq!(nodes[0].function_name, "single_asset");
        assert_eq!(nodes[0].freshness, Freshness::Always);
    }

    #[test]
    fn test_bare_asset() {
        let src = r#"
from barca import asset

@asset
def bare() -> dict:
    return {}
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].function_name, "bare");
    }

    #[test]
    fn test_inputs() {
        let src = r#"
from barca import asset

@asset()
def a() -> str:
    return "hello"

@asset(inputs={"a": a})
def b(a: str) -> str:
    return a.upper()
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[1].inputs.len(), 1);
        assert_eq!(nodes[1].inputs[0].param_name, "a");
        assert!(!nodes[1].inputs[0].collected);
    }

    #[test]
    fn test_sensor_and_effect() {
        let src = r#"
from barca import sensor, effect, Schedule, Always

@sensor(freshness=Schedule("*/5 * * * *"))
def my_sensor():
    return (True, {})

@effect(inputs={"data": my_sensor}, freshness=Always())
def my_effect(data):
    print(data)
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert_eq!(nodes[0].kind, NodeKind::Sensor);
        assert_eq!(
            nodes[0].freshness,
            Freshness::Schedule(CronExpr("*/5 * * * *".into()))
        );
        assert_eq!(nodes[1].kind, NodeKind::Effect);
    }

    #[test]
    fn test_sink_stacking() {
        let src = r#"
from barca import asset, sink

@asset()
@sink("tmp/out.json", serializer="json")
@sink("s3://bucket/out.txt", serializer="text")
def my_asset() -> dict:
    return {}
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].sinks.len(), 2);
        assert_eq!(nodes[0].sinks[0].path, "tmp/out.json");
        assert_eq!(nodes[0].sinks[0].serializer, Some(SerializerKind::Json));
        assert_eq!(nodes[0].sinks[1].path, "s3://bucket/out.txt");
    }
}
