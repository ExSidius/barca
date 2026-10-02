//! Cache checking — run_hash computation and partition-aligned lookups.

use std::collections::HashMap;

/// Compute the run_hash for a step given its context. `env` is the step's declared env values
/// ([`crate::envdeps::hash_input`]); `None` leaves the hash exactly as a node without `env=`.
pub fn compute_run_hash(
    def_hash: &str,
    partition_key: Option<&str>,
    upstream_ids: impl Iterator<Item = impl AsRef<str>>,
    cached_run_hashes: &HashMap<String, String>,
    env: Option<&str>,
) -> String {
    let mut upstream_hashes: Vec<String> = Vec::new();
    for uid in upstream_ids {
        let uid = uid.as_ref();
        if let Some(h) = cached_run_hashes.get(uid) {
            upstream_hashes.push(h.clone());
            continue;
        }
        // Try partition-aligned lookup (same partition as current step).
        if let Some(pk) = partition_key {
            let aligned = format!("{uid}[{pk}]");
            if let Some(h) = cached_run_hashes.get(&aligned) {
                upstream_hashes.push(h.clone());
                continue;
            }
        }
        // Fan-in: collect ALL partition hashes for this base ID (sorted for determinism).
        let prefix = format!("{uid}[");
        let mut partition_hashes: Vec<(&String, &String)> = cached_run_hashes
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .collect();
        if !partition_hashes.is_empty() {
            partition_hashes.sort_by_key(|(k, _)| (*k).clone());
            for (_, h) in partition_hashes {
                upstream_hashes.push(h.clone());
            }
        }
    }
    let hash_refs: Vec<&str> = upstream_hashes.iter().map(|s| s.as_str()).collect();
    crate::hash::run_hash(def_hash, partition_key, &hash_refs, None, env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PartitionKey, StepId};

    #[test]
    fn step_id_parse_unpartitioned() {
        let sid = StepId::parse("test.py:foo");
        assert_eq!(sid.base_id(), "test.py:foo");
        assert!(sid.partition.is_empty());
        assert_eq!(sid.display(), "test.py:foo");
    }

    #[test]
    fn step_id_parse_partitioned() {
        let sid = StepId::parse("test.py:foo[region=us]");
        assert_eq!(sid.base_id(), "test.py:foo");
        assert_eq!(sid.partition.0.get("region").unwrap(), "us");
        assert_eq!(sid.display(), "test.py:foo[region=us]");
    }

    #[test]
    fn step_id_round_trip() {
        let pk = PartitionKey::from(HashMap::from([
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ]));
        let sid = StepId::new("f:x", pk);
        let display = sid.display();
        let parsed = StepId::parse(&display);
        assert_eq!(parsed.base_id(), "f:x");
        assert_eq!(parsed.partition, sid.partition);
    }

    #[test]
    fn compute_run_hash_deterministic() {
        let mut hashes = HashMap::new();
        hashes.insert("upstream".to_string(), "h_up".to_string());

        let h1 = compute_run_hash(
            "def_abc",
            None,
            ["upstream".to_string()].iter(),
            &hashes,
            None,
        );
        let h2 = compute_run_hash(
            "def_abc",
            None,
            ["upstream".to_string()].iter(),
            &hashes,
            None,
        );
        assert_eq!(h1, h2);
    }

    /// Pinned run hashes computed by barca <= 0.9.0 (before declared env existed). A node that
    /// declares no env must keep exactly these hashes, or every existing cache is invalidated.
    #[test]
    fn run_hash_unchanged_for_nodes_without_env() {
        let mut hashes = HashMap::new();
        hashes.insert("upstream".to_string(), "h_up".to_string());
        assert_eq!(
            compute_run_hash(
                "def_abc",
                None,
                ["upstream".to_string()].iter(),
                &hashes,
                None
            ),
            "bc7c6531d8fe3452c9a9ac36fef43665103624231e112b5d87bf20376e2e9288"
        );
        assert_eq!(
            compute_run_hash(
                "def_abc",
                Some("t=X"),
                std::iter::empty::<&String>(),
                &HashMap::new(),
                None
            ),
            "7a4d6ec23915f304b5520de445fd04343119f46d664c9737a7edb7ae11babc5d"
        );
    }

    /// Run hashes of whole pipelines, pinned from barca 0.10.0 (before #178 taught the cone
    /// analysis about bare filenames and `import module` + `module.attr`). Pipelines that use no
    /// project helpers, or that import them with `from ... import` and were run as `./p.py` or by
    /// absolute path, must keep exactly these hashes, or every existing cache is invalidated.
    #[test]
    fn run_hash_unchanged_for_pipelines_without_module_attribute_helpers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("p.py"),
            r#"import json
from barca import asset
from helpers import compute
from utils.maths import double

RATE = 2


def local_helper(x):
    return x * RATE


@asset()
def plain() -> dict:
    return {"v": 1}


@asset()
def uses_local() -> int:
    return local_helper(3)


@asset()
def uses_from() -> int:
    return compute()


@asset()
def uses_subdir() -> int:
    return double(2)


@asset()
def uses_stdlib_module() -> str:
    return json.dumps({"a": 1})


@asset(inputs={"x": uses_from})
def downstream(x: int) -> int:
    return x + 1
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("helpers.py"),
            "def compute():\n    return 1\n\n\ndef unrelated():\n    return 0\n",
        )
        .unwrap();
        std::fs::create_dir(root.join("utils")).unwrap();
        std::fs::write(
            root.join("utils/maths.py"),
            "def double(x):\n    return x * 2\n",
        )
        .unwrap();

        let file = root.join("p.py").to_string_lossy().to_string();
        let dag = crate::commands::build_dag_blocking(
            std::slice::from_ref(&file),
            &std::path::PathBuf::from("python3"),
        )
        .unwrap();
        let mut hashes: HashMap<String, String> = HashMap::new();
        let mut by_name: Vec<(String, String)> = Vec::new();
        for id in dag.topo_order() {
            let node = dag.get_node(id).unwrap();
            let h = compute_run_hash(
                &node.definition_hash,
                None,
                dag.upstream(id).into_iter(),
                &hashes,
                None,
            );
            hashes.insert(id.to_string(), h.clone());
            by_name.push((node.extracted.function_name.clone(), h));
        }
        by_name.sort();
        let got: Vec<String> = by_name.iter().map(|(n, h)| format!("{n} {h}")).collect();
        assert_eq!(
            got,
            [
                "downstream 1c56f2327b95052d761eb9f938de94d7bf6ceb4963f94c5ae7269d8bb70c56e0",
                "plain 951e243083668324e552a72ac14fdaa7a44643e4e3ef612bae42ca99a012f4cd",
                "uses_from 3cfd70e1f1c429d785cbca598dd6c658c516419b1db74fedd6da1f98f5f91b3c",
                "uses_local 20c98bdcb5158223e28ad33f1eb2374383b20e07b6241c31bb6c0d93d90191d9",
                "uses_stdlib_module 38349c33e82697699115193940df0487e7ee4a242b747e20f7b2ddbc153052f4",
                "uses_subdir 6ef456461f6549fe2b0b99a8cacd7ac5eb069530470b5574aac096cfb6b013de",
            ]
        );
    }

    #[test]
    fn declared_env_changes_the_run_hash() {
        use crate::envdeps::{hash_input, resolve_with};
        let names = vec!["SOURCE_CSV".to_string()];
        let hashes = HashMap::new();
        let h = |v: Option<&str>| {
            let vals = resolve_with(&names, |_| v.map(String::from));
            compute_run_hash(
                "def_abc",
                None,
                std::iter::empty::<&String>(),
                &hashes,
                hash_input(&vals).as_deref(),
            )
        };
        let none = compute_run_hash(
            "def_abc",
            None,
            std::iter::empty::<&String>(),
            &hashes,
            None,
        );
        assert_ne!(h(Some("/a.csv")), h(Some("/b.csv")));
        assert_ne!(h(None), h(Some("")));
        assert_ne!(
            h(None),
            none,
            "declaring a variable (even unset) is part of the identity"
        );
        assert_eq!(h(Some("/a.csv")), h(Some("/a.csv")));
    }

    #[test]
    fn compute_run_hash_changes_with_partition() {
        let hashes = HashMap::new();
        let h1 = compute_run_hash(
            "def_abc",
            None,
            std::iter::empty::<&String>(),
            &hashes,
            None,
        );
        let h2 = compute_run_hash(
            "def_abc",
            Some("t=X"),
            std::iter::empty::<&String>(),
            &hashes,
            None,
        );
        assert_ne!(h1, h2);
    }
}
