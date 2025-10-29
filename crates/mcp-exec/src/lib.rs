mod config;
mod error;
mod resolve;
mod runner;
mod verify;

pub use config::{
    ExecConfig, LocalStore, OciAuth, OciStore, RuntimePolicy, ToolStore, VerifyPolicy, WargStore,
};
pub use error::ExecError;

use greentic_types::tenant::TenantCtx;
use serde_json::Value;

use crate::runner::Runner;

#[derive(Clone, Debug)]
pub struct ExecRequest {
    pub component: String,
    pub action: String,
    pub args: Value,
    pub tenant: Option<TenantCtx>,
}

pub fn exec(req: ExecRequest, cfg: &ExecConfig) -> Result<Value, ExecError> {
    let resolved = resolve::resolve(&req.component, &cfg.store)
        .map_err(|err| ExecError::resolve(&req.component, err))?;

    let verified = verify::verify(&req.component, resolved, &cfg.security)
        .map_err(|err| ExecError::verification(&req.component, err))?;

    let runner = runner::DefaultRunner::new(&cfg.runtime)
        .map_err(|err| ExecError::runner(&req.component, err))?;

    runner
        .run(
            &req,
            &verified,
            runner::ExecutionContext {
                runtime: &cfg.runtime,
                http_enabled: cfg.http_enabled,
            },
        )
        .map_err(|err| ExecError::runner(&req.component, err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LocalStore, RuntimePolicy, ToolStore, VerifyPolicy};
    use crate::error::RunnerError;
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use crate::verify::VerifiedArtifact;

    #[derive(Default)]
    struct MockRunner;

    impl runner::Runner for MockRunner {
        fn run(
            &self,
            request: &ExecRequest,
            artifact: &VerifiedArtifact,
            _ctx: runner::ExecutionContext<'_>,
        ) -> Result<Value, RunnerError> {
            let mut payload = request.args.clone();
            if let Value::Object(map) = &mut payload {
                map.insert(
                    "component_digest".to_string(),
                    Value::String(artifact.resolved.digest.clone()),
                );
            }
            Ok(payload)
        }
    }

    #[test]
    fn local_resolve_and_verify_success() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let wasm_path = tempdir.path().join("echo.component.wasm");
        std::fs::write(&wasm_path, b"fake wasm contents").expect("write");

        let mut digests = HashMap::new();
        let digest = crate::resolve::resolve(
            "echo.component",
            &ToolStore::Local(LocalStore::new(vec![tempdir.path().to_path_buf()])),
        )
        .expect("resolve")
        .digest;
        digests.insert("echo.component".to_string(), digest.clone());

        let cfg = ExecConfig {
            store: ToolStore::Local(LocalStore::new(vec![PathBuf::from(tempdir.path())])),
            security: VerifyPolicy {
                allow_unverified: false,
                required_digests: digests.clone(),
                trusted_signers: Vec::new(),
            },
            runtime: RuntimePolicy::default(),
            http_enabled: false,
        };

        let req = ExecRequest {
            component: "echo.component".into(),
            action: "noop".into(),
            args: json!({"message": "hello"}),
            tenant: None,
        };

        // Inject our mock runner to exercise pipeline without executing wasm.
        let resolved =
            crate::resolve::resolve(&req.component, &cfg.store).expect("resolve second time");
        let verified =
            crate::verify::verify(&req.component, resolved, &cfg.security).expect("verify");
        let result = MockRunner
            .run(
                &req,
                &verified,
                runner::ExecutionContext {
                    runtime: &cfg.runtime,
                    http_enabled: cfg.http_enabled,
                },
            )
            .expect("run");

        assert_eq!(
            result.get("component_digest").and_then(Value::as_str),
            Some(digest.as_str())
        );
    }
}
