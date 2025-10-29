use serde::{Deserialize, Serialize};

/// Placeholder tenant context that can be expanded with real multi-tenant data.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TenantCtx {
    pub tenant_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
}
