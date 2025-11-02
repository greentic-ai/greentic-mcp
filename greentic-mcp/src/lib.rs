//! Host-side ToolMap management and WASIX/WASI execution bridge for Greentic MCP tools.

pub mod config;
pub mod executor;
pub mod retry;
pub mod tool_map;
pub mod types;

pub use config::load_tool_map_config;
pub use executor::WasixExecutor;
pub use tool_map::ToolMap;
pub use types::{McpError, ToolInput, ToolMapConfig, ToolOutput, ToolRef};

use serde_json::Value;
/// Invoke a tool by name using a [`ToolMap`] and [`WasixExecutor`].
pub async fn invoke_with_map(
    map: &ToolMap,
    executor: &WasixExecutor,
    name: &str,
    input_json: Value,
) -> Result<Value, McpError> {
    let tool = map.get(name)?;
    let input = ToolInput {
        payload: input_json,
    };
    let output = executor.invoke(tool, &input).await?;
    Ok(output.payload)
}

/// Convenience helper for loading a tool map from disk and building a [`ToolMap`].
pub fn load_tool_map(path: &std::path::Path) -> Result<ToolMap, McpError> {
    let config = load_tool_map_config(path)?;
    ToolMap::from_config(&config)
}
