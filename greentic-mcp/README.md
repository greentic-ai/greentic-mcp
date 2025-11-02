# Greentic MCP Tool Bridge

`greentic-mcp` provides the host-side tool registry and WASIX/WASI execution
bridge used by flows to invoke MCP tools compiled to WebAssembly. It loads a
declarative tool map, resolves tools by logical name, and executes them through
Wasmtime with per-tool timeouts, retry hints, and transient error handling.

The crate leans on the shared contracts published in
[`greentic-types`](https://docs.rs/greentic-types) and the WIT definitions plus
generated bindings in [`greentic-interfaces`](https://docs.rs/greentic-interfaces).
When a tool exposes the standard component ABI, the same bindings can be used to
reach richer type-safe interfaces. Until those tools are widespread this crate
ships a pointer/length fallback ABI that keeps local tool development unblocked.

## Tool map configuration

Tool metadata is loaded from JSON or YAML. Each entry records where the tool
artifact lives, the export to call, and optional execution hints.

```yaml
tools:
  - name: echo
    component: ./tools/echo.wasm
    entry: tool_invoke
    timeout_ms: 1000
    max_retries: 2
    retry_backoff_ms: 200
```

Use `greentic_mcp::load_tool_map` to parse the file and build a `ToolMap`.

```rust,no_run
use greentic_mcp::{invoke_with_map, load_tool_map, WasixExecutor};
use serde_json::json;
use std::path::Path;

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let map = load_tool_map(Path::new("toolmap.yaml"))?;
let executor = WasixExecutor::new()?;

let echoed = invoke_with_map(&map, &executor, "echo", json!({"hello": "world"}))
    .await?;

assert_eq!(echoed, json!({"hello": "world"}));
# Ok(())
# }
```

`WasixExecutor` ensures that traps bubble up as transient errors, applies
exponential backoff with jitter between retries, and converts wall-clock
timeouts into `McpError::Timeout`.

## ABI contracts

See [ABI.md](ABI.md) for details on the fallback pointer/length interface used
by the integration tests. When a tool exports the canonical Greentic component
API, use the bindings from `greentic-interfaces` to avoid manual pointer
management and to return strongly-typed `Outcome<T>` values.
