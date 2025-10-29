use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Configuration for a single executor invocation.
#[derive(Clone, Debug)]
pub struct ExecConfig {
    pub store: ToolStore,
    pub security: VerifyPolicy,
    pub runtime: RuntimePolicy,
    pub http_enabled: bool,
}

/// Supported tool stores that can be resolved into runnable artifacts.
#[derive(Clone, Debug)]
pub enum ToolStore {
    Local(LocalStore),
    Oci(OciStore),
    Warg(WargStore),
}

/// Local filesystem lookup strategy.
#[derive(Clone, Debug)]
pub struct LocalStore {
    pub search_paths: Vec<PathBuf>,
    pub expected_extension: Option<String>,
}

impl LocalStore {
    pub fn new(search_paths: Vec<PathBuf>) -> Self {
        Self {
            search_paths,
            expected_extension: Some("wasm".to_string()),
        }
    }
}

/// OCI registry configuration.
#[derive(Clone, Debug)]
pub struct OciStore {
    pub registry: String,
    pub repository: String,
    pub reference: Option<String>,
    pub auth: Option<OciAuth>,
}

/// Authentication options for OCI registries.
#[derive(Clone, Debug)]
pub enum OciAuth {
    Anonymous,
    BearerToken(String),
    UsernamePassword { username: String, password: String },
}

/// Warg registry configuration.
#[derive(Clone, Debug)]
pub struct WargStore {
    pub server: String,
    pub package: String,
    pub reference: Option<String>,
}

/// Policy describing how artifacts must be verified prior to execution.
#[derive(Clone, Debug, Default)]
pub struct VerifyPolicy {
    /// Whether artifacts without a matching digest/signature are still allowed.
    pub allow_unverified: bool,
    /// Expected digests (hex encoded) keyed by component identifier.
    pub required_digests: HashMap<String, String>,
    /// Signers that are trusted to vouch for artifacts.
    pub trusted_signers: Vec<String>,
}

/// Runtime resource limits applied to the Wasm execution.
#[derive(Clone, Debug)]
pub struct RuntimePolicy {
    pub fuel: Option<u64>,
    pub max_memory: Option<u64>,
    pub wallclock_timeout: Duration,
}

impl Default for RuntimePolicy {
    fn default() -> Self {
        Self {
            fuel: None,
            max_memory: None,
            wallclock_timeout: Duration::from_secs(30),
        }
    }
}
