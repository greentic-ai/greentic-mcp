//! Executor library for loading and running `wasix:mcp` compatible Wasm components.
//! Users supply an [`ExecConfig`] describing how to resolve artifacts and what
//! runtime constraints to enforce, then call [`exec`] with a structured request.

mod config;
pub mod describe;
mod error;
mod path_safety;
mod resolve;
pub mod router;
pub mod runner;
mod store;
#[cfg(test)]
mod test_support;
mod verify;

pub use config::{DynSecretsStore, ExecConfig, RuntimePolicy, SecretsStore, VerifyPolicy};
pub use error::{ExecError, RunnerError};
pub use store::{ToolInfo, ToolStore};

use greentic_types::TenantCtx;
use serde_json::{Value, json};

use crate::runner::Runner;

/// One tool advertised by a `wasix:mcp/router` component.
#[derive(Clone, Debug)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    /// JSON Schema for what the tool RETURNS, when the component declares one.
    ///
    /// `output-schema` is `option<json>` in the WIT and that optionality is
    /// carried through unchanged: an undeclared schema stays `None` rather than
    /// collapsing into an empty object. A consumer asking "what fields does this
    /// tool return?" needs "not declared" and "declared empty" to differ, and
    /// the same contract is already shipped in greentic-mcp-client and
    /// greentic-designer — all three hops have to agree or the distinction is
    /// lost at whichever one disagrees.
    ///
    /// Why this matters: a flow referenced `{{node.mcp_quote.outputs.…}}` and
    /// another `{{node.mcp_quote.…}}`, and both rendered blank with no error,
    /// because a tool's real field names were unknowable to every layer above.
    pub output_schema: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct ExecRequest {
    pub component: String,
    pub action: String,
    pub args: Value,
    pub tenant: Option<TenantCtx>,
}

/// Execute a single action exported by an MCP component.
///
/// Resolution, verification, and runtime enforcement are performed in sequence,
/// with detailed errors surfaced through [`ExecError`].
pub fn exec(req: ExecRequest, cfg: &ExecConfig) -> Result<Value, ExecError> {
    let resolved = resolve::resolve(&req.component, &cfg.store)
        .map_err(|err| ExecError::resolve(&req.component, err))?;

    let verified = verify::verify(&req.component, resolved, &cfg.security)
        .map_err(|err| ExecError::verification(&req.component, err))?;

    let runner = runner::DefaultRunner::new(&cfg.runtime)
        .map_err(|err| ExecError::runner(&req.component, err))?;

    let result = runner.run(
        &req,
        &verified,
        runner::ExecutionContext {
            runtime: &cfg.runtime,
            http_enabled: cfg.http_enabled,
            secrets_store: cfg.secrets_store.clone(),
        },
    );

    let value = match result {
        Ok(v) => v,
        Err(RunnerError::ActionNotFound { .. }) => {
            return Err(ExecError::not_found(
                req.component.clone(),
                req.action.clone(),
            ));
        }
        Err(RunnerError::ToolTransient { component, message }) => {
            return Err(ExecError::tool_error(
                component,
                req.action.clone(),
                "transient",
                json!({ "message": message }),
            ));
        }
        Err(RunnerError::Internal(message)) => {
            return Err(ExecError::runner(
                &req.component,
                RunnerError::Internal(message),
            ));
        }
        Err(err) => return Err(ExecError::runner(&req.component, err)),
    };

    if let Some(code) = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .map(str::to_owned)
    {
        if code == "iface-error.not-found" {
            return Err(ExecError::not_found(req.component, req.action));
        } else {
            return Err(ExecError::tool_error(
                req.component,
                req.action,
                code,
                value,
            ));
        }
    }

    Ok(value)
}

/// List the tools a local `wasix:mcp` component exports (the `tools/list`
/// equivalent), resolving and verifying via the same pipeline as [`exec`].
///
/// Synchronous (blocking Wasmtime) — callers on an async runtime must wrap this
/// in `spawn_blocking`.
pub fn list_tools(component: &str, cfg: &ExecConfig) -> Result<Vec<ToolDef>, ExecError> {
    let resolved = resolve::resolve(component, &cfg.store)
        .map_err(|err| ExecError::resolve(component, err))?;
    let verified = verify::verify(component, resolved, &cfg.security)
        .map_err(|err| ExecError::verification(component, err))?;

    let runner = runner::DefaultRunner::new(&cfg.runtime)
        .map_err(|err| ExecError::runner(component, err))?;

    let tools = runner
        .list_tools_router(
            &verified,
            runner::ExecutionContext {
                runtime: &cfg.runtime,
                http_enabled: cfg.http_enabled,
                secrets_store: cfg.secrets_store.clone(),
            },
        )
        .map_err(|err| ExecError::runner(component, err))?
        .unwrap_or_default();

    Ok(tools
        .into_iter()
        .map(|t| ToolDef {
            name: t.name,
            description: t.description,
            // Pre-existing behaviour, deliberately left alone: a malformed
            // *input* schema still degrades to `{}`. Changing that is a
            // separate call from this one.
            input_schema: serde_json::from_str(&t.input_schema)
                .unwrap_or_else(|_| serde_json::json!({})),
            // A malformed *output* schema is kept verbatim as
            // `Value::String(raw)` instead of being normalised away — this
            // crate reports what the component declared, and a consumer that
            // cares can see it and warn. Matches the 1.3 research line's
            // `parse_json_payload`, so the two lanes do not drift.
            output_schema: t.output_schema.as_deref().map(|raw| {
                serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
            }),
        })
        .collect())
}

#[cfg(test)]
mod output_schema_tests {
    use super::*;
    use serde_json::json;

    /// The mapping the 1.1.x line performs on a router tool record. Kept as a
    /// pure fn so the three decisions below are testable without a wasm
    /// fixture — the integration path needs a built component, and on this lane
    /// the fixture harness silently skips when `CARGO_TARGET_DIR` is set, which
    /// makes it the wrong place to pin a contract.
    fn map_output_schema(raw: Option<&str>) -> Option<Value> {
        raw.map(|raw| serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string())))
    }

    /// A declared schema must survive — this is the whole point: without it a
    /// tool's field names are unknowable to every layer above.
    #[test]
    fn a_declared_output_schema_is_parsed() {
        let got = map_output_schema(Some(
            r#"{"type":"object","properties":{"annual_premium":{"type":"number"}}}"#,
        ));
        assert_eq!(
            got.as_ref()
                .and_then(|v| v.pointer("/properties/annual_premium/type")),
            Some(&json!("number")),
            "got {got:?}"
        );
    }

    /// `output-schema` is `option<json>` in the WIT, so absent is normal — and
    /// must stay distinguishable from a component that declared an empty
    /// schema. greentic-mcp-client and greentic-designer settled on the same
    /// contract; if this lane collapsed absent into `{}` the distinction would
    /// be lost here and nowhere else.
    #[test]
    fn an_undeclared_output_schema_stays_none() {
        assert_eq!(map_output_schema(None), None);
    }

    /// Malformed is kept verbatim as evidence rather than normalised away: this
    /// crate reports what the component declared, and a consumer that cares can
    /// see it and warn. Swallowing it here would hide a broken component from
    /// every consumer at once.
    #[test]
    fn a_malformed_output_schema_is_kept_verbatim() {
        assert_eq!(
            map_output_schema(Some("not-json")),
            Some(Value::String("not-json".to_string()))
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RuntimePolicy, VerifyPolicy};
    use crate::error::RunnerError;
    use crate::store::ToolStore;
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

        let digest = crate::resolve::resolve(
            "echo.component",
            &ToolStore::LocalDir(PathBuf::from(tempdir.path())),
        )
        .expect("resolve")
        .digest;

        let mut required = HashMap::new();
        required.insert("echo.component".to_string(), digest.clone());

        let cfg = ExecConfig {
            store: ToolStore::LocalDir(PathBuf::from(tempdir.path())),
            security: VerifyPolicy {
                allow_unverified: false,
                required_digests: required,
                trusted_signers: Vec::new(),
            },
            runtime: RuntimePolicy::default(),
            http_enabled: false,
            secrets_store: None,
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
                    secrets_store: cfg.secrets_store.clone(),
                },
            )
            .expect("run");

        assert_eq!(
            result.get("component_digest").and_then(Value::as_str),
            Some(digest.as_str())
        );
    }
}
