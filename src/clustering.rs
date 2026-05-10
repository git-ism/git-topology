use anyhow::Result;
use leiden_rs::{GraphDataBuilder, Leiden, LeidenConfig};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use crate::cluster::{Cluster, ClusterMap};
use crate::embeddings::EmbeddingProvider;

pub struct FileInput {
    pub file: String,
    pub embedding: Vec<f32>,
    pub defined: HashSet<String>,
    pub referenced: HashSet<String>,
}

pub fn build_cluster_map<F>(
    inputs: Vec<FileInput>,
    description_embedder: &mut F,
) -> Result<ClusterMap>
where
    F: FnMut(&str) -> Result<Vec<f32>>,
{
    if inputs.is_empty() {
        return Ok(ClusterMap::empty());
    }

    let communities = leiden_cluster(&inputs);

    let mut clusters = Vec::new();

    for group in &communities {
        let centroid = compute_centroid(group);

        let centroid_file = group
            .iter()
            .min_by(|a, b| {
                cosine_distance(&a.embedding, &centroid)
                    .partial_cmp(&cosine_distance(&b.embedding, &centroid))
                    .unwrap()
            })
            .unwrap();

        let files: Vec<String> = group.iter().map(|f| f.file.clone()).collect();
        let name = build_name(centroid_file, group);
        let description = build_description(centroid_file, group);
        let _ = description_embedder(&description);

        let id = generate_cluster_id(&files);

        clusters.push(Cluster {
            id,
            name,
            description,
            files,
        });
    }

    clusters.sort_by(|a, b| b.files.len().cmp(&a.files.len()));

    Ok(ClusterMap {
        version: 1,
        clusters,
    })
}

pub fn embed_and_cluster(
    file_texts: Vec<(String, String)>,
    provider: &mut dyn EmbeddingProvider,
) -> Result<ClusterMap> {
    let mut inputs = Vec::new();

    for (file, text) in &file_texts {
        let embedding = provider.generate_embedding(text)?;
        let mut defined = HashSet::new();
        let mut referenced = HashSet::new();
        extract_names(text, &mut defined, &mut referenced);
        for name in &defined {
            referenced.remove(name);
        }
        inputs.push(FileInput {
            file: file.clone(),
            embedding,
            defined,
            referenced,
        });
    }

    let mut noop_embedder = |_: &str| -> Result<Vec<f32>> { Ok(vec![]) };
    build_cluster_map(inputs, &mut noop_embedder)
}

fn leiden_cluster(file_units: &[FileInput]) -> Vec<Vec<&FileInput>> {
    let n = file_units.len();

    if n <= 1 {
        return file_units.iter().map(|f| vec![f]).collect();
    }

    const SIMILARITY_THRESHOLD: f32 = 0.65;

    let mut builder = GraphDataBuilder::new(n);
    for i in 0..n {
        for j in (i + 1)..n {
            let sim = 1.0 - cosine_distance(&file_units[i].embedding, &file_units[j].embedding);
            if sim > SIMILARITY_THRESHOLD {
                let _ = builder.add_edge(i, j, sim as f64);
            }
        }
    }

    let graph = match builder.build() {
        Ok(g) => g,
        Err(_) => return file_units.iter().map(|f| vec![f]).collect(),
    };

    let max_comm = (n / 10).clamp(5, 50);
    let config = LeidenConfig {
        seed: Some(42),
        resolution: 2.0,
        max_comm_size: max_comm,
        ..Default::default()
    };

    let partition = match Leiden::new(config).run(&graph) {
        Ok(result) => result.partition,
        Err(_) => return file_units.iter().map(|f| vec![f]).collect(),
    };

    let mut community_map: HashMap<usize, Vec<&FileInput>> = HashMap::new();
    for (node_idx, file_unit) in file_units.iter().enumerate() {
        let community = partition.community_of(node_idx);
        community_map.entry(community).or_default().push(file_unit);
    }

    let mut communities: Vec<Vec<&FileInput>> = community_map.into_values().collect();
    communities.sort_by_key(|b| Reverse(b.len()));
    communities
}

fn compute_centroid(group: &[&FileInput]) -> Vec<f32> {
    let dim = group[0].embedding.len();
    let mut sum = vec![0.0f32; dim];
    for f in group {
        for (d, v) in f.embedding.iter().enumerate() {
            sum[d] += v;
        }
    }
    let n = group.len() as f32;
    sum.iter().map(|v| v / n).collect()
}

fn build_name(centroid: &FileInput, group: &[&FileInput]) -> String {
    let dirs: HashSet<String> = group.iter().map(|f| file_dir(&f.file)).collect();

    if dirs.len() == 1 {
        let dir = file_dir(&centroid.file);
        let dir_stem = std::path::Path::new(&dir)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&dir)
            .to_string();
        let file_stem = std::path::Path::new(&centroid.file)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if dir_stem == file_stem || dir_stem == "." {
            file_stem
        } else {
            format!("{}/{}", dir_stem, file_stem)
        }
    } else {
        let parts: Vec<Vec<&str>> = group.iter().map(|f| f.file.split('/').collect()).collect();
        let min_len = parts.iter().map(|p| p.len()).min().unwrap_or(0);
        let mut common = Vec::new();
        for i in 0..min_len.saturating_sub(1) {
            let seg = parts[0][i];
            if parts.iter().all(|p| p[i] == seg) {
                common.push(seg);
            } else {
                break;
            }
        }
        if common.is_empty() {
            file_dir(&centroid.file)
        } else {
            common.join("/")
        }
    }
}

fn build_description(centroid: &FileInput, group: &[&FileInput]) -> String {
    let dir = file_dir(&centroid.file);
    let stem = std::path::Path::new(&centroid.file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&centroid.file)
        .to_string();

    let dir_label = if dir == "." {
        stem
    } else {
        format!("{}/{}", dir, stem)
    };

    let mut names: Vec<String> = centroid.defined.iter().cloned().collect();
    for f in group {
        if f.file == centroid.file {
            continue;
        }
        for name in &f.defined {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    names.truncate(7);

    if names.is_empty() {
        dir_label
    } else {
        format!("{}: {}", dir_label, names.join(", "))
    }
}

fn file_dir(file: &str) -> String {
    std::path::Path::new(file)
        .parent()
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(".")
        .to_string()
}

fn generate_cluster_id(files: &[String]) -> String {
    let mut sorted = files.to_vec();
    sorted.sort();
    let key = sorted.join("|");
    let hash = key
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    format!("{:012x}", hash & 0xffffffffffff)
}

pub fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 1.0;
    }
    1.0 - (dot / (norm_a * norm_b))
}

fn extract_names(text: &str, defined: &mut HashSet<String>, referenced: &mut HashSet<String>) {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
        }

        for kw in &[
            "fn ", "struct ", "trait ", "enum ", "def ", "class ", "func ",
        ] {
            if let Some(rest) = trimmed.find(kw).map(|pos| &trimmed[pos + kw.len()..]) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    defined.insert(name);
                }
            }
        }

        extract_call_references(trimmed, referenced);
    }
}

fn extract_call_references(line: &str, referenced: &mut HashSet<String>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_alphabetic() && bytes[i] != b'_' {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let ident = &line[start..i];
        let mut j = i;
        while j < bytes.len() && bytes[j] == b' ' {
            j += 1;
        }
        if j < bytes.len()
            && (bytes[j] == b'('
                || (j + 1 < bytes.len() && bytes[j] == b':' && bytes[j + 1] == b':'))
            && ident.len() > 2
            && !is_keyword(ident)
        {
            referenced.insert(ident.to_string());
        }
        i = i.max(start + 1);
    }
}

fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "fn" | "let"
            | "mut"
            | "pub"
            | "use"
            | "mod"
            | "impl"
            | "struct"
            | "enum"
            | "trait"
            | "for"
            | "in"
            | "if"
            | "else"
            | "match"
            | "return"
            | "self"
            | "Self"
            | "super"
            | "crate"
            | "where"
            | "async"
            | "await"
            | "move"
            | "ref"
            | "type"
            | "const"
            | "static"
            | "unsafe"
            | "extern"
            | "true"
            | "false"
            | "new"
            | "default"
            | "clone"
            | "from"
            | "into"
            | "as_ref"
            | "unwrap"
            | "expect"
            | "map"
            | "and_then"
            | "ok"
            | "err"
            | "is_empty"
            | "len"
            | "iter"
            | "collect"
            | "push"
            | "pop"
            | "get"
            | "set"
            | "insert"
            | "remove"
            | "contains"
            | "None"
            | "Some"
            | "Ok"
            | "Err"
            | "def"
            | "class"
            | "import"
            | "func"
            | "var"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_distance_identical() {
        let a = vec![1.0, 0.0, 0.0];
        assert!((cosine_distance(&a, &a) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_distance_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_distance(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_distance_zero_vector() {
        let a = vec![0.0, 0.0];
        let b = vec![1.0, 0.0];
        assert_eq!(cosine_distance(&a, &b), 1.0);
    }

    #[test]
    fn cluster_id_stable() {
        let files = vec!["src/auth.rs".to_string(), "src/db.rs".to_string()];
        let id1 = generate_cluster_id(&files);
        let id2 = generate_cluster_id(&files);
        assert_eq!(id1, id2);
    }

    #[test]
    fn cluster_id_order_independent() {
        let a = vec!["src/auth.rs".to_string(), "src/db.rs".to_string()];
        let b = vec!["src/db.rs".to_string(), "src/auth.rs".to_string()];
        assert_eq!(generate_cluster_id(&a), generate_cluster_id(&b));
    }

    #[test]
    fn cluster_id_different_files() {
        let a = vec!["src/auth.rs".to_string()];
        let b = vec!["src/db.rs".to_string()];
        assert_ne!(generate_cluster_id(&a), generate_cluster_id(&b));
    }

    #[test]
    fn extract_names_finds_definitions() {
        let text = "fn authenticate() {}\nstruct User {}\n";
        let mut defined = std::collections::HashSet::new();
        let mut referenced = std::collections::HashSet::new();
        extract_names(text, &mut defined, &mut referenced);
        assert!(defined.contains("authenticate"));
        assert!(defined.contains("User"));
    }

    #[test]
    fn extract_names_skips_keywords() {
        let text = "fn new() {}\n";
        let mut defined = std::collections::HashSet::new();
        let mut referenced = std::collections::HashSet::new();
        extract_names(text, &mut defined, &mut referenced);
        assert!(!referenced.contains("new"));
    }

    #[test]
    fn build_cluster_map_empty_input() {
        let mut noop = |_: &str| -> Result<Vec<f32>> { Ok(vec![]) };
        let map = build_cluster_map(vec![], &mut noop).unwrap();
        assert!(map.clusters.is_empty());
    }

    #[test]
    fn build_cluster_map_single_file() {
        let input = FileInput {
            file: "src/main.rs".to_string(),
            embedding: vec![1.0, 0.0, 0.0],
            defined: std::collections::HashSet::new(),
            referenced: std::collections::HashSet::new(),
        };
        let mut noop = |_: &str| -> Result<Vec<f32>> { Ok(vec![]) };
        let map = build_cluster_map(vec![input], &mut noop).unwrap();
        assert_eq!(map.clusters.len(), 1);
        assert_eq!(map.clusters[0].files, vec!["src/main.rs"]);
    }
}
