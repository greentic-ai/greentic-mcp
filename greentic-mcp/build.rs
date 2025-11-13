use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set");
    let wasm_pkg = Path::new(&manifest_dir).join("tests/wasm/echo_tool");
    println!(
        "cargo:rerun-if-changed={}",
        wasm_pkg.join("Cargo.toml").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        wasm_pkg.join("src/lib.rs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        wasm_pkg.join("wit/tool.wit").display()
    );

    let target = "wasm32-wasip2";
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", target])
        .current_dir(&wasm_pkg)
        .status()
        .expect("failed to build echo wasm tool");
    if !status.success() {
        panic!("failed to compile echo wasm fixture");
    }

    let src = wasm_pkg
        .join("target")
        .join(target)
        .join("release")
        .join("echo_tool.wasm");
    let dest_dir = Path::new(&manifest_dir).join("tests/fixtures/echo_tool");
    fs::create_dir_all(&dest_dir).expect("failed to create fixture dir");
    fs::copy(&src, dest_dir.join("echo_tool.wasm"))
        .expect("failed to copy wasm fixture to tests/fixtures");
}
