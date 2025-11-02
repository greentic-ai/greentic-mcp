use serde_json::Value;
use std::thread;
use std::time::Duration;
use std::sync::atomic::{AtomicU32, Ordering};

static FLAKY_ATTEMPTS: AtomicU32 = AtomicU32::new(0);

#[no_mangle]
pub extern "C" fn tool_invoke(ptr: i32, len: i32) -> (i32, i32) {
    let input = unsafe { read_guest(ptr, len) };
    let value: Value = serde_json::from_slice(input).expect("valid json input");

    if matches!(value.get("fail").and_then(Value::as_str), Some("transient")) {
        panic!("transient failure requested");
    }

    if let Some(ms) = value.get("sleep_ms").and_then(Value::as_u64) {
        thread::sleep(Duration::from_millis(ms));
    }

    if value.get("flaky").and_then(Value::as_bool) == Some(true) {
        let attempt = FLAKY_ATTEMPTS.fetch_add(1, Ordering::SeqCst);
        if attempt % 2 == 0 {
            panic!("flaky tool induced transient failure");
        }
    }

    let payload = serde_json::to_vec(&value).expect("serde to encode rest");
    leak(payload)
}

unsafe fn read_guest(ptr: i32, len: i32) -> &'static [u8] {
    let ptr = ptr as usize;
    let len = len as usize;
    let mem = ptr as *const u8;
    std::slice::from_raw_parts(mem, len)
}

fn leak(mut bytes: Vec<u8>) -> (i32, i32) {
    let ptr = bytes.as_mut_ptr();
    let len = bytes.len();
    std::mem::forget(bytes);
    (ptr as i32, len as i32)
}
