//! Cache checking — run_hash computation and partition-aligned lookups.

use std::collections::HashMap;

/// Compute the run_hash for a step given its context.
pub fn compute_run_hash(
    def_hash: &str,
    partition_key: Option<&str>,
    upstream_ids: impl Iterator<Item = impl AsRef<str>>,
    cached_run_hashes: &HashMap<String, String>,
) -> String {
    let upstream_hashes: Vec<&str> = upstream_ids
        .filter_map(|uid| {
            let uid = uid.as_ref();
            if let Some(h) = cached_run_hashes.get(uid) {
                return Some(h.as_str());
            }
            if let Some(pk) = partition_key {
                let aligned = format!("{uid}[{pk}]");
                if let Some(h) = cached_run_hashes.get(&aligned) {
                    return Some(h.as_str());
                }
            }
            None
        })
        .collect();
    crate::hash::run_hash(def_hash, partition_key, &upstream_hashes, None)
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

        let h1 = compute_run_hash("def_abc", None, ["upstream".to_string()].iter(), &hashes);
        let h2 = compute_run_hash("def_abc", None, ["upstream".to_string()].iter(), &hashes);
        assert_eq!(h1, h2);
    }

    #[test]
    fn compute_run_hash_changes_with_partition() {
        let hashes = HashMap::new();
        let h1 = compute_run_hash("def_abc", None, std::iter::empty::<&String>(), &hashes);
        let h2 = compute_run_hash(
            "def_abc",
            Some("t=X"),
            std::iter::empty::<&String>(),
            &hashes,
        );
        assert_ne!(h1, h2);
    }
}
