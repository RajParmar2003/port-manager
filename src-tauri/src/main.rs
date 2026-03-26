// src-tauri/src/main.rs
// Tauri backend: scans open ports, enriches with CPU/memory/orphan/docker data,
// tracks port history, and provides free-port + kill commands.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;

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
    // v2 fields
    pub cpu: f32,
    pub memory_mb: f32,
    pub is_orphan: bool,
    pub docker_container: Option<String>,
    pub launch_agent: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub timestamp: u64,
    pub event_type: String, // "opened" or "closed"
    pub port: u16,
    pub pid: u32,
    pub process: String,
}

#[derive(Debug, Serialize)]
pub struct FreePortResult {
    pub success: bool,
    pub port: u16,
    pub killed_pid: Option<u32>,
    pub killed_process: Option<String>,
    pub error: Option<String>,
}

// ─── History state ──────────────────────────────────────────────────────────

pub struct AppState {
    history: Mutex<Vec<HistoryEvent>>,
    last_ports: Mutex<HashMap<(u32, u16), String>>, // (pid, port) -> process name
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ─── Port categorization ────────────────────────────────────────────────────

fn categorize_port(process: &str, port: u16, user: &str) -> String {
    let proc_lower = process.to_lowercase();

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

    let container_procs = ["docker", "containerd", "kubelet", "kubectl", "podman"];
    if container_procs.iter().any(|p| proc_lower.contains(p))
        || [2375, 2376, 2377, 10250, 10255].contains(&port)
    {
        return "Containers".to_string();
    }

    let web_procs = ["nginx", "httpd", "apache", "caddy", "traefik", "haproxy", "envoy"];
    if web_procs.iter().any(|p| proc_lower.contains(p)) || [80, 443, 8443].contains(&port) {
        return "Web / Proxy".to_string();
    }

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

    let app_procs = [
        "spotify", "slack", "discord", "zoom", "teams", "telegram",
        "signal", "whatsapp", "dropbox", "1password", "chrome",
        "firefox", "safari", "brave", "arc", "figma", "notion",
        "obsidian", "vscode", "code",
    ];
    if app_procs.iter().any(|p| proc_lower.contains(p)) || port > 49152 {
        return "Apps".to_string();
    }

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

// ─── Parse lsof -F output ───────────────────────────────────────────────────

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
    let mut cur_pid: u32 = 0;
    let mut cur_process = String::new();
    let mut cur_user = String::new();
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
                cur_protocol.clear();
                cur_port = 0;
                cur_state.clear();
            }
            'P' => cur_protocol = value.to_uppercase(),
            'n' => {
                if let Some(port_str) = value.rsplit(':').next() {
                    if let Ok(p) = port_str.parse::<u16>() {
                        cur_port = p;
                    }
                }
            }
            'T' => {
                if let Some(st) = value.strip_prefix("ST=") {
                    cur_state = st.to_string();
                }
            }
            _ => {}
        }

        if (field_type == 'T' || field_type == 'n') && cur_port > 0 && cur_pid > 0 {
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
                cur_port = 0;
                cur_state.clear();
            }
        }
    }
    records
}

// ─── Process enrichment ─────────────────────────────────────────────────────

struct ProcessInfo {
    command: String,
    cpu: f32,
    memory_mb: f32,
    ppid: u32,
}

fn get_process_info(pid: u32) -> ProcessInfo {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command=,pcpu=,rss=,ppid="])
        .output();

    match output {
        Ok(out) => {
            let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if raw.is_empty() {
                return ProcessInfo {
                    command: format!("PID {}", pid),
                    cpu: 0.0,
                    memory_mb: 0.0,
                    ppid: 0,
                };
            }
            // ps output: "command... CPU  RSS  PPID"
            // Parse from the right since command can contain spaces
            let parts: Vec<&str> = raw.split_whitespace().collect();
            let len = parts.len();
            if len >= 4 {
                let ppid: u32 = parts[len - 1].parse().unwrap_or(0);
                let rss_kb: f32 = parts[len - 2].parse().unwrap_or(0.0);
                let cpu: f32 = parts[len - 3].parse().unwrap_or(0.0);
                let cmd = parts[..len - 3].join(" ");
                let cmd_truncated = if cmd.len() > 200 {
                    format!("{}...", &cmd[..200])
                } else {
                    cmd
                };
                ProcessInfo {
                    command: cmd_truncated,
                    cpu,
                    memory_mb: rss_kb / 1024.0,
                    ppid,
                }
            } else {
                ProcessInfo {
                    command: raw,
                    cpu: 0.0,
                    memory_mb: 0.0,
                    ppid: 0,
                }
            }
        }
        Err(_) => ProcessInfo {
            command: format!("PID {}", pid),
            cpu: 0.0,
            memory_mb: 0.0,
            ppid: 0,
        },
    }
}

fn is_orphan_process(ppid: u32) -> bool {
    // A process is orphaned if its parent is launchd (PID 1) or init
    // This means the original parent (e.g., Terminal, iTerm) has exited
    ppid == 1
}

// ─── Docker container lookup ────────────────────────────────────────────────

fn build_docker_port_map() -> HashMap<u16, String> {
    let mut map: HashMap<u16, String> = HashMap::new();

    let output = Command::new("docker")
        .args(["ps", "--format", "{{.Names}}\t{{.Image}}\t{{.Ports}}"])
        .output();

    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let name = parts[0];
                let image = parts[1];
                let ports_str = parts[2];
                // Parse port mappings like "0.0.0.0:5432->5432/tcp, 0.0.0.0:6379->6379/tcp"
                for mapping in ports_str.split(',') {
                    let mapping = mapping.trim();
                    // Look for "host_port->" pattern
                    if let Some(arrow_pos) = mapping.find("->") {
                        let host_part = &mapping[..arrow_pos];
                        if let Some(port_str) = host_part.rsplit(':').next() {
                            if let Ok(port) = port_str.parse::<u16>() {
                                map.insert(port, format!("{} ({})", name, image));
                            }
                        }
                    }
                }
            }
        }
    }
    map
}

// ─── Launch agent detection ─────────────────────────────────────────────────

fn build_launch_agent_map() -> HashMap<String, String> {
    // Maps process name (lowercase) -> launch agent plist name
    // Only flag well-known services that users might not realize are running
    let mut map: HashMap<String, String> = HashMap::new();

    // Check user launch agents
    let home = std::env::var("HOME").unwrap_or_default();
    let agent_dirs = [
        format!("{}/Library/LaunchAgents", home),
        "/Library/LaunchAgents".to_string(),
    ];

    for dir in &agent_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let filename = entry.file_name().to_string_lossy().to_string();
                if !filename.ends_with(".plist") {
                    continue;
                }
                // Extract the likely process name from the plist filename
                // e.g., "homebrew.mxcl.postgresql@14.plist" -> "postgres"
                // e.g., "homebrew.mxcl.redis.plist" -> "redis"
                let name_lower = filename.to_lowercase();
                let known_services = [
                    ("postgres", "postgres"),
                    ("mysql", "mysql"),
                    ("redis", "redis"),
                    ("mongo", "mongod"),
                    ("elasticsearch", "elasticsearch"),
                    ("memcache", "memcached"),
                    ("nginx", "nginx"),
                    ("httpd", "httpd"),
                    ("apache", "httpd"),
                ];
                for (pattern, proc_name) in &known_services {
                    if name_lower.contains(pattern) {
                        map.insert(proc_name.to_string(), filename.clone());
                    }
                }
            }
        }
    }
    map
}

// ─── Core scan logic ────────────────────────────────────────────────────────

fn do_scan_ports() -> Vec<PortEntry> {
    let output = Command::new("lsof")
        .args(["-i", "-P", "-n", "-F", "pcLfPnT"])
        .output();

    let records = match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            parse_lsof_field_output(&stdout)
        }
        Err(_) => return vec![],
    };

    let docker_map = build_docker_port_map();
    let launch_map = build_launch_agent_map();

    let mut entries: Vec<PortEntry> = Vec::new();
    let mut seen: HashMap<(u32, u16), bool> = HashMap::new();

    for r in records {
        if r.port == 0 || seen.contains_key(&(r.pid, r.port)) {
            continue;
        }
        seen.insert((r.pid, r.port), true);

        let info = get_process_info(r.pid);
        let category = categorize_port(&r.process, r.port, &r.user);
        let docker_container = docker_map.get(&r.port).cloned();
        let launch_agent = launch_map.get(&r.process.to_lowercase()).cloned();

        entries.push(PortEntry {
            pid: r.pid,
            process: r.process,
            port: r.port,
            protocol: r.protocol,
            state: r.state,
            user: r.user,
            command: info.command,
            category,
            cpu: info.cpu,
            memory_mb: (info.memory_mb * 10.0).round() / 10.0,
            is_orphan: is_orphan_process(info.ppid),
            docker_container,
            launch_agent,
        });
    }

    entries.sort_by(|a, b| a.category.cmp(&b.category).then(a.port.cmp(&b.port)));
    entries
}

// ─── History tracking ───────────────────────────────────────────────────────

fn update_history(
    new_ports: &[PortEntry],
    last_ports: &mut HashMap<(u32, u16), String>,
    history: &mut Vec<HistoryEvent>,
) {
    let ts = now_unix();
    let mut current: HashMap<(u32, u16), String> = HashMap::new();

    for p in new_ports {
        current.insert((p.pid, p.port), p.process.clone());
    }

    // Detect newly opened ports
    for ((pid, port), process) in &current {
        if !last_ports.contains_key(&(*pid, *port)) {
            history.push(HistoryEvent {
                timestamp: ts,
                event_type: "opened".to_string(),
                port: *port,
                pid: *pid,
                process: process.clone(),
            });
        }
    }

    // Detect closed ports
    for ((pid, port), process) in last_ports.iter() {
        if !current.contains_key(&(*pid, *port)) {
            history.push(HistoryEvent {
                timestamp: ts,
                event_type: "closed".to_string(),
                port: *port,
                pid: *pid,
                process: process.clone(),
            });
        }
    }

    // Keep history bounded (last 500 events)
    if history.len() > 500 {
        let drain_count = history.len() - 500;
        history.drain(..drain_count);
    }

    // Update last_ports
    *last_ports = current;
}

// ─── Tauri commands ─────────────────────────────────────────────────────────

#[tauri::command]
fn scan_ports(state: State<AppState>) -> ScanResult {
    let ports = do_scan_ports();

    // Update history
    if let (Ok(mut last), Ok(mut hist)) = (state.last_ports.lock(), state.history.lock()) {
        update_history(&ports, &mut last, &mut hist);
    }

    ScanResult {
        ports,
        error: None,
    }
}

#[tauri::command]
fn kill_ports(pids: Vec<u32>, force: bool) -> KillResult {
    let signal = if force { "-9" } else { "-15" };
    let mut killed: Vec<u32> = Vec::new();
    let mut failed: Vec<KillFailure> = Vec::new();

    for pid in pids {
        if pid <= 1 {
            failed.push(KillFailure {
                pid,
                reason: "Refusing to kill system process (PID <= 1)".to_string(),
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
                            "Permission denied".to_string()
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

#[tauri::command]
fn free_port(port: u16, force: bool) -> FreePortResult {
    // Find what's on this port and kill it
    let ports = do_scan_ports();
    let target = ports.iter().find(|p| p.port == port);

    match target {
        Some(entry) => {
            if entry.pid <= 1 {
                return FreePortResult {
                    success: false,
                    port,
                    killed_pid: None,
                    killed_process: None,
                    error: Some("Refusing to kill system process".to_string()),
                };
            }

            let signal = if force { "-9" } else { "-15" };
            let output = Command::new("kill")
                .args([signal, &entry.pid.to_string()])
                .output();

            match output {
                Ok(out) if out.status.success() => FreePortResult {
                    success: true,
                    port,
                    killed_pid: Some(entry.pid),
                    killed_process: Some(entry.process.clone()),
                    error: None,
                },
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    FreePortResult {
                        success: false,
                        port,
                        killed_pid: Some(entry.pid),
                        killed_process: Some(entry.process.clone()),
                        error: Some(stderr),
                    }
                }
                Err(e) => FreePortResult {
                    success: false,
                    port,
                    killed_pid: Some(entry.pid),
                    killed_process: Some(entry.process.clone()),
                    error: Some(format!("Failed to execute kill: {}", e)),
                },
            }
        }
        None => FreePortResult {
            success: true,
            port,
            killed_pid: None,
            killed_process: None,
            error: None,
        },
    }
}

#[tauri::command]
fn get_history(state: State<AppState>) -> Vec<HistoryEvent> {
    state.history.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

// ─── Main ───────────────────────────────────────────────────────────────────

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            history: Mutex::new(Vec::new()),
            last_ports: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            scan_ports, kill_ports, free_port, get_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running Port Manager");
}
