# git-topology

> Shared clustering primitives for git-cognitive and git-semantic — Leiden community detection over code embeddings, stored on a dedicated Git branch.

`git-topology` is a library crate. It is not a standalone CLI. It provides the shared building blocks that both `git-cognitive` and `git-semantic` depend on to cluster a codebase into semantically coherent subsystems and persist those clusters as shared Git state.

---

## What it does

1. Walks the repository and chunks all supported source files using tree-sitter
2. Embeds each file (Gemma local or OpenAI) and runs Leiden community detection on the similarity graph
3. Writes the resulting clusters to a `topology/v1` orphan Git branch as `.clusters.json` alongside a `.indexed-sha` file that records the HEAD commit at index time

Any tool that reads `topology/v1` can consume the clusters without knowing which tool wrote them. The branch is the contract.

`run_index` is idempotent: it compares HEAD against the stored `.indexed-sha` and skips re-clustering when no relevant source files changed. Concurrent callers are serialized via a `.git/topology.lock` exclusive lock.

---

## How git-cognitive and git-semantic use it

```
git-topology index          git-topology index
       ↓                           ↓
topology/v1   ←——————————→   topology/v1
       ↑                           ↑
git-cognitive audit         git-semantic map
(stamps cluster_id          (groups subsystems
 on each commit)             by cluster)
```

Whoever runs first writes the clusters. Whoever runs second reads them and skips re-clustering. The Leiden seed is fixed (`42`), so the same codebase produces stable cluster ids regardless of which tool triggered the index.

---

## Supported languages

Rust, Python, JavaScript, TypeScript, Java, C, C++, Go

---

## Embedding providers

| Provider | How to enable |
|---|---|
| Gemma 300M (local, default) | No setup — model downloads on first run to `~/.cache/fastembed` |
| OpenAI `text-embedding-3-small` | Set `OPENAI_API_KEY` and configure `git config topology.provider openai` |

---

## Usage as a library

```toml
[dependencies]
git-topology = { git = "https://github.com/ccherrad/git-topology" }
```

```rust
use git_topology::{run_index, read_cluster_map, is_stale, EmbeddingConfig};
use std::path::Path;

let repo = Path::new(".");

// build clusters (no-op if HEAD unchanged since last index)
let config = EmbeddingConfig::load_or_default()?;
run_index(repo, config)?;

// check staleness without triggering a full index
if is_stale(repo) {
    println!("clusters are out of date");
}

// read clusters (returns None if branch does not exist)
if let Some(map) = read_cluster_map(repo)? {
    for cluster in &map.clusters {
        println!("{}: {} files", cluster.name, cluster.files.len());
    }

    // find which clusters a set of changed files belong to
    let changed = vec!["src/auth/mod.rs".to_string()];
    let matched = map.clusters_for_files(&changed);
}
```

---

## Cluster format

Two files are written to the `topology/v1` branch:

**`.clusters.json`**
```json
{
  "version": 1,
  "clusters": [
    {
      "id": "a1b2c3d4e5f6",
      "name": "auth/middleware",
      "description": "auth/middleware: authenticate, authorize, verify_token",
      "files": ["src/auth/mod.rs", "src/auth/jwt.rs", "src/auth/session.rs"]
    }
  ]
}
```

**`.indexed-sha`** — the full SHA of HEAD at the time of the last index run. Used by `is_stale` to skip redundant re-clustering.

The `id` is a stable hash of the file membership. It does not change if the cluster is re-indexed with the same files.

---

## Requirements

- Rust 1.75+
- Git 2.0+
- For Gemma: ~600 MB disk for model download on first run
- For OpenAI: `OPENAI_API_KEY` environment variable
