use anyhow::{Context, Result};
use fs2::FileExt;
use std::path::Path;
use walkdir::WalkDir;

use crate::chunking;
use crate::cluster::{is_stale, read_cluster_map, write_cluster_map, ClusterMap};
use crate::clustering::embed_and_cluster;
use crate::embeddings::{config::EmbeddingConfig, create_provider};

const IGNORED_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".next",
    "dist",
    "build",
    "__pycache__",
];

pub fn run_index(repo_path: &Path, config: EmbeddingConfig) -> Result<ClusterMap> {
    if !is_stale(repo_path) {
        return Ok(read_cluster_map(repo_path)?.unwrap_or_else(ClusterMap::empty));
    }

    let lock_path = repo_path.join(".git").join("topology.lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&lock_path)
        .context("Failed to open topology lock file")?;
    lock_file
        .lock_exclusive()
        .context("Failed to acquire topology lock")?;

    if !is_stale(repo_path) {
        lock_file.unlock().ok();
        return Ok(read_cluster_map(repo_path)?.unwrap_or_else(ClusterMap::empty));
    }

    let result = run_index_inner(repo_path, config);
    lock_file.unlock().ok();
    result
}

fn run_index_inner(repo_path: &Path, config: EmbeddingConfig) -> Result<ClusterMap> {
    let mut provider = create_provider(&config)?;
    provider.init()?;

    println!("Scanning files...");
    let file_texts = collect_file_texts(repo_path)?;

    if file_texts.is_empty() {
        println!("No supported source files found.");
        return Ok(ClusterMap::empty());
    }

    println!(
        "Embedding {} files with {}...",
        file_texts.len(),
        provider.provider_name()
    );
    let map =
        embed_and_cluster(file_texts, provider.as_mut()).context("Failed to build cluster map")?;

    println!("Writing {} clusters to topology/v1...", map.clusters.len());
    write_cluster_map(repo_path, &map).context("Failed to write cluster map")?;

    Ok(map)
}

fn collect_file_texts(repo_path: &Path) -> Result<Vec<(String, String)>> {
    let mut result = Vec::new();

    for entry in WalkDir::new(repo_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !IGNORED_DIRS.contains(&name.as_ref())
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let file_str = path.to_string_lossy();

        if chunking::languages::detect_language(&file_str).is_none() {
            continue;
        }

        let relative = path
            .strip_prefix(repo_path)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => continue,
        };

        if text.trim().is_empty() {
            continue;
        }

        let chunks = chunking::chunk_code(&text, Some(&file_str)).unwrap_or_default();
        let combined = chunks
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        result.push((relative, combined));
    }

    Ok(result)
}
