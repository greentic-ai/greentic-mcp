use std::time::Instant;

use serde_json::Value;
use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

use crate::ExecRequest;
use crate::config::RuntimePolicy;
use crate::error::RunnerError;
use crate::verify::VerifiedArtifact;

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
        let component = Component::from_binary(&self.engine, artifact.resolved.bytes.as_ref())?;
        let mut linker = Linker::new(&self.engine);

        // TODO: wire up greentic-interfaces host imports. For now we provide no-op fallbacks
        // so that components without these imports can still run.
        linker.allow_shadowing(true);

        let mut store = Store::new(
            &self.engine,
            StoreState::new(ctx.runtime.clone(), ctx.http_enabled),
        );

        let instance = linker.instantiate(&mut store, &component)?;
        let exec = instance.get_typed_func::<(String, String), (String,)>(&mut store, "exec")?;

        let args_json = serde_json::to_string(&request.args)?;
        let started = Instant::now();
        let (raw_response,) = exec.call(&mut store, (request.action.clone(), args_json))?;

        if started.elapsed() > ctx.runtime.wallclock_timeout {
            return Err(RunnerError::Timeout {
                elapsed: started.elapsed(),
            });
        }

        let value: Value = serde_json::from_str(&raw_response)?;
        Ok(value)
    }
}

#[allow(dead_code)]
struct StoreState {
    runtime: RuntimePolicy,
    http_enabled: bool,
}

impl StoreState {
    fn new(runtime: RuntimePolicy, http_enabled: bool) -> Self {
        Self {
            runtime,
            http_enabled,
        }
    }
}
