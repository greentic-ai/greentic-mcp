use greentic_mcp::exec_with_retries;
use mcp_exec::{ExecConfig, ExecError, ExecRequest, RuntimePolicy, ToolStore, VerifyPolicy};
use serde_json::json;
use std::{fs, path::PathBuf, time::Duration};
use tempfile::TempDir;

fn default_runtime_policy() -> RuntimePolicy {
    RuntimePolicy {
        per_call_timeout: Duration::from_secs(10),
        max_attempts: 1,
        base_backoff: Duration::from_millis(50),
        ..RuntimePolicy::default()
    }
}

fn setup_config(runtime: RuntimePolicy) -> (ExecConfig, TempDir) {
    let tmp = TempDir::new().expect("tempdir");
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/echo_tool/echo_tool.wasm");
    let dest = tmp.path().join("echo_tool.wasm");
    fs::copy(&fixture, &dest).expect("copy wasm fixture");

    let cfg = ExecConfig {
        store: ToolStore::LocalDir(tmp.path().into()),
        security: VerifyPolicy {
            allow_unverified: true,
            ..Default::default()
        },
        runtime,
        http_enabled: false,
    };
    (cfg, tmp)
}

fn make_request(payload: serde_json::Value) -> ExecRequest {
    ExecRequest {
        component: "echo_tool".into(),
        action: "tool-invoke".into(),
        args: payload,
        tenant: None,
    }
}

#[tokio::test]
async fn echo_ok_wasm() {
    let (cfg, _tmp) = setup_config(default_runtime_policy());
    let req = make_request(json!({"hello": "world"}));
    let result = mcp_exec::exec(req, &cfg).expect("tool success");
    assert_eq!(result, json!({"hello": "world"}));
}

#[tokio::test]
async fn echo_timeout_wasm() {
    let mut runtime = default_runtime_policy();
    runtime.per_call_timeout = Duration::from_millis(150);
    runtime.max_attempts = 1;
    let (cfg, _tmp) = setup_config(runtime);
    let req = make_request(json!({"sleep_ms": 500, "note": "slow"}));

    let err = mcp_exec::exec(req, &cfg).expect_err("should timeout");
    match err {
        ExecError::Runner { source, .. } => match source {
            mcp_exec::RunnerError::Timeout { .. } => {}
            other => panic!("expected timeout but got {other:?}"),
        },
        other => panic!("expected runner timeout, got {other:?}"),
    }
}

#[tokio::test]
async fn echo_transient_retries_wasm() {
    let mut runtime = default_runtime_policy();
    runtime.per_call_timeout = Duration::from_secs(3);
    runtime.max_attempts = 5;
    runtime.base_backoff = Duration::from_millis(50);
    let (cfg, _tmp) = setup_config(runtime);

    let req = make_request(json!({"flaky": true, "message": "hello"}));
    let result = exec_with_retries(req, &cfg)
        .await
        .expect("flaky tool should eventually succeed");
    assert_eq!(result, json!({"flaky": true, "message": "hello"}));
}
