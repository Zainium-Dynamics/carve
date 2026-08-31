// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Thin client for quantra-netd's existing IPC — same wire format
// quantra-net (its own CLI) and quantra-ctl use elsewhere: length-prefixed
// JSON over a Unix socket. No new protocol invented here, and no fake
// interface/network data if the daemon isn't reachable — callers get a
// clear error and the wizard's network step is honestly skipped.

use std::{os::unix::net::UnixStream, path::Path, time::Duration};

use quantra_net_common::{
    recv_message_sync, send_message_sync, DhcpLeaseInfo, InterfaceInfo, NetCommand, NetResponse,
    WifiNetwork, WifiSecurity, SOCKET_PATH,
};

fn connect() -> Result<UnixStream, String> {
    if !Path::new(SOCKET_PATH).exists() {
        return Err(format!(
            "quantra-netd not running (no socket at {SOCKET_PATH}) — network step unavailable"
        ));
    }
    let stream = UnixStream::connect(SOCKET_PATH)
        .map_err(|e| format!("connect {SOCKET_PATH}: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(10))).ok();
    Ok(stream)
}

fn call(cmd: NetCommand) -> Result<NetResponse, String> {
    let mut stream = connect()?;
    send_message_sync(&mut stream, &cmd).map_err(|e| format!("send: {e}"))?;
    recv_message_sync(&mut stream).map_err(|e| format!("recv: {e}"))
}

pub fn is_available() -> bool {
    Path::new(SOCKET_PATH).exists()
}

pub fn list_interfaces() -> Result<Vec<InterfaceInfo>, String> {
    match call(NetCommand::Status { verbose: false })? {
        NetResponse::Status(ifaces, ..) => Ok(ifaces),
        NetResponse::Error(e) => Err(e),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

pub fn dhcp_acquire(interface: &str) -> Result<DhcpLeaseInfo, String> {
    match call(NetCommand::DhcpAcquire(interface.to_string()))? {
        NetResponse::DhcpLease(lease) => Ok(lease),
        NetResponse::Error(e) => Err(e),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

pub fn wifi_scan(interface: &str) -> Result<Vec<WifiNetwork>, String> {
    match call(NetCommand::WifiScan {
        interface: interface.to_string(),
    })? {
        NetResponse::WifiNetworks(nets) => Ok(nets),
        NetResponse::Error(e) => Err(e),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

pub fn wifi_connect(
    interface: &str,
    ssid: &str,
    password: Option<String>,
    security: WifiSecurity,
) -> Result<(), String> {
    match call(NetCommand::WifiConnect {
        interface: interface.to_string(),
        ssid: ssid.to_string(),
        password,
        security,
        hidden: false,
    })? {
        NetResponse::Success(_) => Ok(()),
        NetResponse::Error(e) => Err(e),
        other => Err(format!("unexpected response: {other:?}")),
    }
}
