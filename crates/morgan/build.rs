//! Datos de la compilación para la ficha de `morgan about`: el commit y la versión de Rust.
//! Sin git (un tarball) el commit queda vacío y la ficha no lo muestra.

use std::process::Command;

fn salida(programa: &str, args: &[&str]) -> String {
    Command::new(programa)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

fn main() {
    let commit = salida("git", &["rev-parse", "--short", "HEAD"]);
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    // "rustc 1.99.0 (abc123 2026-…)"
    let version = salida(&rustc, &["--version"]);
    let version = version.split_whitespace().nth(1).unwrap_or("");
    println!("cargo:rustc-env=MORGAN_COMMIT={commit}");
    println!("cargo:rustc-env=MORGAN_RUSTC={version}");
    // El commit cambia con cada commit: se vuelve a correr cuando se mueve HEAD o una rama.
    let git = salida("git", &["rev-parse", "--absolute-git-dir"]);
    if !git.is_empty() {
        for p in ["HEAD", "refs/heads", "packed-refs"] {
            println!("cargo:rerun-if-changed={git}/{p}");
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
