use indexmap::IndexMap;

use crate::types::{McpError, ToolMapConfig, ToolRef};

/// Name to [`ToolRef`] lookup.
#[derive(Clone, Debug)]
pub struct ToolMap {
    tools: IndexMap<String, ToolRef>,
}

impl ToolMap {
    /// Build a [`ToolMap`] from a configuration file.
    pub fn from_config(config: &ToolMapConfig) -> Result<Self, McpError> {
        let mut tools = IndexMap::with_capacity(config.tools.len());
        for tool in &config.tools {
            if tools.contains_key(&tool.name) {
                return Err(McpError::InvalidInput(format!(
                    "duplicate tool name `{}`",
                    tool.name
                )));
            }
            tools.insert(tool.name.clone(), tool.clone());
        }

        Ok(ToolMap { tools })
    }

    /// Retrieve a tool by name.
    pub fn get(&self, name: &str) -> Result<&ToolRef, McpError> {
        self.tools
            .get(name)
            .ok_or_else(|| McpError::tool_not_found(name.to_string()))
    }

    /// Iterate over desired tool references.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &ToolRef)> {
        self.tools.iter()
    }
}
