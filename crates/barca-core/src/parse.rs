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
    CronExpr, DeclaredInput, ExtractedNode, Freshness, NodeKind, NodeRef, ParallelCall,
    PartitionSpec, PartitionValue, SerializerKind, SinkDecl,
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
            // Cone hash computed later in build_dag with cached module definitions.
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

        // Check for @asset/@sensor/@task
        if let Some((k, kws)) = match_node_decorator(&decorator.expression) {
            kind = Some(k);
            keywords = kws;
        }
    }

    let kind = kind?;

    let freshness = extract_freshness(&keywords).unwrap_or(Freshness::default_for(kind));
    let inputs = extract_inputs(&keywords);
    let partitions = extract_partitions(&keywords, source);
    let explicit_name = extract_string_kwarg(&keywords, "name");
    let description = extract_string_kwarg(&keywords, "description");
    let timeout_seconds = extract_int_kwarg(&keywords, "timeout_seconds").unwrap_or(300);
    // `retries` is the total number of attempts (1 = no retry). Clamp 0 → 1.
    let retries = extract_int_kwarg(&keywords, "retries").unwrap_or(1).max(1);
    let retry_backoff_seconds = extract_float_kwarg(&keywords, "retry_backoff").unwrap_or(0.0);
    let tags = extract_tags(&keywords);
    let artifact_serializer = keywords
        .iter()
        .find(|kw| kw.arg.as_ref().map(|a| a.as_str()) == Some("serializer"))
        .and_then(|kw| extract_serializer_kind(&kw.value));

    let parallel_calls = if kind == NodeKind::Task {
        extract_parallel_calls(&func.body)
    } else {
        Vec::new()
    };

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
        retries,
        retry_backoff_seconds,
        description,
        tags,
        is_unsafe,
        source_file: file_path.to_string(),
        byte_offset: start,
        source_text,
        cone_hash: String::new(), // computed after extraction in extract_nodes()
        artifact_serializer,
        parallel_calls,
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
                "task" => NodeKind::Task,
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
                "task" => NodeKind::Task,
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

fn extract_partitions(keywords: &[&Keyword], source: &str) -> HashMap<String, PartitionSpec> {
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
                                "partitions" => extract_partition_spec(call, source),
                                "partitions_from" => {
                                    let source_ref = call.arguments.args.first().and_then(|a| {
                                        if let Expr::Name(n) = a {
                                            Some(NodeRef::FunctionName(n.id.to_string()))
                                        } else {
                                            None
                                        }
                                    });
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

/// Extract a PartitionSpec from a `partitions(...)` call.
/// If the argument is a literal list, extract values statically.
/// Otherwise, extract the source text for dynamic Python evaluation at plan time.
fn extract_partition_spec(call: &ast::ExprCall, source: &str) -> PartitionSpec {
    let Some(first_arg) = call.arguments.args.first() else {
        return PartitionSpec::Static { values: vec![] };
    };

    // Try static extraction first (literal list).
    if let Expr::List(_) = first_arg {
        let values = extract_partition_values(first_arg);
        return PartitionSpec::Static { values };
    }

    // Non-literal (ListComp, Call, etc.) — extract source text for Python eval.
    let start = first_arg.range().start().to_usize();
    let end = first_arg.range().end().to_usize();
    let source_text = source[start..end].to_string();
    PartitionSpec::Dynamic { source_text }
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

/// Extract a float kwarg, accepting both float (`2.0`) and int (`2`) literals.
fn extract_float_kwarg(keywords: &[&Keyword], name: &str) -> Option<f64> {
    for kw in keywords {
        let Some(ref ident) = kw.arg else { continue };
        if ident.as_str() == name
            && let Expr::NumberLiteral(n) = &kw.value
        {
            return match &n.value {
                ast::Number::Float(f) => Some(*f),
                ast::Number::Int(i) => i.as_u32().map(|v| v as f64),
                _ => None,
            };
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

/// Scan a task function body for `parallel(...)` and `parallel_map(...)` calls.
/// Returns a list of ParallelCall structs describing each call.
fn extract_parallel_calls(body: &[Stmt]) -> Vec<ParallelCall> {
    let mut results = Vec::new();
    collect_parallel_calls_from_stmts(body, &mut results);
    results
}

/// Recursively walk statements looking for parallel()/parallel_map() calls.
fn collect_parallel_calls_from_stmts(stmts: &[Stmt], results: &mut Vec<ParallelCall>) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(expr_stmt) => {
                collect_parallel_calls_from_expr(&expr_stmt.value, results);
            }
            Stmt::Assign(assign) => {
                collect_parallel_calls_from_expr(&assign.value, results);
            }
            Stmt::AnnAssign(assign) => {
                if let Some(ref value) = assign.value {
                    collect_parallel_calls_from_expr(value, results);
                }
            }
            Stmt::Return(ret) => {
                if let Some(ref value) = ret.value {
                    collect_parallel_calls_from_expr(value, results);
                }
            }
            Stmt::If(if_stmt) => {
                collect_parallel_calls_from_stmts(&if_stmt.body, results);
                for clause in &if_stmt.elif_else_clauses {
                    collect_parallel_calls_from_stmts(&clause.body, results);
                }
            }
            Stmt::For(for_stmt) => {
                collect_parallel_calls_from_stmts(&for_stmt.body, results);
                collect_parallel_calls_from_stmts(&for_stmt.orelse, results);
            }
            Stmt::While(while_stmt) => {
                collect_parallel_calls_from_stmts(&while_stmt.body, results);
                collect_parallel_calls_from_stmts(&while_stmt.orelse, results);
            }
            Stmt::With(with_stmt) => {
                collect_parallel_calls_from_stmts(&with_stmt.body, results);
            }
            Stmt::Try(try_stmt) => {
                collect_parallel_calls_from_stmts(&try_stmt.body, results);
                for handler in &try_stmt.handlers {
                    let ast::ExceptHandler::ExceptHandler(h) = handler;
                    collect_parallel_calls_from_stmts(&h.body, results);
                }
                collect_parallel_calls_from_stmts(&try_stmt.orelse, results);
                collect_parallel_calls_from_stmts(&try_stmt.finalbody, results);
            }
            Stmt::Match(m) => {
                for case in &m.cases {
                    collect_parallel_calls_from_stmts(&case.body, results);
                }
            }
            Stmt::AugAssign(a) => {
                collect_parallel_calls_from_expr(&a.value, results);
            }
            _ => {}
        }
    }
}

/// Check if an expression is a call to `parallel()` or `parallel_map()` and extract it.
/// Recursively descends into sub-expressions (call args, ternaries, lists, tuples,
/// list comprehensions) to find nested parallel() calls.
fn collect_parallel_calls_from_expr(expr: &Expr, results: &mut Vec<ParallelCall>) {
    if let Expr::Call(call) = expr {
        if let Expr::Name(n) = call.func.as_ref() {
            match n.id.as_str() {
                "parallel" => {
                    results.push(extract_parallel_call(call));
                    return;
                }
                "parallel_map" => {
                    results.push(extract_parallel_map_call(call));
                    return;
                }
                _ => {}
            }
        }
        // Not a parallel/parallel_map call — descend into call arguments
        for arg in &call.arguments.args {
            collect_parallel_calls_from_expr(arg, results);
        }
        return;
    }

    // Descend into other expression forms
    match expr {
        Expr::If(e) => {
            collect_parallel_calls_from_expr(&e.body, results);
            collect_parallel_calls_from_expr(&e.test, results);
            collect_parallel_calls_from_expr(&e.orelse, results);
        }
        Expr::List(l) => {
            for elt in &l.elts {
                collect_parallel_calls_from_expr(elt, results);
            }
        }
        Expr::Tuple(t) => {
            for elt in &t.elts {
                collect_parallel_calls_from_expr(elt, results);
            }
        }
        Expr::ListComp(lc) => {
            collect_parallel_calls_from_expr(&lc.elt, results);
        }
        _ => {}
    }
}

/// Extract a ParallelCall from a `parallel(...)` call expression.
fn extract_parallel_call(call: &ast::ExprCall) -> ParallelCall {
    let mut static_refs = Vec::new();
    let mut is_dynamic = false;

    for arg in &call.arguments.args {
        match arg {
            // partial(func_name, ...) or functools.partial(func_name, ...) — extract func_name
            Expr::Call(inner_call) => {
                let is_partial = match inner_call.func.as_ref() {
                    Expr::Name(n) => n.id.as_str() == "partial",
                    Expr::Attribute(a) => a.attr.as_str() == "partial",
                    _ => false,
                };
                if is_partial {
                    if let Some(first_arg) = inner_call.arguments.args.first() {
                        if let Expr::Name(func_name) = first_arg {
                            static_refs.push(NodeRef::FunctionName(func_name.id.to_string()));
                        }
                    }
                }
            }
            // *expr — starred argument, always dynamic
            Expr::Starred(starred) => {
                is_dynamic = true;
                // Try to extract static_refs from the starred expression.
                // e.g., *(partial(deploy, r) for r in regions) — extract "deploy"
                extract_refs_from_starred(&starred.value, &mut static_refs);
            }
            _ => {}
        }
    }

    ParallelCall {
        static_refs,
        is_dynamic,
    }
}

/// Extract a ParallelCall from a `parallel_map(func, items, ...)` call expression.
fn extract_parallel_map_call(call: &ast::ExprCall) -> ParallelCall {
    let mut static_refs = Vec::new();

    // First arg is the function reference
    if let Some(first_arg) = call.arguments.args.first() {
        if let Expr::Name(func_name) = first_arg {
            static_refs.push(NodeRef::FunctionName(func_name.id.to_string()));
        }
    }

    // parallel_map is always dynamic (items resolved at runtime)
    ParallelCall {
        static_refs,
        is_dynamic: true,
    }
}

/// Try to extract function references from a starred expression.
/// Handles patterns like:
///   *(partial(deploy, r) for r in regions)  → extracts "deploy"
///   *work_items                              → no refs extractable
fn extract_refs_from_starred(expr: &Expr, refs: &mut Vec<NodeRef>) {
    match expr {
        // Generator expression: (partial(func, ...) for ... in ...)
        Expr::Generator(genexpr) => {
            if let Expr::Call(inner_call) = genexpr.elt.as_ref() {
                let is_partial = match inner_call.func.as_ref() {
                    Expr::Name(n) => n.id.as_str() == "partial",
                    Expr::Attribute(a) => a.attr.as_str() == "partial",
                    _ => false,
                };
                if is_partial {
                    if let Some(first_arg) = inner_call.arguments.args.first() {
                        if let Expr::Name(func_name) = first_arg {
                            refs.push(NodeRef::FunctionName(func_name.id.to_string()));
                        }
                    }
                }
            }
        }
        // List comprehension: [partial(func, ...) for ... in ...]
        Expr::ListComp(comp) => {
            if let Expr::Call(inner_call) = comp.elt.as_ref() {
                let is_partial = match inner_call.func.as_ref() {
                    Expr::Name(n) => n.id.as_str() == "partial",
                    Expr::Attribute(a) => a.attr.as_str() == "partial",
                    _ => false,
                };
                if is_partial {
                    if let Some(first_arg) = inner_call.arguments.args.first() {
                        if let Expr::Name(func_name) = first_arg {
                            refs.push(NodeRef::FunctionName(func_name.id.to_string()));
                        }
                    }
                }
            }
        }
        // Plain variable: *work_items — can't extract refs
        _ => {}
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
    fn test_sensor_and_task() {
        let src = r#"
from barca import sensor, task, Schedule, Always

@sensor(freshness=Schedule("*/5 * * * *"))
def my_sensor():
    return (True, {})

@task(inputs={"data": my_sensor}, freshness=Always())
def my_task(data):
    print(data)
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert_eq!(nodes[0].kind, NodeKind::Sensor);
        assert_eq!(
            nodes[0].freshness,
            Freshness::Schedule(CronExpr("*/5 * * * *".into()))
        );
        assert_eq!(nodes[1].kind, NodeKind::Task);
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

    // ═══════════════════════════════════════════════════════════════════════
    // Parallel call extraction
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn parallel_static_partials_extracted() {
        let src = r#"
@task()
def deploy_all():
    parallel(partial(deploy_us, model), partial(deploy_eu, model))
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert_eq!(nodes[0].parallel_calls.len(), 1);
        assert!(!nodes[0].parallel_calls[0].is_dynamic);
        assert_eq!(nodes[0].parallel_calls[0].static_refs.len(), 2);
    }

    #[test]
    fn parallel_dynamic_generator_extracted() {
        let src = r#"
@task()
def deploy_all():
    parallel(*(partial(deploy, r) for r in regions))
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert_eq!(nodes[0].parallel_calls.len(), 1);
        assert!(nodes[0].parallel_calls[0].is_dynamic);
        assert_eq!(nodes[0].parallel_calls[0].static_refs.len(), 1); // knows the function
    }

    #[test]
    fn parallel_map_extracted() {
        let src = r#"
@task()
def deploy_all():
    parallel_map(deploy, regions)
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert_eq!(nodes[0].parallel_calls.len(), 1);
        assert!(nodes[0].parallel_calls[0].is_dynamic);
        assert_eq!(nodes[0].parallel_calls[0].static_refs.len(), 1);
    }

    #[test]
    fn parallel_fully_dynamic_extracted() {
        let src = r#"
@task()
def release():
    parallel(*work_items)
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert_eq!(nodes[0].parallel_calls.len(), 1);
        assert!(nodes[0].parallel_calls[0].is_dynamic);
        assert!(nodes[0].parallel_calls[0].static_refs.is_empty());
    }

    #[test]
    fn parallel_not_extracted_from_assets() {
        let src = r#"
@asset()
def compute():
    parallel(partial(a), partial(b))
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert!(nodes[0].parallel_calls.is_empty());
    }

    #[test]
    fn parallel_multiple_calls_in_body() {
        let src = r#"
@task()
def pipeline():
    parallel(partial(a), partial(b))
    parallel(partial(c), partial(d))
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert_eq!(nodes[0].parallel_calls.len(), 2);
    }

    #[test]
    fn parallel_inside_if_else() {
        let src = r#"
@task()
def conditional():
    if True:
        parallel(partial(a), partial(b))
"#;
        let nodes = extract_nodes(src, "test.py").unwrap();
        assert_eq!(nodes[0].parallel_calls.len(), 1);
    }
}
