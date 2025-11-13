use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static FLAKY_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

pub fn echo(req: &Value) -> Result<Value, String> {
    Ok(req.clone())
}

pub fn flaky_echo(req: &Value) -> Result<Value, String> {
    let attempt = FLAKY_ATTEMPTS.fetch_add(1, Ordering::SeqCst);
    if attempt < 2 {
        Err("transient.echo".to_string())
    } else {
        Ok(req.clone())
    }
}

pub fn timeout_echo(req: &Value, sleep: Duration) -> Result<Value, String> {
    std::thread::sleep(sleep);
    Ok(req.clone())
}
