use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Coverage information for a single file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileCoverage {
    /// Digest of the file contents used to invalidate stale coverage.
    pub digest: String,
    /// Set of covered line numbers.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub lines: BTreeSet<u32>,
}

/// Mapping from file paths to coverage information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(transparent)]
pub struct CoverageMap(pub HashMap<String, FileCoverage>);

impl CoverageMap {
    /// Create an empty coverage map.
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Merge another coverage map into this one.
    ///
    /// If a file's digest has changed, the existing coverage is replaced.
    pub fn merge(&mut self, other: &CoverageMap) {
        for (path, new_cov) in &other.0 {
            let entry = self
                .0
                .entry(path.clone())
                .or_insert_with(|| FileCoverage {
                    digest: new_cov.digest.clone(),
                    lines: BTreeSet::new(),
                });

            if entry.digest != new_cov.digest {
                // File changed, replace coverage.
                entry.digest = new_cov.digest.clone();
                entry.lines.clear();
            }

            entry.lines.extend(&new_cov.lines);
        }
    }

    /// Load a coverage map from a JSON file.
    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read_to_string(path).context("failed to read coverage map")?;
        let map: CoverageMap = serde_json::from_str(&data).context("invalid coverage map json")?;
        Ok(map)
    }

    /// Save a coverage map to a JSON file.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("failed to create coverage map directory")?;
        }
        let data = serde_json::to_string_pretty(self).context("failed to serialize coverage map")?;
        std::fs::write(path, data).context("failed to write coverage map")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fc(digest: &str, lines: &[u32]) -> FileCoverage {
        FileCoverage {
            digest: digest.to_string(),
            lines: lines.iter().copied().collect(),
        }
    }

    #[test]
    fn merge_idempotent() {
        let mut base = CoverageMap(HashMap::from([(
            "file.py".to_string(),
            fc("d1", &[1, 2]),
        )]));
        let delta = CoverageMap(HashMap::from([(
            "file.py".to_string(),
            fc("d1", &[2, 3]),
        )]));

        let mut merged_once = base.clone();
        merged_once.merge(&delta);

        let mut merged_twice = merged_once.clone();
        merged_twice.merge(&delta);

        assert_eq!(merged_once, merged_twice);
    }

    #[test]
    fn merge_associative() {
        let a = CoverageMap(HashMap::from([(
            "f.py".to_string(),
            fc("d", &[1]),
        )]));
        let b = CoverageMap(HashMap::from([(
            "f.py".to_string(),
            fc("d", &[2]),
        )]));
        let c = CoverageMap(HashMap::from([(
            "f.py".to_string(),
            fc("d", &[3]),
        )]));

        let mut left = a.clone();
        left.merge(&b);
        left.merge(&c);

        let mut right = a.clone();
        let mut bc = b.clone();
        bc.merge(&c);
        right.merge(&bc);

        assert_eq!(left, right);
    }

    #[test]
    fn digest_change_replaces_coverage() {
        let mut base = CoverageMap(HashMap::from([(
            "file.py".to_string(),
            fc("d1", &[1, 2]),
        )]));
        let delta = CoverageMap(HashMap::from([(
            "file.py".to_string(),
            fc("d2", &[5]),
        )]));

        base.merge(&delta);
        assert_eq!(base.0["file.py"].digest, "d2");
        assert_eq!(base.0["file.py"].lines, BTreeSet::from([5]));
    }
}
