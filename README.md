# Greentic MCP

Executor and component tooling for the Greentic platform targeting the
`wasix:mcp` interface. The workspace currently provides a reusable Rust
library (`mcp-exec`) that can load Wasm components, verify their provenance,
wire in the Greentic host imports, and execute the exported MCP entrypoint,
plus placeholder crates for future component work.

## Workspace layout

```
greentic-mcp/
├─ crates/
│  ├─ mcp-component/    # Wasm component shell scaffold (placeholder)
│  └─ mcp-exec/         # executor library
└─ Cargo.toml           # workspace manifest
```

### `mcp-exec`

Public API:

```rust
use greentic_types::{EnvId, TenantCtx, TenantId};
use mcp_exec::{ExecConfig, ExecRequest, RuntimePolicy, ToolStore, VerifyPolicy};
use serde_json::json;
use std::path::PathBuf;

let tenant = TenantCtx {
    env: EnvId("dev".into()),
    tenant: TenantId("acme".into()),
    team: None,
    user: None,
    trace_id: Some("trace-123".into()),
    correlation_id: None,
    deadline: None,
    attempt: 0,
    idempotency_key: None,
};

let cfg = ExecConfig {
    store: ToolStore::LocalDir(PathBuf::from("./tools")),
    security: VerifyPolicy::default(),
    runtime: RuntimePolicy::default(),
    http_enabled: false,
};

let result = mcp_exec::exec(
    ExecRequest {
        component: "weather_api".into(),
        action: "forecast_weather".into(),
        args: json!({"location": "AMS"}),
        tenant: Some(tenant),
    },
    &cfg,
)?;
```

Key features:

- **Resolver** – Reads Wasm bytes from local directories or single-file HTTP sources (with caching).
- **Verifier** – Checks digest/signature policy before execution.
- **Runner** – Spins up a Wasmtime component environment, registers Greentic host imports, and calls the tool's MCP `exec` export.
- **Errors** – Structured error types map resolution, verification, and runtime failures to caller-friendly variants.

### `mcp-component`

Placeholder crate intended to host a reference Wasm component that exports the
`wasix:mcp` interface. The current implementation is a stub so the crate can be
expanded alongside the executor.

### `greentic-types`

Pulled from crates.io; provides `TenantCtx`, identifiers, and supporting types for multi-tenant flows.

## Development

```bash
rustup target add wasm32-wasip2
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
RUN_ONLINE_TESTS=1 cargo test -p mcp-exec --test online_weather
```

The online weather integration test is skipped unless `RUN_ONLINE_TESTS=1` is set.

## Releases & Publishing

- Versions are taken directly from each crate's `Cargo.toml`.
- When a commit lands on `master`, any crate whose manifest version changed gets a Git tag `<crate>-v<version>` pushed automatically.
- The publish workflow then runs, linting and testing before calling `katyo/publish-crates@v2` to publish updated crates to crates.io.
- Publishing is idempotent; if the specified version already exists, the workflow exits successfully without pushing anything new.

## Roadmap

- Implement OCI and Warg resolvers, including signature verification.
- Publish spec docs and add end-to-end examples powered by real tool WASMs.

## License

Dual-licensed under either MIT or Apache-2.0. See `LICENSE-MIT` and
`LICENSE-APACHE` once added to the repository.
