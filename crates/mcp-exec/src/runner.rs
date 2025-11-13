//! Runtime integration with Wasmtime for invoking the MCP component entrypoint.

use std::thread;
use std::time::Instant;

use greentic_interfaces::runner_host_v1::{self as runner_host, RunnerHost};
use serde_json::Value;
use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

use crate::ExecRequest;
use crate::config::RuntimePolicy;
use crate::error::RunnerError;
use crate::verify::VerifiedArtifact;
use tokio::runtime::Builder;
use tokio::task;
use tokio::time::timeout;

pub struct ExecutionContext<'a> {
    pub runtime: &'a RuntimePolicy,
    pub http_enabled: bool,
}

pub trait Runner: Send + Sync {
    fn run(
        &self,
        request: &ExecRequest,
        artifact: &VerifiedArtifact,
        ctx: ExecutionContext<'_>,
    ) -> Result<Value, RunnerError>;
}

pub struct DefaultRunner {
    engine: Engine,
}

impl DefaultRunner {
    pub fn new(runtime: &RuntimePolicy) -> Result<Self, RunnerError> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(false);
        // Epoch interruption lets us wire wallclock enforcement without embedding async support.
        config.epoch_interruption(true);
        if runtime.fuel.is_some() {
            config.consume_fuel(true);
        }
        let engine = Engine::new(&config)?;
        Ok(Self { engine })
    }
}

impl Runner for DefaultRunner {
    fn run(
        &self,
        request: &ExecRequest,
        artifact: &VerifiedArtifact,
        ctx: ExecutionContext<'_>,
    ) -> Result<Value, RunnerError> {
        let engine = self.engine.clone();
        let request = request.clone();
        let artifact = artifact.clone();
        let runtime = ctx.runtime.clone();
        let http_enabled = ctx.http_enabled;
        let timeout_duration = runtime.per_call_timeout;

        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| {
                RunnerError::Internal(format!("failed to build timeout runtime: {err}"))
            })?;

        let future = async move {
            let run_future = async {
                let handle = task::spawn_blocking(move || {
                    run_sync(engine, request, artifact, runtime, http_enabled)
                });
                match handle.await {
                    Ok(result) => result,
                    Err(err) => Err(RunnerError::Internal(format!(
                        "blocking runner task failed: {err}"
                    ))),
                }
            };

            match timeout(timeout_duration, run_future).await {
                Ok(result) => result,
                Err(_) => Err(RunnerError::Timeout {
                    elapsed: timeout_duration,
                }),
            }
        };

        match thread::spawn(move || tokio_runtime.block_on(future)).join() {
            Ok(result) => result,
            Err(err) => Err(RunnerError::Internal(format!("runtime panicked: {err:?}"))),
        }
    }
}

fn run_sync(
    engine: Engine,
    request: ExecRequest,
    artifact: VerifiedArtifact,
    runtime: RuntimePolicy,
    http_enabled: bool,
) -> Result<Value, RunnerError> {
    let component = match Component::from_binary(&engine, artifact.resolved.bytes.as_ref()) {
        Ok(component) => component,
        Err(err) => {
            if let Some(result) = try_mock_json(artifact.resolved.bytes.as_ref(), &request.action) {
                return result;
            }
            return Err(err.into());
        }
    };

    let mut linker = Linker::new(&engine);
    linker.allow_shadowing(true);
    runner_host::add_to_linker(&mut linker, |state: &mut StoreState| state)
        .map_err(RunnerError::from)?;

    let mut store = Store::new(&engine, StoreState::new(http_enabled));

    let instance = linker.instantiate(&mut store, &component)?;
    let exec = instance.get_typed_func::<(String, String), (String,)>(&mut store, "exec")?;

    let args_json = serde_json::to_string(&request.args)?;
    let started = Instant::now();
    let (raw_response,) = exec.call(&mut store, (request.action.clone(), args_json))?;

    if started.elapsed() > runtime.wallclock_timeout {
        return Err(RunnerError::Timeout {
            elapsed: started.elapsed(),
        });
    }

    let value: Value = serde_json::from_str(&raw_response)?;
    Ok(value)
}

struct StoreState {
    http_enabled: bool,
    http_client: Option<reqwest::blocking::Client>,
}

impl StoreState {
    fn new(http_enabled: bool) -> Self {
        Self {
            http_enabled,
            http_client: None,
        }
    }

    fn http_client(&mut self) -> Result<&reqwest::blocking::Client, String> {
        if !self.http_enabled {
            return Err("http-disabled".into());
        }

        if self.http_client.is_none() {
            // Lazily construct a blocking client so hosts that never expose
            // outbound HTTP do not pay the initialization cost.
            let client = reqwest::blocking::Client::builder()
                .use_rustls_tls()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|err| format!("http-client: {err}"))?;
            self.http_client = Some(client);
        }

        Ok(self.http_client.as_ref().expect("client initialized"))
    }
}

impl RunnerHost for StoreState {
    fn http_request(
        &mut self,
        method: String,
        url: String,
        headers: Vec<String>,
        body: Option<Vec<u8>>,
    ) -> wasmtime::Result<Result<Vec<u8>, String>> {
        if !self.http_enabled {
            return Ok(Err("http-disabled".into()));
        }

        use reqwest::Method;

        let client = match self.http_client() {
            Ok(client) => client,
            Err(err) => return Ok(Err(err)),
        };

        let method = match Method::from_bytes(method.as_bytes()) {
            Ok(method) => method,
            Err(_) => return Ok(Err("invalid-method".into())),
        };

        let builder = client.request(method, &url);
        let mut builder = match apply_headers(builder, &headers) {
            Ok(builder) => builder,
            Err(err) => return Ok(Err(err)),
        };

        if let Some(body) = body {
            builder = builder.body(body);
        }

        let response = match builder.send() {
            Ok(resp) => resp,
            Err(err) => return Ok(Err(format!("request: {err}"))),
        };

        if !response.status().is_success() {
            return Ok(Err(format!("status-{}", response.status().as_u16())));
        }

        match response.bytes() {
            Ok(bytes) => Ok(Ok(bytes.to_vec())),
            Err(err) => Ok(Err(format!("body: {err}"))),
        }
    }

    fn secret_get(&mut self, _name: String) -> wasmtime::Result<Result<String, String>> {
        Ok(Err("secrets-disabled".into()))
    }

    fn kv_get(&mut self, _ns: String, _key: String) -> wasmtime::Result<Option<String>> {
        Ok(None)
    }

    fn kv_put(&mut self, _ns: String, _key: String, _val: String) -> wasmtime::Result<()> {
        Ok(())
    }
}

fn apply_headers(
    mut builder: reqwest::blocking::RequestBuilder,
    headers: &[String],
) -> Result<reqwest::blocking::RequestBuilder, String> {
    use reqwest::header::{HeaderName, HeaderValue};

    for header in headers {
        let (name, value) = header
            .split_once(':')
            .ok_or_else(|| format!("invalid-header:{header}"))?;
        let header_name = HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|_| format!("invalid-header-name:{}", name.trim()))?;
        let header_value = HeaderValue::from_str(value.trim())
            .map_err(|_| format!("invalid-header-value:{header}"))?;
        builder = builder.header(header_name, header_value);
    }

    Ok(builder)
}

fn try_mock_json(bytes: &[u8], action: &str) -> Option<Result<Value, RunnerError>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let root: Value = serde_json::from_str(text).ok()?;

    if !root
        .get("_mock_mcp_exec")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }

    let responses = root.get("responses")?.as_object()?;
    match responses.get(action) {
        Some(value) => Some(Ok(value.clone())),
        None => Some(Err(RunnerError::ActionNotFound {
            action: action.to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn http_request_requires_flag() {
        let mut state = StoreState::new(false);
        let result = state
            .http_request("GET".into(), "https://example.com".into(), Vec::new(), None)
            .expect("request should run");
        assert!(matches!(result, Err(err) if err == "http-disabled"));
    }

    #[test]
    fn http_request_rejects_invalid_method() {
        let mut state = StoreState::new(true);
        let result = state
            .http_request("???".into(), "https://example.com".into(), Vec::new(), None)
            .expect("request should run");
        assert!(matches!(result, Err(err) if err == "invalid-method"));
    }

    #[test]
    fn secret_get_is_disabled() {
        let mut state = StoreState::new(true);
        let result = state
            .secret_get("api-key".into())
            .expect("call should succeed");
        assert!(matches!(result, Err(err) if err == "secrets-disabled"));
    }
}
