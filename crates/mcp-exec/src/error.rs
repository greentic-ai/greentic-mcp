use std::io;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecError {
    #[error("failed to resolve component `{component}`: {source}")]
    Resolve {
        component: String,
        #[source]
        source: ResolveError,
    },
    #[error("artifact verification failed for `{component}`: {source}")]
    Verification {
        component: String,
        #[source]
        source: VerificationError,
    },
    #[error("runtime error while executing `{component}`: {source}")]
    Runner {
        component: String,
        #[source]
        source: RunnerError,
    },
}

impl ExecError {
    pub fn resolve(component: impl Into<String>, source: ResolveError) -> Self {
        Self::Resolve {
            component: component.into(),
            source,
        }
    }

    pub fn verification(component: impl Into<String>, source: VerificationError) -> Self {
        Self::Verification {
            component: component.into(),
            source,
        }
    }

    pub fn runner(component: impl Into<String>, source: RunnerError) -> Self {
        Self::Runner {
            component: component.into(),
            source,
        }
    }
}

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("component was not found in the configured store(s)")]
    NotFound,
    #[error("I/O error while reading artifact")]
    Io(#[from] io::Error),
    #[error("OCI resolver is not yet implemented")]
    OciNotImplemented,
    #[error("Warg resolver is not yet implemented")]
    WargNotImplemented,
}

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: String, actual: String },
    #[error("artifact is unsigned and policy does not allow it")]
    UnsignedRejected,
}

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("wasm execution timed out after {elapsed:?}")]
    Timeout { elapsed: Duration },
    #[error("wasmtime error: {0}")]
    Wasmtime(#[from] wasmtime::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("runner is not implemented for this configuration")]
    NotImplemented,
}
