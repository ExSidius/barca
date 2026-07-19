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
/// definitions (helpers, constants, imports), traces them transitively, and
/// hashes the combined source text.
///
/// `other_sources` maps module names to their source content (for cross-file resolution).
/// e.g., `{"helpers": "def compute(x): return x * 2\n"}`.
///
/// Returns empty string if the function has no module-level dependencies.
pub fn cone_hash(source: &str, function_name: &str) -> String {
    cone_hash_with_imports(source, function_name, &HashMap::new())
}

/// Cone hash with cross-file import resolution.
pub fn cone_hash_with_imports(
    source: &str,
    function_name: &str,
    other_sources: &HashMap<String, ModuleSource>,
) -> String {
    let defs = collect_module_definitions(source);
    cone_hash_from_defs(&defs, function_name, other_sources)
}

/// Cone hash from pre-parsed module definitions. Avoids re-parsing the source
/// when computing hashes for multiple functions in the same file.
pub fn cone_hash_from_defs(
    defs: &HashMap<String, ModuleDef>,
    function_name: &str,
    other_sources: &HashMap<String, ModuleSource>,
) -> String {
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
            ModuleDef::Import { module } => {
                // Resolve cross-file import, following re-export chains.
                resolve_import(
                    module,
                    &name,
                    other_sources,
                    &mut visited,
                    &mut cone_parts,
                    0, // depth limit to prevent infinite loops
                );
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

/// A cross-file-importable module's source, plus whether it's a package
/// (`__init__.py`) — determines how relative imports (`from .x`, `from ..x`)
/// resolve when tracing into it (see `resolve_relative_module`).
#[derive(Clone)]
pub struct ModuleSource {
    pub content: String,
    pub is_package: bool,
}

pub enum ModuleDef {
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

/// Resolve a cross-file import, following re-export chains up to a depth limit.
fn resolve_import(
    module: &str,
    name: &str,
    other_sources: &HashMap<String, ModuleSource>,
    visited: &mut HashSet<String>,
    cone_parts: &mut Vec<(String, String)>,
    depth: usize,
) {
    if depth > 5 {
        cone_parts.push((name.to_string(), format!("import:{module}:{name}")));
        return;
    }

    let Some(module_source) = other_sources.get(module) else {
        cone_parts.push((name.to_string(), format!("import:{module}:{name}")));
        return;
    };

    let imported_defs = collect_module_definitions_with_context(
        &module_source.content,
        Some(module),
        module_source.is_package,
    );
    let Some(imported_def) = imported_defs.get(name) else {
        cone_parts.push((name.to_string(), format!("import:{module}:{name}")));
        return;
    };

    match imported_def {
        ModuleDef::Function {
            source_text,
            references,
        }
        | ModuleDef::Assignment {
            source_text,
            references,
        } => {
            cone_parts.push((format!("{module}:{name}"), source_text.clone()));
            // Trace transitive deps within the module.
            let mut bfs_queue: Vec<String> = references
                .iter()
                .filter(|r| imported_defs.contains_key(r.as_str()))
                .cloned()
                .collect();
            while let Some(dep) = bfs_queue.pop() {
                let dep_key = format!("{module}:{dep}");
                if !visited.insert(dep_key.clone()) {
                    continue;
                }
                if let Some(
                    ModuleDef::Function {
                        source_text,
                        references,
                    }
                    | ModuleDef::Assignment {
                        source_text,
                        references,
                    },
                ) = imported_defs.get(&dep)
                {
                    cone_parts.push((dep_key, source_text.clone()));
                    for r in references {
                        if !visited.contains(&format!("{module}:{r}"))
                            && imported_defs.contains_key(r.as_str())
                        {
                            bfs_queue.push(r.clone());
                        }
                    }
                }
            }
        }
        ModuleDef::Import {
            module: next_module,
        } => {
            // Re-export chain: follow to the next module recursively.
            resolve_import(
                next_module,
                name,
                other_sources,
                visited,
                cone_parts,
                depth + 1,
            );
        }
    }
}

// ─── Module definition collection ────────────────────────────────────────────

/// Resolve a relative import's target module, following Python's semantics:
/// `level` dots climb from the *current module's package* — which is the
/// module's own dotted name if it's a package's `__init__.py`, or one level
/// up (its name minus the last segment) if it's a regular submodule. Each
/// additional level beyond the first climbs one more segment. See issue #63.
fn resolve_relative_module(
    module_name: &str,
    is_package: bool,
    level: u32,
    raw_module: &str,
) -> String {
    let mut segments: Vec<&str> = module_name.split('.').collect();
    if !is_package {
        segments.pop();
    }
    let climb = (level - 1) as usize;
    let new_len = segments.len().saturating_sub(climb);
    segments.truncate(new_len);
    let base = segments.join(".");
    if raw_module.is_empty() {
        base
    } else if base.is_empty() {
        raw_module.to_string()
    } else {
        format!("{base}.{raw_module}")
    }
}

pub fn collect_module_definitions(source: &str) -> HashMap<String, ModuleDef> {
    collect_module_definitions_with_context(source, None, false)
}

fn collect_module_definitions_with_context(
    source: &str,
    parent_module: Option<&str>,
    is_package: bool,
) -> HashMap<String, ModuleDef> {
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
                let raw_module = import
                    .module
                    .as_ref()
                    .map(|m| m.to_string())
                    .unwrap_or_default();
                // Resolve relative imports: `from .core import x` with level=1
                // becomes `parent_module.core` when we know the parent.
                let module_name = if import.level > 0 {
                    if let Some(parent) = parent_module {
                        resolve_relative_module(parent, is_package, import.level, &raw_module)
                    } else {
                        raw_module
                    }
                } else {
                    raw_module
                };
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
        Stmt::AugAssign(s) => {
            collect_expr_names(&s.target, names);
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
        Stmt::Try(s) => {
            for st in &s.body {
                collect_stmt_names(st, names);
            }
            for handler in &s.handlers {
                if let Some(h) = handler.as_except_handler() {
                    for st in &h.body {
                        collect_stmt_names(st, names);
                    }
                }
            }
            for st in &s.orelse {
                collect_stmt_names(st, names);
            }
            for st in &s.finalbody {
                collect_stmt_names(st, names);
            }
        }
        Stmt::Match(s) => {
            collect_expr_names(&s.subject, names);
            for case in &s.cases {
                if let Some(guard) = &case.guard {
                    collect_expr_names(guard, names);
                }
                for st in &case.body {
                    collect_stmt_names(st, names);
                }
            }
        }
        Stmt::ClassDef(s) => {
            for arg in s.arguments.iter().flat_map(|a| a.args.iter()) {
                collect_expr_names(arg, names);
            }
            for st in &s.body {
                collect_stmt_names(st, names);
            }
        }
        Stmt::Raise(s) => {
            if let Some(exc) = &s.exc {
                collect_expr_names(exc, names);
            }
            if let Some(cause) = &s.cause {
                collect_expr_names(cause, names);
            }
        }
        Stmt::Assert(s) => {
            collect_expr_names(&s.test, names);
            if let Some(msg) = &s.msg {
                collect_expr_names(msg, names);
            }
        }
        Stmt::Delete(s) => {
            for target in &s.targets {
                collect_expr_names(target, names);
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
        Expr::ListComp(c) => {
            collect_expr_names(&c.elt, names);
            for comp in &c.generators {
                collect_expr_names(&comp.iter, names);
                for cond in &comp.ifs {
                    collect_expr_names(cond, names);
                }
            }
        }
        Expr::SetComp(c) => {
            collect_expr_names(&c.elt, names);
            for comp in &c.generators {
                collect_expr_names(&comp.iter, names);
                for cond in &comp.ifs {
                    collect_expr_names(cond, names);
                }
            }
        }
        Expr::DictComp(c) => {
            if let Some(k) = &c.key {
                collect_expr_names(k, names);
            }
            collect_expr_names(&c.value, names);
            for comp in &c.generators {
                collect_expr_names(&comp.iter, names);
                for cond in &comp.ifs {
                    collect_expr_names(cond, names);
                }
            }
        }
        Expr::Generator(g) => {
            collect_expr_names(&g.elt, names);
            for comp in &g.generators {
                collect_expr_names(&comp.iter, names);
                for cond in &comp.ifs {
                    collect_expr_names(cond, names);
                }
            }
        }
        Expr::FString(f) => {
            for part in &f.value {
                if let ruff_python_ast::FStringPart::FString(fstr) = part {
                    for interp in fstr.elements.interpolations() {
                        collect_expr_names(&interp.expression, names);
                    }
                }
            }
        }
        Expr::Named(n) => {
            collect_expr_names(&n.value, names);
        }
        Expr::Await(a) => collect_expr_names(&a.value, names),
        Expr::Yield(y) => {
            if let Some(v) = &y.value {
                collect_expr_names(v, names);
            }
        }
        Expr::YieldFrom(y) => collect_expr_names(&y.value, names),
        Expr::Slice(s) => {
            if let Some(l) = &s.lower {
                collect_expr_names(l, names);
            }
            if let Some(u) = &s.upper {
                collect_expr_names(u, names);
            }
            if let Some(st) = &s.step {
                collect_expr_names(st, names);
            }
        }
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

    // ─── Relative import resolution (issue #63) ────────────────────────────

    #[test]
    fn resolve_relative_module_package_level_1() {
        assert_eq!(
            resolve_relative_module("mylib", true, 1, "core"),
            "mylib.core"
        );
    }

    #[test]
    fn resolve_relative_module_submodule_level_1_uses_containing_package() {
        // Regression test for issue #63: a regular submodule's own dotted name
        // must be stripped before appending — the bug appended raw_module to
        // the submodule's own name (`mylib.core.util`) instead of its
        // containing package (`mylib.util`).
        assert_eq!(
            resolve_relative_module("mylib.core", false, 1, "util"),
            "mylib.util"
        );
    }

    #[test]
    fn resolve_relative_module_submodule_level_2_climbs_to_grandparent() {
        assert_eq!(
            resolve_relative_module("pkg.sub.core", false, 2, "util"),
            "pkg.util"
        );
    }

    #[test]
    fn resolve_relative_module_package_level_2_climbs_from_the_package_itself() {
        assert_eq!(
            resolve_relative_module("pkg.sub", true, 2, "util"),
            "pkg.util"
        );
    }

    #[test]
    fn resolve_relative_module_bare_dot_import_refers_to_containing_package() {
        // `from . import x` inside a regular submodule refers to its own package.
        assert_eq!(resolve_relative_module("mylib.core", false, 1, ""), "mylib");
        // `from . import x` inside a package's __init__.py refers to itself.
        assert_eq!(resolve_relative_module("mylib", true, 1, ""), "mylib");
    }

    #[test]
    fn resolve_relative_module_climb_beyond_top_level_degrades_to_bare_name() {
        // Climbing past the top-level package is an ImportError in real Python;
        // we degrade gracefully to the bare name rather than panicking.
        assert_eq!(resolve_relative_module("mylib", true, 2, "foo"), "foo");
    }

    #[test]
    fn relative_import_in_regular_submodule_resolves_to_sibling_not_self() {
        // End-to-end: `pkg/sub.py` re-exporting `from .util import h` must
        // resolve to `pkg.util`, so a change to `h`'s body invalidates the
        // cone hash of an asset that (transitively, through the re-export)
        // uses it — matching the coverage `test_relative_import_in_init` in
        // python/tests/test_cross_file.py already has for `__init__.py`.
        let top_source = r#"
from pkg.sub import h

def my_asset():
    return h()
"#;
        let mut other_sources = HashMap::new();
        other_sources.insert(
            "pkg.sub".to_string(),
            ModuleSource {
                content: "from .util import h\n".to_string(),
                is_package: false,
            },
        );
        other_sources.insert(
            "pkg.util".to_string(),
            ModuleSource {
                content: "def h():\n    return 1\n".to_string(),
                is_package: false,
            },
        );

        let h1 = cone_hash_with_imports(top_source, "my_asset", &other_sources);

        let mut other_sources2 = other_sources.clone();
        other_sources2.insert(
            "pkg.util".to_string(),
            ModuleSource {
                content: "def h():\n    return 999\n".to_string(),
                is_package: false,
            },
        );
        let h2 = cone_hash_with_imports(top_source, "my_asset", &other_sources2);

        assert_ne!(
            h1, h2,
            "change behind relative import in a regular submodule must invalidate the cone hash"
        );
    }

    #[test]
    fn relative_import_level_2_climbs_two_packages() {
        // `from ..util import h` (level=2) inside `pkg.sub.core` must resolve
        // to `pkg.util` (grandparent package), not `pkg.sub.util` (what a
        // level=1-only implementation would wrongly produce).
        let top_source = r#"
from pkg.sub.core import h

def my_asset():
    return h()
"#;
        let mut other_sources = HashMap::new();
        other_sources.insert(
            "pkg.sub.core".to_string(),
            ModuleSource {
                content: "from ..util import h\n".to_string(),
                is_package: false,
            },
        );
        other_sources.insert(
            "pkg.util".to_string(),
            ModuleSource {
                content: "def h():\n    return 1\n".to_string(),
                is_package: false,
            },
        );
        // A decoy at the level=1 (wrong) location — must not be the one used.
        other_sources.insert(
            "pkg.sub.util".to_string(),
            ModuleSource {
                content: "def h():\n    return -1\n".to_string(),
                is_package: false,
            },
        );

        let h1 = cone_hash_with_imports(top_source, "my_asset", &other_sources);

        let mut other_sources2 = other_sources.clone();
        other_sources2.insert(
            "pkg.util".to_string(),
            ModuleSource {
                content: "def h():\n    return 999\n".to_string(),
                is_package: false,
            },
        );
        let h2 = cone_hash_with_imports(top_source, "my_asset", &other_sources2);

        assert_ne!(
            h1, h2,
            "level=2 relative import must resolve to the grandparent package"
        );
    }
}
