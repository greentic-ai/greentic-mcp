use greentic_mcp::{McpError, ToolInput, ToolMap, ToolMapConfig, ToolRef, WasixExecutor};
use serde_json::json;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

fn wasm_target_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let output = Command::new("rustc")
            .args(["--print", "target-list"])
            .output();
        match output {
            Ok(out) if out.status.success() => {
                let targets = String::from_utf8_lossy(&out.stdout);
                targets.lines().any(|line| line.trim() == "wasm32-wasip2")
            }
            _ => false,
        }
    })
}

fn build_echo_wasm() -> Option<PathBuf> {
    static WASM_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    WASM_PATH
        .get_or_init(|| {
            if !wasm_target_available() {
                return None;
            }
            let fixture_dir =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/echo_tool");
            let status = Command::new("cargo")
                .args(["build", "--release", "--target", "wasm32-wasip2"])
                .current_dir(&fixture_dir)
                .status()
                .expect("build echo fixture");
            if !status.success() {
                eprintln!("skipping echo tests: echo tool build failed with status {status:?}");
                return None;
            }
            Some(fixture_dir.join("target/wasm32-wasip2/release/echo_tool.wasm"))
        })
        .clone()
}

fn tool_ref(name: &str) -> Option<ToolRef> {
    let wasm = build_echo_wasm()?;
    Some(ToolRef {
        name: name.into(),
        component: wasm.to_string_lossy().into_owned(),
        entry: "tool_invoke".into(),
        timeout_ms: Some(1_000),
        max_retries: Some(2),
        retry_backoff_ms: Some(25),
    })
}

#[tokio::test]
async fn echo_ok() {
    let Some(tool) = tool_ref("echo") else {
        eprintln!("skipping echo_ok: wasm32-wasip2 target not installed");
        return;
    };
    let config = ToolMapConfig {
        tools: vec![tool.clone()],
    };
    let map = ToolMap::from_config(&config).expect("tool map");
    let executor = WasixExecutor::new().expect("executor");
    let output = executor
        .invoke(
            map.get("echo").expect("tool"),
            &ToolInput {
                payload: json!({"hello": "world"}),
            },
        )
        .await
        .expect("tool success");
    assert_eq!(output.payload, json!({"hello": "world"}));
}

#[tokio::test]
async fn echo_transient_retries() {
    let Some(mut tool) = tool_ref("echo-flaky") else {
        eprintln!("skipping echo_transient_retries: wasm32-wasip2 target not installed");
        return;
    };
    tool.max_retries = Some(3);
    let executor = WasixExecutor::new().expect("executor");
    let output = executor
        .invoke(
            &tool,
            &ToolInput {
                payload: json!({"flaky": true, "message": "hello"}),
            },
        )
        .await
        .expect("flaky tool should eventually succeed");

    assert_eq!(output.payload, json!({"flaky": true, "message": "hello"}));
}

#[tokio::test]
async fn echo_timeout() {
    let Some(mut tool) = tool_ref("echo-timeout") else {
        eprintln!("skipping echo_timeout: wasm32-wasip2 target not installed");
        return;
    };
    tool.timeout_ms = Some(50);
    tool.max_retries = Some(0);
    let executor = WasixExecutor::new().expect("executor");

    let err = executor
        .invoke(
            &tool,
            &ToolInput {
                payload: json!({"sleep_ms": 200, "note": "slow"}),
            },
        )
        .await
        .expect_err("should timeout");

    match err {
        McpError::Timeout { .. } => {}
        other => panic!("expected timeout error, got {other:?}"),
    }
}
