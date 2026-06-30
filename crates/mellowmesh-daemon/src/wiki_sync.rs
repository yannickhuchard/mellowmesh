use mellowmesh_core::okf;
use mellowmesh_store::Store;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub fn get_configured_wikis() -> HashMap<String, PathBuf> {
    let mut wikis = HashMap::new();
    if let Ok(env_val) = std::env::var("MELLOWMESH_WIKIS") {
        // Parse "default:./wiki,dev:./wiki_dev"
        for part in env_val.split(',') {
            if let Some((name, path_str)) = part.split_once(':') {
                wikis.insert(name.trim().to_string(), PathBuf::from(path_str.trim()));
            }
        }
    }
    if wikis.is_empty() {
        wikis.insert("default".to_string(), PathBuf::from("./wiki"));
    }
    wikis
}

pub async fn sync_all_wikis(store: &Store, wikis: &HashMap<String, PathBuf>) -> anyhow::Result<()> {
    for (name, path) in wikis {
        if let Err(e) = sync_wiki(store, name, path).await {
            tracing::error!("Failed to sync wiki '{}' at {:?}: {}", name, path, e);
        }
    }
    Ok(())
}

pub async fn sync_wiki(store: &Store, wiki_name: &str, wiki_root: &Path) -> anyhow::Result<()> {
    if !wiki_root.exists() {
        std::fs::create_dir_all(wiki_root)?;
    }

    tracing::info!("Syncing wiki '{}' from {:?}", wiki_name, wiki_root);

    // Helper for recursive traversal
    fn visit_dirs(
        dir: &Path,
        root: &Path,
        store: &Store,
        wiki_name: &str,
        scanned: &mut HashSet<String>,
    ) -> anyhow::Result<()> {
        if dir.is_dir() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    visit_dirs(&path, root, store, wiki_name, scanned)?;
                } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
                    if let Ok(rel_path) = path.strip_prefix(root) {
                        let rel_path_str = rel_path.to_string_lossy().replace('\\', "/");
                        scanned.insert(rel_path_str.clone());

                        if let Ok(content) = std::fs::read_to_string(&path) {
                            match okf::parse_okf_string(wiki_name, &rel_path_str, &content) {
                                Ok(doc) => {
                                    if let Err(e) = store.save_wiki_page(&doc) {
                                        tracing::error!(
                                            "Failed to save wiki page '{}': {}",
                                            rel_path_str,
                                            e
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Skipped invalid OKF file '{:?}': {}", path, e);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    // Wrap visit_dirs in spawn_blocking
    let store_clone = store.clone();
    let wiki_name_clone = wiki_name.to_string();
    let wiki_root_clone = wiki_root.to_path_buf();

    let scanned = tokio::task::spawn_blocking(move || {
        let mut scanned = HashSet::new();
        visit_dirs(
            &wiki_root_clone,
            &wiki_root_clone,
            &store_clone,
            &wiki_name_clone,
            &mut scanned,
        )?;
        Ok::<HashSet<String>, anyhow::Error>(scanned)
    })
    .await??;

    // Detect deleted files
    let store_clone = store.clone();
    let wiki_name_clone = wiki_name.to_string();
    tokio::task::spawn_blocking(move || {
        if let Ok(all_indexed) = store_clone.list_wiki_pages(&wiki_name_clone) {
            for doc in all_indexed {
                if !scanned.contains(&doc.path) {
                    tracing::info!("Removing deleted wiki page '{}' from database", doc.path);
                    let _ = store_clone.delete_wiki_page(&wiki_name_clone, &doc.path);
                }
            }
        }
    })
    .await?;

    Ok(())
}
