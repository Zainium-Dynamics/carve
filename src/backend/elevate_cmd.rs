// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Privilege helper — **never sudo / never classic Linux PAM**.
// Uses setuid-root `elevate` (Zainium elevate stack) when euid != 0.
// See: zex-native/elevate (elevate-sudo + elevate-pam + elevate-crypto).

use std::process::{Command, Output, Stdio};

/// True if we already run as root (euid 0).
pub fn is_root() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Resolve elevate binary: env → PATH → syshub paths (no /usr).
pub fn elevate_bin() -> Option<String> {
    if let Ok(p) = std::env::var("ZAINIUM_ELEVATE") {
        if !p.is_empty() && std::path::Path::new(&p).is_file() {
            return Some(p);
        }
    }
    for c in [
        "elevate",
        "/bin/elevate",
        "/overlayer/syshub/bin/elevate",
        "/sbin/elevate",
    ] {
        if which(c) || (c.starts_with('/') && std::path::Path::new(c).is_file()) {
            return Some(c.to_string());
        }
    }
    None
}

/// Run a command as root. Prefer direct exec if root; else `elevate <cmd>…`.
/// Never calls `sudo`.
pub fn run_root(program: &str, args: &[&str]) -> Result<Output, String> {
    if is_root() {
        return Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("exec {program}: {e}"));
    }
    let elev = elevate_bin().ok_or_else(|| {
        "not root and `elevate` not found (install Zainium elevate — no sudo on this OS)".to_string()
    })?;
    let mut full = Vec::with_capacity(1 + args.len());
    full.push(program);
    full.extend_from_slice(args);
    Command::new(&elev)
        .args(&full)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("elevate {program}: {e}"))
}

/// Run and require success exit status.
pub fn run_root_ok(program: &str, args: &[&str]) -> Result<(), String> {
    let out = run_root(program, args)?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} {:?} failed (status {:?}): {}",
            args,
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// Capture stdout on success.
pub fn run_root_stdout(program: &str, args: &[&str]) -> Result<String, String> {
    let out = run_root(program, args)?;
    if !out.status.success() {
        return Err(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// True if `bin` resolves on PATH (or is a file, for an absolute path).
pub fn which(bin: &str) -> bool {
    if bin.contains('/') {
        return std::path::Path::new(bin).is_file();
    }
    Command::new("sh")
        .args(["-c", &format!("command -v {bin} >/dev/null 2>&1")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
