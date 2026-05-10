use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

const TOPOLOGY_BRANCH: &str = "topology/v1";
const CLUSTERS_FILE: &str = ".clusters.json";
const INDEXED_SHA_FILE: &str = ".indexed-sha";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cluster {
    pub id: String,
    pub name: String,
    pub description: String,
    pub files: Vec<String>,
}

impl Cluster {
    pub fn contains_file(&self, file: &str) -> bool {
        self.files.iter().any(|f| f == file)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMap {
    pub version: u8,
    pub clusters: Vec<Cluster>,
}

impl ClusterMap {
    pub fn empty() -> Self {
        Self {
            version: 1,
            clusters: vec![],
        }
    }

    pub fn clusters_for_files(&self, files: &[String]) -> Vec<&Cluster> {
        self.clusters
            .iter()
            .filter(|c| files.iter().any(|f| c.contains_file(f)))
            .collect()
    }
}

pub fn is_stale(repo_path: &Path) -> bool {
    if !branch_exists(repo_path) {
        return true;
    }

    let indexed_sha = read_indexed_sha(repo_path);
    let indexed_sha = match indexed_sha {
        Some(s) => s,
        None => return true,
    };

    let head = Command::new("git")
        .current_dir(repo_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    let head = match head {
        Some(s) => s,
        None => return true,
    };

    if indexed_sha == head {
        return false;
    }

    // Check if any source file changed between indexed sha and HEAD
    let diff = Command::new("git")
        .current_dir(repo_path)
        .args(["diff", "--name-only", &indexed_sha, &head])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    diff.lines()
        .any(|f| crate::chunking::languages::detect_language(f).is_some())
}

fn read_indexed_sha(repo_path: &Path) -> Option<String> {
    let out = Command::new("git")
        .current_dir(repo_path)
        .args(["show", &format!("{}:{}", TOPOLOGY_BRANCH, INDEXED_SHA_FILE)])
        .output()
        .ok()?;

    if out.status.success() {
        String::from_utf8(out.stdout)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    }
}

pub fn branch_exists(repo_path: &Path) -> bool {
    Command::new("git")
        .current_dir(repo_path)
        .args(["rev-parse", "--verify", TOPOLOGY_BRANCH])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn read_cluster_map(repo_path: &Path) -> Result<Option<ClusterMap>> {
    if !branch_exists(repo_path) {
        return Ok(None);
    }

    let out = Command::new("git")
        .current_dir(repo_path)
        .args(["show", &format!("{}:{}", TOPOLOGY_BRANCH, CLUSTERS_FILE)])
        .output()
        .context("Failed to run git show for cluster map")?;

    if !out.status.success() {
        return Ok(None);
    }

    let map = serde_json::from_slice(&out.stdout).context("Failed to parse cluster map")?;
    Ok(Some(map))
}

pub fn write_cluster_map(repo_path: &Path, map: &ClusterMap) -> Result<()> {
    ensure_topology_branch(repo_path)?;

    let worktree_path = repo_path.join(".git").join("topology-worktree");
    setup_worktree(repo_path, &worktree_path, TOPOLOGY_BRANCH)?;

    let json = serde_json::to_string_pretty(map).context("Failed to serialize cluster map")?;
    std::fs::write(worktree_path.join(CLUSTERS_FILE), json)
        .context("Failed to write cluster map")?;

    let head_sha = Command::new("git")
        .current_dir(repo_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    std::fs::write(worktree_path.join(INDEXED_SHA_FILE), &head_sha)
        .context("Failed to write indexed sha")?;

    commit_worktree(repo_path, &worktree_path, "topology: update cluster map")?;
    remove_worktree(repo_path, &worktree_path)?;

    Ok(())
}

fn ensure_topology_branch(repo_path: &Path) -> Result<()> {
    if branch_exists(repo_path) {
        return Ok(());
    }

    let empty_tree = Command::new("git")
        .current_dir(repo_path)
        .args(["hash-object", "-t", "tree", "--stdin"])
        .stdin(std::process::Stdio::null())
        .output()
        .context("Failed to create empty tree")?;

    if !empty_tree.status.success() {
        anyhow::bail!(
            "Failed to create empty tree: {}",
            String::from_utf8_lossy(&empty_tree.stderr)
        );
    }

    let tree_sha = String::from_utf8_lossy(&empty_tree.stdout)
        .trim()
        .to_string();

    let commit = Command::new("git")
        .current_dir(repo_path)
        .args([
            "commit-tree",
            &tree_sha,
            "-m",
            "init: create topology branch",
        ])
        .output()
        .context("Failed to create initial commit")?;

    if !commit.status.success() {
        anyhow::bail!(
            "Failed to create initial commit: {}",
            String::from_utf8_lossy(&commit.stderr)
        );
    }

    let commit_sha = String::from_utf8_lossy(&commit.stdout).trim().to_string();

    let out = Command::new("git")
        .current_dir(repo_path)
        .args(["branch", TOPOLOGY_BRANCH, &commit_sha])
        .output()
        .context("Failed to create topology branch")?;

    if !out.status.success() {
        anyhow::bail!(
            "Failed to create branch: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    Ok(())
}

fn setup_worktree(repo_path: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    if worktree_path.exists() {
        Command::new("git")
            .current_dir(repo_path)
            .args([
                "worktree",
                "remove",
                "--force",
                worktree_path.to_str().unwrap(),
            ])
            .output()
            .ok();
        std::fs::remove_dir_all(worktree_path).ok();
        Command::new("git")
            .current_dir(repo_path)
            .args(["worktree", "prune"])
            .output()
            .ok();
    }

    let out = Command::new("git")
        .current_dir(repo_path)
        .args([
            "worktree",
            "add",
            "--no-checkout",
            worktree_path.to_str().unwrap(),
            branch,
        ])
        .output()
        .context("Failed to add topology worktree")?;

    if !out.status.success() {
        anyhow::bail!(
            "Failed to set up topology worktree: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    Command::new("git")
        .current_dir(worktree_path)
        .args(["checkout", branch, "--", "."])
        .output()
        .ok();

    Ok(())
}

fn commit_worktree(repo_path: &Path, worktree_path: &Path, message: &str) -> Result<()> {
    Command::new("git")
        .current_dir(worktree_path)
        .args(["add", "-A"])
        .output()
        .context("Failed to stage topology files")?;

    let status = Command::new("git")
        .current_dir(worktree_path)
        .args(["diff", "--cached", "--quiet"])
        .status()
        .context("Failed to check worktree status")?;

    if status.success() {
        return Ok(());
    }

    let head_sha = Command::new("git")
        .current_dir(repo_path)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let full_message = format!("{} ({})", message, head_sha);

    let out = Command::new("git")
        .current_dir(worktree_path)
        .args(["commit", "-m", &full_message])
        .output()
        .context("Failed to commit topology branch")?;

    if !out.status.success() {
        anyhow::bail!(
            "Failed to commit topology branch: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    Ok(())
}

fn remove_worktree(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    Command::new("git")
        .current_dir(repo_path)
        .args([
            "worktree",
            "remove",
            "--force",
            worktree_path.to_str().unwrap(),
        ])
        .output()
        .context("Failed to remove topology worktree")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cluster(id: &str, name: &str, files: &[&str]) -> Cluster {
        Cluster {
            id: id.to_string(),
            name: name.to_string(),
            description: String::new(),
            files: files.iter().map(|f| f.to_string()).collect(),
        }
    }

    #[test]
    fn cluster_contains_file() {
        let c = make_cluster("abc", "auth", &["src/auth/mod.rs", "src/auth/jwt.rs"]);
        assert!(c.contains_file("src/auth/mod.rs"));
        assert!(!c.contains_file("src/main.rs"));
    }

    #[test]
    fn cluster_map_empty() {
        let map = ClusterMap::empty();
        assert_eq!(map.version, 1);
        assert!(map.clusters.is_empty());
    }

    #[test]
    fn clusters_for_files_matches() {
        let map = ClusterMap {
            version: 1,
            clusters: vec![
                make_cluster("a1", "auth", &["src/auth/mod.rs", "src/auth/jwt.rs"]),
                make_cluster("b2", "db", &["src/db.rs", "src/models.rs"]),
            ],
        };

        let files = vec!["src/auth/jwt.rs".to_string()];
        let matched = map.clusters_for_files(&files);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].name, "auth");
    }

    #[test]
    fn clusters_for_files_no_match() {
        let map = ClusterMap {
            version: 1,
            clusters: vec![make_cluster("a1", "auth", &["src/auth/mod.rs"])],
        };

        let files = vec!["src/main.rs".to_string()];
        let matched = map.clusters_for_files(&files);
        assert!(matched.is_empty());
    }

    #[test]
    fn clusters_for_files_multi_cluster_match() {
        let map = ClusterMap {
            version: 1,
            clusters: vec![
                make_cluster("a1", "auth", &["src/auth/mod.rs"]),
                make_cluster("b2", "db", &["src/db.rs"]),
            ],
        };

        let files = vec!["src/auth/mod.rs".to_string(), "src/db.rs".to_string()];
        let matched = map.clusters_for_files(&files);
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn cluster_map_serialization_roundtrip() {
        let map = ClusterMap {
            version: 1,
            clusters: vec![make_cluster(
                "abc123",
                "auth/middleware",
                &["src/auth/mod.rs"],
            )],
        };

        let json = serde_json::to_string(&map).unwrap();
        let parsed: ClusterMap = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.clusters.len(), 1);
        assert_eq!(parsed.clusters[0].id, "abc123");
        assert_eq!(parsed.clusters[0].name, "auth/middleware");
    }
}
