use std::fs;

use tokio::task::JoinError;
use tokio::time::{sleep, timeout};
use tracing::instrument;
use wasmtime::{Engine, Linker, Module, Store, Trap};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;

use crate::retry;
use crate::types::{McpError, ToolInput, ToolOutput, ToolRef};

const WASM_PAGE_SIZE: usize = 64 * 1024;

/// Executes WASIX/WASI tools compiled to WebAssembly.
#[derive(Clone)]
pub struct WasixExecutor {
    engine: Engine,
}

impl WasixExecutor {
    /// Construct a new executor using a synchronous engine.
    pub fn new() -> Result<Self, McpError> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(false);
        config.epoch_interruption(true);
        let engine = Engine::new(&config)
            .map_err(|err| McpError::Internal(format!("failed to create engine: {err}")))?;
        Ok(Self { engine })
    }

    /// Access the underlying Wasmtime engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Invoke the specified tool with the provided input payload.
    #[instrument(skip(self, tool, input), fields(tool = %tool.name))]
    pub async fn invoke(&self, tool: &ToolRef, input: &ToolInput) -> Result<ToolOutput, McpError> {
        let input_bytes = serde_json::to_vec(&input.payload)
            .map_err(|err| McpError::InvalidInput(err.to_string()))?;
        let attempts = tool.max_retries().saturating_add(1);
        let timeout_duration = tool.timeout();
        let base_backoff = tool.retry_backoff();

        for attempt in 0..attempts {
            let exec = self.exec_once(tool.clone(), input_bytes.clone());
            let result = if let Some(duration) = timeout_duration {
                match timeout(duration, exec).await {
                    Ok(res) => res,
                    Err(_) => return Err(McpError::timeout(&tool.name, duration)),
                }
            } else {
                exec.await
            };

            match result {
                Ok(bytes) => {
                    let payload = serde_json::from_slice(&bytes).map_err(|err| {
                        McpError::ExecutionFailed(format!("invalid tool output JSON: {err}"))
                    })?;
                    return Ok(ToolOutput { payload });
                }
                Err(InvocationFailure::Transient(msg)) => {
                    if attempt + 1 >= attempts {
                        return Err(McpError::Transient(tool.name.clone(), msg));
                    }
                    let backoff = retry::backoff(base_backoff, attempt);
                    tracing::debug!(attempt, ?backoff, "transient failure, retrying");
                    sleep(backoff).await;
                }
                Err(InvocationFailure::Fatal(err)) => return Err(err),
            }
        }

        Err(McpError::Internal("unreachable retry loop".into()))
    }

    async fn exec_once(&self, tool: ToolRef, input: Vec<u8>) -> Result<Vec<u8>, InvocationFailure> {
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || invoke_blocking(engine, tool, input))
            .await
            .map_err(|err| join_error(err, "spawn_blocking failed"))?
    }
}

impl Default for WasixExecutor {
    fn default() -> Self {
        Self::new().expect("engine construction should succeed")
    }
}

fn join_error(err: JoinError, context: &str) -> InvocationFailure {
    InvocationFailure::Fatal(McpError::Internal(format!("{context}: {err}")))
}

enum InvocationFailure {
    Transient(String),
    Fatal(McpError),
}

impl InvocationFailure {
    fn transient(msg: impl Into<String>) -> Self {
        Self::Transient(msg.into())
    }

    fn fatal(err: impl Into<McpError>) -> Self {
        Self::Fatal(err.into())
    }
}

fn invoke_blocking(
    engine: Engine,
    tool: ToolRef,
    input: Vec<u8>,
) -> Result<Vec<u8>, InvocationFailure> {
    // TODO: support component/WIT pathway using greentic-interfaces bindings.
    let module_bytes = fs::read(tool.component_path()).map_err(|err| {
        InvocationFailure::fatal(McpError::ExecutionFailed(format!(
            "failed to read `{}`: {err}",
            tool.component
        )))
    })?;
    let module = Module::from_binary(&engine, &module_bytes).map_err(|err| {
        InvocationFailure::fatal(McpError::ExecutionFailed(format!(
            "failed to compile `{}`: {err}",
            tool.component
        )))
    })?;

    let mut linker: Linker<WasiState> = Linker::new(&engine);
    p1::add_to_linker_sync(&mut linker, |state: &mut WasiState| &mut state.wasi).map_err(
        |err| {
            InvocationFailure::fatal(McpError::Internal(format!(
                "failed to link WASI imports: {err}"
            )))
        },
    )?;

    let pre = linker.instantiate_pre(&module).map_err(|err| {
        InvocationFailure::fatal(McpError::ExecutionFailed(format!(
            "failed to prepare `{}`: {err}",
            tool.component
        )))
    })?;

    let mut store = Store::new(&engine, WasiState::new());
    let instance = pre
        .instantiate(&mut store)
        .map_err(|err| classify(err, &tool))?;

    let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
        InvocationFailure::fatal(McpError::ExecutionFailed(
            "guest lacks exported memory".into(),
        ))
    })?;

    let input_len = i32::try_from(input.len()).map_err(|_| {
        InvocationFailure::fatal(McpError::InvalidInput("input too large for wasm32".into()))
    })?;
    let input_ptr = allocate(&memory, &mut store, input.len()).map_err(|err| {
        InvocationFailure::fatal(McpError::Internal(format!(
            "failed to allocate guest memory: {err}"
        )))
    })?;

    memory
        .write(&mut store, input_ptr as usize, &input)
        .map_err(|err| {
            InvocationFailure::fatal(McpError::Internal(format!(
                "failed to write guest memory: {err}"
            )))
        })?;

    let pair_call = instance.get_typed_func::<(i32, i32), (i32, i32)>(&mut store, &tool.entry);
    let (out_ptr, out_len) = match pair_call {
        Ok(func) => match func.call(&mut store, (input_ptr, input_len)) {
            Ok(v) => v,
            Err(err) => return Err(classify(err, &tool)),
        },
        Err(pair_err) => {
            let packed = instance
                .get_typed_func::<(i32, i32), i64>(&mut store, &tool.entry)
                .map_err(|_| {
                    InvocationFailure::fatal(McpError::ExecutionFailed(format!(
                        "missing entry `{}`: {pair_err}",
                        tool.entry
                    )))
                })?;
            let packed_value = match packed.call(&mut store, (input_ptr, input_len)) {
                Ok(val) => val,
                Err(err) => return Err(classify(err, &tool)),
            };
            let ptr = (packed_value & 0xFFFF_FFFF) as i32;
            let len = (packed_value >> 32) as i32;
            (ptr, len)
        }
    };

    if out_ptr < 0 || out_len < 0 {
        return Err(InvocationFailure::fatal(McpError::ExecutionFailed(
            "guest returned negative pointer or length".into(),
        )));
    }

    let out_len_usize = out_len as usize;
    let mut buffer = vec![0u8; out_len_usize];
    memory
        .read(&mut store, out_ptr as usize, &mut buffer)
        .map_err(|err| {
            InvocationFailure::fatal(McpError::ExecutionFailed(format!(
                "failed to read guest output: {err}"
            )))
        })?;

    Ok(buffer)
}

fn allocate(
    memory: &wasmtime::Memory,
    store: &mut Store<WasiState>,
    len: usize,
) -> Result<i32, wasmtime::Error> {
    let current_bytes = memory.data_size(&*store);
    let required = current_bytes
        .checked_add(len)
        .ok_or_else(|| wasmtime::Error::msg("guest memory overflow"))?;

    ensure_capacity(memory, store, required)?;
    let ptr = i32::try_from(current_bytes)
        .map_err(|_| wasmtime::Error::msg("guest memory exceeds wasm32 address space"))?;
    Ok(ptr)
}

fn ensure_capacity(
    memory: &wasmtime::Memory,
    store: &mut Store<WasiState>,
    required_bytes: usize,
) -> Result<(), wasmtime::Error> {
    let mut current_pages = memory.size(&*store);
    let required_pages = required_bytes.div_ceil(WASM_PAGE_SIZE) as u64;
    if required_pages > current_pages {
        let grow = required_pages - current_pages;
        memory.grow(store, grow)?;
        current_pages += grow;
        tracing::debug!(
            pages = grow,
            total_pages = current_pages,
            "grew guest memory for argument/response buffers"
        );
    }
    Ok(())
}

fn classify(err: wasmtime::Error, tool: &ToolRef) -> InvocationFailure {
    if err.downcast_ref::<Trap>().is_some() {
        InvocationFailure::transient(err.to_string())
    } else {
        InvocationFailure::fatal(McpError::ExecutionFailed(format!(
            "tool `{}` failed: {err}",
            tool.name
        )))
    }
}

struct WasiState {
    wasi: WasiP1Ctx,
}

impl WasiState {
    fn new() -> Self {
        let mut builder = WasiCtxBuilder::new();
        builder.inherit_stdio();
        builder.inherit_env();
        builder.allow_blocking_current_thread(true);
        Self {
            wasi: builder.build_p1(),
        }
    }
}
