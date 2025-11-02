use std::fs;
use std::path::Path;

use crate::types::{McpError, ToolMapConfig};

/// Load a [`ToolMapConfig`] from JSON or YAML.
pub fn load_tool_map_config(path: &Path) -> Result<ToolMapConfig, McpError> {
    let content = fs::read_to_string(path)?;
    parse_tool_map_config(path, &content)
}

fn parse_tool_map_config(path: &Path, content: &str) -> Result<ToolMapConfig, McpError> {
    if is_json(path, content) {
        Ok(serde_json::from_str(content)?)
    } else {
        Ok(serde_yaml_bw::from_str(content)?)
    }
}

fn is_json(path: &Path, content: &str) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if matches!(ext, "json") {
            return true;
        }
        if matches!(ext, "yaml" | "yml") {
            return false;
        }
    }

    content
        .chars()
        .find(|c| !c.is_whitespace())
        .is_some_and(|c| c == '{' || c == '[')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json() {
        let config = parse_tool_map_config(
            Path::new("config.json"),
            r#"{"tools":[{"name":"echo","component":"./echo.wasm","entry":"tool_invoke"}]}"#,
        )
        .unwrap();

        assert_eq!(config.tools.len(), 1);
        assert_eq!(config.tools[0].name, "echo");
    }

    #[test]
    fn parses_yaml() {
        let config = parse_tool_map_config(
            Path::new("config.yaml"),
            r#"
tools:
  - name: echo
    component: ./echo.wasm
    entry: tool_invoke
        "#,
        )
        .unwrap();

        assert_eq!(config.tools.len(), 1);
        assert_eq!(config.tools[0].name, "echo");
    }
}
