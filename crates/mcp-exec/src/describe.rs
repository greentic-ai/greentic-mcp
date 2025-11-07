use anyhow::{Context, Result};
use serde_json::Value;

use crate::{ExecConfig, ExecError, ExecRequest, exec};

#[cfg(feature = "describe-v1")]
const DESCRIBE_INTERFACE: &str = "greentic:component/describe-v1@1.0.0";
#[cfg(feature = "describe-v1")]
const DESCRIBE_EXPORT: &str = "greentic:component/describe-v1@1.0.0#describe-json";

#[derive(Debug)]
pub enum Maybe<T> {
    Data(T),
    Unsupported,
}

#[derive(Debug)]
pub struct ToolDescribe {
    pub describe_v1: Option<Value>,
    pub capabilities: Maybe<Vec<String>>,
    pub secrets: Maybe<Value>,
    pub config_schema: Maybe<Value>,
}

pub fn describe_tool(name: &str, cfg: &ExecConfig) -> Result<ToolDescribe> {
    #[cfg(feature = "describe-v1")]
    {
        if let Some(document) = try_describe_v1(name, cfg)? {
            return Ok(ToolDescribe {
                describe_v1: Some(document),
                capabilities: Maybe::Unsupported,
                secrets: Maybe::Unsupported,
                config_schema: Maybe::Unsupported,
            });
        }
    }

    fn try_action(name: &str, action: &str, cfg: &ExecConfig) -> Result<Maybe<Value>> {
        let req = ExecRequest {
            component: name.to_string(),
            action: action.to_string(),
            args: Value::Object(Default::default()),
            tenant: None,
        };

        match exec(req, cfg) {
            Ok(v) => Ok(Maybe::Data(v)),
            Err(ExecError::NotFound { .. }) => Ok(Maybe::Unsupported),
            Err(ExecError::Tool { code, payload, .. }) if code == "iface-error.not-found" => {
                let _ = payload;
                Ok(Maybe::Unsupported)
            }
            Err(e) => Err(e.into()),
        }
    }

    let capabilities_value = try_action(name, "capabilities", cfg)?;
    let secrets = try_action(name, "list_secrets", cfg)?;
    let config_schema = try_action(name, "config_schema", cfg)?;

    let capabilities = match capabilities_value {
        Maybe::Data(value) => {
            if let Some(arr) = value.as_array() {
                let list = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>();
                Maybe::Data(list)
            } else {
                Maybe::Data(Vec::new())
            }
        }
        Maybe::Unsupported => Maybe::Unsupported,
    };

    Ok(ToolDescribe {
        describe_v1: None,
        capabilities,
        secrets,
        config_schema,
    })
}

#[cfg(feature = "describe-v1")]
fn try_describe_v1(name: &str, cfg: &ExecConfig) -> Result<Option<Value>> {
    use wasmtime::component::{Component, Linker};
    use wasmtime::{Config, Engine, Store};

    let resolved =
        crate::resolve::resolve(name, &cfg.store).map_err(|err| ExecError::resolve(name, err))?;
    let verified = crate::verify::verify(name, resolved, &cfg.security)
        .map_err(|err| ExecError::verification(name, err))?;

    let mut config = Config::new();
    config.wasm_component_model(true);
    config.async_support(false);
    config.epoch_interruption(true);

    let engine = Engine::new(&config)?;
    let component = match Component::from_binary(&engine, verified.resolved.bytes.as_ref()) {
        Ok(component) => component,
        Err(_) => return Ok(None),
    };
    let linker = Linker::new(&engine);
    let mut store = Store::new(&engine, ());

    let instance = match linker.instantiate(&mut store, &component) {
        Ok(instance) => instance,
        Err(_) => return Ok(None),
    };
    if instance
        .get_export(&mut store, None, DESCRIBE_INTERFACE)
        .is_none()
    {
        return Ok(None);
    }

    let func = match instance.get_typed_func::<(), (String,)>(&mut store, DESCRIBE_EXPORT) {
        Ok(func) => func,
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("unknown export") {
                return Ok(None);
            }
            return Err(err);
        }
    };

    let (raw,) = func.call(&mut store, ())?;
    let value: Value =
        serde_json::from_str(&raw).with_context(|| "describe-json returned invalid JSON")?;
    Ok(Some(value))
}
