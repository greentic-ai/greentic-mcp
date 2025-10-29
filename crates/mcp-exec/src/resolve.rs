use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::config::{LocalStore, ToolStore};
use crate::error::ResolveError;

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum ArtifactOrigin {
    Local(PathBuf),
    Oci {
        reference: String,
    },
    Warg {
        package: String,
        reference: Option<String>,
    },
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ResolvedArtifact {
    pub origin: ArtifactOrigin,
    pub bytes: Arc<[u8]>,
    pub digest: String,
}

pub fn resolve(component: &str, store: &ToolStore) -> Result<ResolvedArtifact, ResolveError> {
    match store {
        ToolStore::Local(local) => resolve_local(component, local),
        ToolStore::Oci(_) => Err(ResolveError::OciNotImplemented),
        ToolStore::Warg(_) => Err(ResolveError::WargNotImplemented),
    }
}

fn resolve_local(component: &str, local: &LocalStore) -> Result<ResolvedArtifact, ResolveError> {
    let candidate_names = candidate_file_names(component, local.expected_extension.as_deref());

    for search_root in &local.search_paths {
        for candidate in &candidate_names {
            let path = search_root.join(candidate);
            if path.is_file() {
                let bytes = Arc::from(fs::read(&path)?);
                let digest = compute_digest(&bytes);
                return Ok(ResolvedArtifact {
                    origin: ArtifactOrigin::Local(path),
                    bytes,
                    digest,
                });
            }
        }
    }

    Err(ResolveError::NotFound)
}

fn candidate_file_names(component: &str, extension: Option<&str>) -> Vec<PathBuf> {
    let base = PathBuf::from(component);
    let mut names = vec![base.clone()];

    if let Some(ext) = extension {
        let matches_extension = base
            .extension()
            .map(|existing| existing == ext)
            .unwrap_or(false);
        if !matches_extension {
            let mut with_ext = base.clone();
            with_ext.set_extension(ext);
            names.push(with_ext);
        }
    }

    if !component.ends_with(".wasm") {
        names.push(PathBuf::from(format!("{component}.wasm")));
    }

    if !component.ends_with(".component.wasm") {
        names.push(PathBuf::from(format!("{component}.component.wasm")));
    }

    names
}

fn compute_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hash = hasher.finalize();
    hex::encode(hash)
}
