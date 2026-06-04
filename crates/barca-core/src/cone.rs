//! Dependency cone analysis — computes the transitive set of module-level
//! definitions that a function references.
//!
//! Pure function: (source, function_name) → cone hash string.
//! Uses ruff's AST for name resolution within a single file.

use ruff_python_ast::{Expr, Stmt};
use ruff_python_parser::parse_module;
use ruff_text_size::Ranged;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// Compute the dependency cone hash for a function within a source file.
///
/// Walks the function body, finds all names that resolve to module-level
/// definitions (helpers, constants), traces them transitively, and hashes
/// the combined source text.
///
/// Returns empty string if the function has no module-level dependencies.
pub fn cone_hash(source: &str, function_name: &str) -> String {
    let defs = collect_module_definitions(source);
    let Some(target) = defs.get(function_name) else {
        return String::new();
    };

    let refs = match target {
        ModuleDef::Function { references, .. } => references,
        _ => return String::new(),
    };

    // BFS through transitive dependencies.
    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = refs
        .iter()
        .filter(|r| defs.contains_key(r.as_str()))
        .cloned()
        .collect();

    let mut cone_parts: Vec<(String, String)> = Vec::new();

    while let Some(name) = queue.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        let Some(def) = defs.get(&name) else {
            continue;
        };
        match def {
            ModuleDef::Function {
                source_text,
                references,
            } => {
                cone_parts.push((name.clone(), source_text.clone()));
                for r in references {
                    if !visited.contains(r) && defs.contains_key(r.as_str()) {
                        queue.push(r.clone());
                    }
                }
            }
            ModuleDef::Assignment {
                source_text,
                references,
            } => {
                cone_parts.push((name.clone(), source_text.clone()));
                for r in references {
                    if !visited.contains(r) && defs.contains_key(r.as_str()) {
                        queue.push(r.clone());
                    }
                }
            }
            ModuleDef::Import { .. } => {
                cone_parts.push((name.clone(), name.clone()));
            }
        }
    }

    if cone_parts.is_empty() {
        return String::new();
    }

    // Sort by name for determinism, then hash.
    cone_parts.sort_by_key(|(name, _)| name.clone());
    let mut hasher = Sha256::new();
    for (name, text) in &cone_parts {
        hasher.update(name.as_bytes());
        hasher.update(b":");
        hasher.update(text.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

// ─── Internal types ──────────────────────────────────────────────────────────

enum ModuleDef {
    Function {
        source_text: String,
        references: HashSet<String>,
    },
    Assignment {
        source_text: String,
        references: HashSet<String>,
    },
    #[allow(dead_code)]
    Import { module: String },
}

// ─── Module definition collection ────────────────────────────────────────────

fn collect_module_definitions(source: &str) -> HashMap<String, ModuleDef> {
    let Ok(parsed) = parse_module(source) else {
        return HashMap::new();
    };

    let module = parsed.into_syntax();
    let mut defs: HashMap<String, ModuleDef> = HashMap::new();

    for stmt in &module.body {
        match stmt {
            Stmt::FunctionDef(func) => {
                let name = func.name.to_string();
                let start = func.range().start().to_usize();
                let end = func.range().end().to_usize();
                let source_text = source[start..end].to_string();
                let references = collect_names_from_stmts(&func.body);
                defs.insert(
                    name,
                    ModuleDef::Function {
                        source_text,
                        references,
                    },
                );
            }
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    if let Expr::Name(n) = target {
                        let name = n.id.to_string();
                        let start = assign.range().start().to_usize();
                        let end = assign.range().end().to_usize();
                        let source_text = source[start..end].to_string();
                        let references = collect_names_from_expr(&assign.value);
                        defs.insert(
                            name,
                            ModuleDef::Assignment {
                                source_text,
                                references,
                            },
                        );
                    }
                }
            }
            Stmt::AnnAssign(assign) => {
                if let Expr::Name(n) = assign.target.as_ref() {
                    let name = n.id.to_string();
                    let start = assign.range().start().to_usize();
                    let end = assign.range().end().to_usize();
                    let source_text = source[start..end].to_string();
                    let references = assign
                        .value
                        .as_ref()
                        .map(|v| collect_names_from_expr(v))
                        .unwrap_or_default();
                    defs.insert(
                        name,
                        ModuleDef::Assignment {
                            source_text,
                            references,
                        },
                    );
                }
            }
            Stmt::ImportFrom(import) => {
                let module_name = import
                    .module
                    .as_ref()
                    .map(|m| m.to_string())
                    .unwrap_or_default();
                for alias in &import.names {
                    let imported_name = alias
                        .asname
                        .as_ref()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| alias.name.to_string());
                    defs.insert(
                        imported_name,
                        ModuleDef::Import {
                            module: module_name.clone(),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    defs
}

// ─── Name collection from AST ────────────────────────────────────────────────

fn collect_names_from_stmts(stmts: &[Stmt]) -> HashSet<String> {
    let mut names = HashSet::new();
    for stmt in stmts {
        collect_stmt_names(stmt, &mut names);
    }
    names
}

fn collect_names_from_expr(expr: &Expr) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_expr_names(expr, &mut names);
    names
}

fn collect_stmt_names(stmt: &Stmt, names: &mut HashSet<String>) {
    match stmt {
        Stmt::Expr(s) => collect_expr_names(&s.value, names),
        Stmt::Return(s) => {
            if let Some(v) = &s.value {
                collect_expr_names(v, names);
            }
        }
        Stmt::Assign(s) => {
            collect_expr_names(&s.value, names);
        }
        Stmt::AnnAssign(s) => {
            if let Some(v) = &s.value {
                collect_expr_names(v, names);
            }
        }
        Stmt::If(s) => {
            collect_expr_names(&s.test, names);
            for st in &s.body {
                collect_stmt_names(st, names);
            }
            for clause in &s.elif_else_clauses {
                if let Some(test) = &clause.test {
                    collect_expr_names(test, names);
                }
                for st in &clause.body {
                    collect_stmt_names(st, names);
                }
            }
        }
        Stmt::For(s) => {
            collect_expr_names(&s.iter, names);
            for st in &s.body {
                collect_stmt_names(st, names);
            }
        }
        Stmt::While(s) => {
            collect_expr_names(&s.test, names);
            for st in &s.body {
                collect_stmt_names(st, names);
            }
        }
        Stmt::With(s) => {
            for item in &s.items {
                collect_expr_names(&item.context_expr, names);
            }
            for st in &s.body {
                collect_stmt_names(st, names);
            }
        }
        Stmt::FunctionDef(s) => {
            for st in &s.body {
                collect_stmt_names(st, names);
            }
        }
        _ => {}
    }
}

fn collect_expr_names(expr: &Expr, names: &mut HashSet<String>) {
    match expr {
        Expr::Name(n) => {
            names.insert(n.id.to_string());
        }
        Expr::Call(c) => {
            collect_expr_names(&c.func, names);
            for arg in c.arguments.args.iter() {
                collect_expr_names(arg, names);
            }
            for kw in c.arguments.keywords.iter() {
                collect_expr_names(&kw.value, names);
            }
        }
        Expr::Attribute(a) => collect_expr_names(&a.value, names),
        Expr::Subscript(s) => {
            collect_expr_names(&s.value, names);
            collect_expr_names(&s.slice, names);
        }
        Expr::BinOp(b) => {
            collect_expr_names(&b.left, names);
            collect_expr_names(&b.right, names);
        }
        Expr::UnaryOp(u) => collect_expr_names(&u.operand, names),
        Expr::BoolOp(b) => {
            for v in &b.values {
                collect_expr_names(v, names);
            }
        }
        Expr::Compare(c) => {
            collect_expr_names(&c.left, names);
            for v in &c.comparators {
                collect_expr_names(v, names);
            }
        }
        Expr::If(i) => {
            collect_expr_names(&i.test, names);
            collect_expr_names(&i.body, names);
            collect_expr_names(&i.orelse, names);
        }
        Expr::Dict(d) => {
            for item in &d.items {
                if let Some(k) = &item.key {
                    collect_expr_names(k, names);
                }
                collect_expr_names(&item.value, names);
            }
        }
        Expr::List(l) => {
            for e in &l.elts {
                collect_expr_names(e, names);
            }
        }
        Expr::Tuple(t) => {
            for e in &t.elts {
                collect_expr_names(e, names);
            }
        }
        Expr::Lambda(l) => collect_expr_names(&l.body, names),
        Expr::Starred(s) => collect_expr_names(&s.value, names),
        _ => {}
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_deps() {
        let src = r#"
from barca import asset

@asset()
def simple():
    return {"value": 1}
"#;
        let h = cone_hash(src, "simple");
        assert!(h.is_empty()); // no module-level deps
    }

    #[test]
    fn test_helper_function() {
        let src = r#"
def compute(x):
    return x * 2

def my_asset():
    return compute(21)
"#;
        let h = cone_hash(src, "my_asset");
        assert!(!h.is_empty()); // depends on compute
    }

    #[test]
    fn test_global_constant() {
        let src = r#"
THRESHOLD = 0.5

def check():
    return THRESHOLD > 0.3
"#;
        let h = cone_hash(src, "check");
        assert!(!h.is_empty()); // depends on THRESHOLD
    }

    #[test]
    fn test_transitive_deps() {
        let src = r#"
def step_a():
    return 1

def step_b():
    return step_a() + 1

def result():
    return step_b() + 1
"#;
        let h = cone_hash(src, "result");
        assert!(!h.is_empty());
        // Changing step_a should change the cone hash
        let src2 = src.replace("return 1", "return 99");
        let h2 = cone_hash(&src2, "result");
        assert_ne!(h, h2);
    }

    #[test]
    fn test_helper_change_changes_hash() {
        let src1 = r#"
def helper():
    return 1

def my_asset():
    return helper()
"#;
        let src2 = r#"
def helper():
    return 999

def my_asset():
    return helper()
"#;
        let h1 = cone_hash(src1, "my_asset");
        let h2 = cone_hash(src2, "my_asset");
        assert_ne!(h1, h2); // helper changed → cone hash changed
    }

    #[test]
    fn test_constant_change_changes_hash() {
        let src1 = r#"
THRESHOLD = 0.5

def check():
    return THRESHOLD > 0.3
"#;
        let src2 = r#"
THRESHOLD = 100

def check():
    return THRESHOLD > 0.3
"#;
        let h1 = cone_hash(src1, "check");
        let h2 = cone_hash(src2, "check");
        assert_ne!(h1, h2); // constant changed → cone hash changed
    }

    #[test]
    fn test_unrelated_change_no_effect() {
        let src1 = r#"
def unrelated():
    return "not used"

def my_asset():
    return {"value": 1}
"#;
        let src2 = r#"
def unrelated():
    return "changed but irrelevant"

def my_asset():
    return {"value": 1}
"#;
        let h1 = cone_hash(src1, "my_asset");
        let h2 = cone_hash(src2, "my_asset");
        assert_eq!(h1, h2); // unrelated function changed → no effect
    }

    #[test]
    fn test_deterministic() {
        let src = r#"
def helper():
    return 42

def my_asset():
    return helper()
"#;
        let h1 = cone_hash(src, "my_asset");
        let h2 = cone_hash(src, "my_asset");
        assert_eq!(h1, h2);
    }
}
