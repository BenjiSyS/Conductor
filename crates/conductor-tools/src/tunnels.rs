//! Local port detection and development tunnels.
//!
//! Tunnels are explicit and short-lived: each has an owner (task/Goal), is
//! closed when that owner finishes, and is never left running by accident.
//! Remote links use `cloudflared` quick tunnels when installed.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListeningPort {
    pub port: u16,
    pub address: String,
    pub pid: Option<u32>,
}

/// Parse `netstat -ano` (Windows) / `ss -ltnp` / `lsof` style output for
/// listening TCP ports.
pub fn parse_listening(output: &str) -> Vec<ListeningPort> {
    let mut v = Vec::new();
    for line in output.lines() {
        let l = line.trim();
        let upper = l.to_uppercase();
        if !(upper.contains("LISTEN")) {
            continue;
        }
        let cols: Vec<&str> = l.split_whitespace().collect();
        // Find the first column that looks like addr:port.
        let Some(addr_col) = cols.iter().find(|c| {
            c.rsplit_once(':')
                .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
        }) else {
            continue;
        };
        let (addr, port) = addr_col.rsplit_once(':').expect("checked");
        let pid = cols.last().and_then(|c| c.parse::<u32>().ok()).or_else(|| {
            l.split("pid=")
                .nth(1)
                .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|n| n.parse().ok())
        });
        let port: u16 = port.parse().unwrap_or(0);
        if port == 0 || v.iter().any(|x: &ListeningPort| x.port == port) {
            continue;
        }
        v.push(ListeningPort {
            port,
            address: addr.trim_matches(['[', ']']).to_string(),
            pid,
        });
    }
    v.sort_by_key(|p| p.port);
    v
}

pub async fn listening_ports() -> Vec<ListeningPort> {
    let (prog, args): (&str, &[&str]) = if cfg!(windows) {
        ("netstat", &["-ano", "-p", "TCP"])
    } else if cfg!(target_os = "macos") {
        ("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN"])
    } else {
        ("ss", &["-ltnp"])
    };
    let req = crate::exec::ExecRequest::new(prog, args, std::env::temp_dir()).timeout(10);
    let r = crate::exec::run(req, tokio_util::sync::CancellationToken::new()).await;
    parse_listening(&r.stdout)
}

/// Ports commonly used by dev servers, for highlighting in the Preview Center.
pub fn is_dev_port(p: u16) -> bool {
    matches!(p, 1420 | 3000..=3010 | 4173 | 4200 | 5000 | 5173..=5180 | 8000 | 8080 | 8081 | 8888 | 9000)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TunnelInfo {
    pub id: String,
    pub port: u16,
    pub url: Option<String>,
    pub owner: String,
}

pub struct TunnelManager {
    tunnels: BTreeMap<String, (TunnelInfo, Child)>,
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            tunnels: BTreeMap::new(),
        }
    }

    pub fn available() -> bool {
        crate::exec::resolve_program("cloudflared").is_some()
    }

    /// Start a quick tunnel for a local port; waits up to 30s for the URL.
    pub async fn open(&mut self, port: u16, owner: &str) -> Result<TunnelInfo, String> {
        let exe =
            crate::exec::resolve_program("cloudflared").ok_or("cloudflared is not installed")?;
        let mut cmd = Command::new(exe);
        cmd.args([
            "tunnel",
            "--no-autoupdate",
            "--url",
            &format!("http://127.0.0.1:{port}"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let mut lines = BufReader::new(stderr).lines();
        let url = tokio::time::timeout(Duration::from_secs(30), async {
            while let Ok(Some(l)) = lines.next_line().await {
                if let Some(i) = l.find("https://") {
                    let u: String = l[i..]
                        .chars()
                        .take_while(|c| !c.is_whitespace() && *c != '|')
                        .collect();
                    if u.contains("trycloudflare.com") {
                        return Some(u);
                    }
                }
            }
            None
        })
        .await
        .ok()
        .flatten();
        let info = TunnelInfo {
            id: uuid::Uuid::new_v4().simple().to_string(),
            port,
            url,
            owner: owner.into(),
        };
        // Keep draining stderr so the process doesn't block.
        tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
        self.tunnels.insert(info.id.clone(), (info.clone(), child));
        Ok(info)
    }

    pub fn list(&self) -> Vec<TunnelInfo> {
        self.tunnels.values().map(|(i, _)| i.clone()).collect()
    }

    pub async fn close(&mut self, id: &str) -> bool {
        if let Some((_, mut c)) = self.tunnels.remove(id) {
            let _ = c.kill().await;
            true
        } else {
            false
        }
    }

    /// Close every tunnel owned by a finished task/Goal.
    pub async fn close_owned_by(&mut self, owner: &str) -> usize {
        let ids: Vec<String> = self
            .tunnels
            .iter()
            .filter(|(_, (i, _))| i.owner == owner)
            .map(|(k, _)| k.clone())
            .collect();
        for id in &ids {
            self.close(id).await;
        }
        ids.len()
    }

    pub async fn close_all(&mut self) {
        let ids: Vec<String> = self.tunnels.keys().cloned().collect();
        for id in ids {
            self.close(&id).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_netstat_and_ss() {
        let win = "  Proto  Local Address          Foreign Address        State           PID\n  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1234\n  TCP    127.0.0.1:5173         0.0.0.0:0              LISTENING       999\n  TCP    [::]:135               [::]:0                 LISTENING       1234\n  TCP    127.0.0.1:50000        1.2.3.4:443            ESTABLISHED     5\n";
        let p = parse_listening(win);
        assert_eq!(
            p.iter().map(|x| x.port).collect::<Vec<_>>(),
            vec![135, 5173]
        );
        assert_eq!(p[1].pid, Some(999));
        let ss = "State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process\nLISTEN 0 511 127.0.0.1:3000 0.0.0.0:* users:((\"node\",pid=4242,fd=20))\n";
        let p = parse_listening(ss);
        assert_eq!(p[0].port, 3000);
        assert_eq!(p[0].pid, Some(4242));
        assert!(is_dev_port(5173));
        assert!(!is_dev_port(135));
    }

    #[tokio::test]
    async fn detects_a_real_listener() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let ports = listening_ports().await;
        if ports.is_empty() {
            eprintln!("port listing tool unavailable; skipping");
            return;
        }
        assert!(
            ports.iter().any(|p| p.port == port),
            "port {port} not found"
        );
    }
}
