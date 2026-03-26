// src-tauri/src/main.rs
// Tauri backend: scans open ports via `lsof` and kills processes via `kill`

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

// ─── Data structures ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortEntry {
    pub pid: u32,
    pub process: String,
    pub port: u16,
    pub protocol: String,
    pub state: String,
    pub user: String,
    pub command: String,
    pub category: String,
}

#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub ports: Vec<PortEntry>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct KillResult {
    pub success: bool,
    pub killed: Vec<u32>,
    pub failed: Vec<KillFailure>,
}

#[derive(Debug, Serialize)]
pub struct KillFailure {
    pub pid: u32,
    pub reason: String,
}

// ─── Port categorization ────────────────────────────────────────────────────

fn categorize_port(process: &str, port: u16, user: &str) -> String {
    let proc_lower = process.to_lowercase();

    // Databases
    let db_procs = [
        "postgres", "mysqld", "mysql", "redis-server", "redis", "mongod",
        "mongos", "memcached", "cassandra", "couchdb", "neo4j",
        "elasticsearch", "clickhouse",
    ];
    if db_procs.iter().any(|p| proc_lower.contains(p))
        || [5432, 3306, 6379, 27017, 11211, 9042, 5984, 7474, 9200, 8123].contains(&port)
    {
        return "Databases".to_string();
    }

    // Containers / orchestration
    let container_procs = ["docker", "containerd", "kubelet", "kubectl", "podman"];
    if container_procs.iter().any(|p| proc_lower.contains(p))
        || [2375, 2376, 2377, 10250, 10255].contains(&port)
    {
        return "Containers".to_string();
    }

    // Web servers / proxies
    let web_procs = ["nginx", "httpd", "apache", "caddy", "traefik", "haproxy", "envoy"];
    if web_procs.iter().any(|p| proc_lower.contains(p)) || [80, 443, 8443].contains(&port) {
        return "Web / Proxy".to_string();
    }

    // Dev servers
    let dev_procs = [
        "node", "python", "python3", "ruby", "java", "go", "deno", "bun",
        "php", "uvicorn", "gunicorn", "flask", "next", "vite", "webpack",
        "esbuild", "grafana", "prometheus",
    ];
    if dev_procs.iter().any(|p| proc_lower.contains(p))
        || (3000..=3999).contains(&port)
        || (4000..=4999).contains(&port)
        || (5000..=5999).contains(&port)
        || (8000..=8099).contains(&port)
        || (8080..=8089).contains(&port)
        || (9000..=9099).contains(&port)
    {
        return "Dev Servers".to_string();
    }

    // Desktop apps
    let app_procs = [
        "spotify", "slack", "discord", "zoom", "teams", "telegram",
        "signal", "whatsapp", "dropbox", "1password", "chrome",
        "firefox", "safari", "brave", "arc", "figma", "notion",
        "obsidian", "vscode", "code",
    ];
    if app_procs.iter().any(|p| proc_lower.contains(p)) || port > 49152 {
        return "Apps".to_string();
    }

    // System
    let system_procs = ["sshd", "ssh", "launchd", "mDNSResponder", "systemd", "cupsd"];
    if system_procs.iter().any(|p| proc_lower.contains(p))
        || user == "root"
        || user.starts_with('_')
        || [22, 53, 631].contains(&port)
    {
        return "System".to_string();
    }

    "Other".to_string()
}

// ─── Parse lsof -F output ────────────────────────────────────────────────────
// Uses lsof's machine-readable -F (field) output format instead of fragile
// column-based parsing. Each line starts with a single-char field identifier:
//   p = PID, c = command name, L = login name (user),
//   P = protocol, n = name (addr:port), T = TCP state (TST=LISTEN etc.)

struct LsofRecord {
    pid: u32,
    process: String,
    user: String,
    protocol: String,
    port: u16,
    state: String,
}

fn parse_lsof_field_output(output: &str) -> Vec<LsofRecord> {
    let mut records: Vec<LsofRecord> = Vec::new();

    // Current process-level fields (persist across file descriptors)
    let mut cur_pid: u32 = 0;
    let mut cur_process = String::new();
    let mut cur_user = String::new();

    // Current file-descriptor-level fields (reset on each new 'f' line)
    let mut cur_protocol = String::new();
    let mut cur_port: u16 = 0;
    let mut cur_state = String::new();

    for line in output.lines() {
        if line.is_empty() {
            continue;
        }

        let field_type = line.as_bytes()[0] as char;
        let value = &line[1..];

        match field_type {
            'p' => {
                // New process — flush any pending fd record first
                cur_pid = value.parse().unwrap_or(0);
                cur_process.clear();
                cur_user.clear();
                cur_protocol.clear();
                cur_port = 0;
                cur_state.clear();
            }
            'c' => cur_process = value.to_string(),
            'L' => cur_user = value.to_string(),
            'f' => {
                // New file descriptor — reset fd-level fields
                cur_protocol.clear();
                cur_port = 0;
                cur_state.clear();
            }
            'P' => cur_protocol = value.to_uppercase(),
            'n' => {
                // Name field: *:PORT, 127.0.0.1:PORT, [::1]:PORT, etc.
                // Extract the port number from after the last ':'
                if let Some(port_str) = value.rsplit(':').next() {
                    if let Ok(p) = port_str.parse::<u16>() {
                        cur_port = p;
                    }
                }
            }
            'T' => {
                // TCP info field, e.g. "TST=LISTEN" or "TST=ESTABLISHED"
                if let Some(st) = value.strip_prefix("ST=") {
                    cur_state = st.to_string();
                }
            }
            _ => {}
        }

        // After processing a T (state) or n (name) line, check if we have a complete LISTEN record
        if (field_type == 'T' || field_type == 'n') && cur_port > 0 && cur_pid > 0 {
            // For TCP we require LISTEN state; for UDP there's no state so accept any
            let dominated_by_listen = cur_state == "LISTEN" || cur_protocol == "UDP";
            if dominated_by_listen {
                records.push(LsofRecord {
                    pid: cur_pid,
                    process: cur_process.clone(),
                    user: cur_user.clone(),
                    protocol: if cur_protocol.is_empty() { "TCP".to_string() } else { cur_protocol.clone() },
                    port: cur_port,
                    state: if cur_state.is_empty() { "LISTEN".to_string() } else { cur_state.clone() },
                });
                // Reset fd-level so we don't duplicate
                cur_port = 0;
                cur_state.clear();
            }
        }
    }

    records
}

fn parse_lsof_to_entries(output: &str) -> Vec<PortEntry> {
    let records = parse_lsof_field_output(output);
    let mut entries: Vec<PortEntry> = Vec::new();
    let mut seen: HashMap<(u32, u16), bool> = HashMap::new();

    for r in records {
        if r.port == 0 || seen.contains_key(&(r.pid, r.port)) {
            continue;
        }
        seen.insert((r.pid, r.port), true);

        let command = get_command_for_pid(r.pid)
            .unwrap_or_else(|| format!("{} (PID {})", r.process, r.pid));
        let category = categorize_port(&r.process, r.port, &r.user);

        entries.push(PortEntry {
            pid: r.pid,
            process: r.process,
            port: r.port,
            protocol: r.protocol,
            state: r.state,
            user: r.user,
            command,
            category,
        });
    }

    entries.sort_by(|a, b| a.category.cmp(&b.category).then(a.port.cmp(&b.port)));
    entries
}

fn get_command_for_pid(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;

    let cmd = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if cmd.is_empty() {
        None
    } else {
        // Truncate very long command lines
        Some(if cmd.len() > 200 {
            format!("{}…", &cmd[..200])
        } else {
            cmd
        })
    }
}

// ─── Tauri commands ─────────────────────────────────────────────────────────

#[tauri::command]
fn scan_ports() -> ScanResult {
    // Run lsof with -F for machine-readable field output
    // -i = network files, -P = no port name resolution, -n = no host name resolution
    // -F pcLPnT = output fields: PID, command, login user, protocol, name, TCP state
    let output = Command::new("lsof")
        .args(["-i", "-P", "-n", "-F", "pcLfPnT"])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let ports = parse_lsof_to_entries(&stdout);
            ScanResult {
                ports,
                error: None,
            }
        }
        Err(e) => ScanResult {
            ports: vec![],
            error: Some(format!("Failed to run lsof: {}", e)),
        },
    }
}

#[tauri::command]
fn kill_ports(pids: Vec<u32>, force: bool) -> KillResult {
    let signal = if force { "-9" } else { "-15" };
    let mut killed: Vec<u32> = Vec::new();
    let mut failed: Vec<KillFailure> = Vec::new();

    for pid in pids {
        // Safety: never kill PID 0 or 1
        if pid <= 1 {
            failed.push(KillFailure {
                pid,
                reason: "Refusing to kill system process (PID ≤ 1)".to_string(),
            });
            continue;
        }

        let output = Command::new("kill")
            .args([signal, &pid.to_string()])
            .output();

        match output {
            Ok(out) => {
                if out.status.success() {
                    killed.push(pid);
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    failed.push(KillFailure {
                        pid,
                        reason: if stderr.contains("Operation not permitted") {
                            "Permission denied — try running as admin".to_string()
                        } else if stderr.contains("No such process") {
                            "Process already terminated".to_string()
                        } else {
                            stderr
                        },
                    });
                }
            }
            Err(e) => {
                failed.push(KillFailure {
                    pid,
                    reason: format!("Failed to execute kill: {}", e),
                });
            }
        }
    }

    KillResult {
        success: failed.is_empty(),
        killed,
        failed,
    }
}

// ─── Main ───────────────────────────────────────────────────────────────────

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![scan_ports, kill_ports])
        .run(tauri::generate_context!())
        .expect("error while running Port Manager");
}
