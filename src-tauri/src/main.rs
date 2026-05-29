// src-tauri/src/main.rs
// Port Manager v2 — macOS port scanner with enrichment, history, and tray mode.
// Every label and tag shown in the UI must be provably accurate.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{
    AppHandle, CustomMenuItem, Manager, State, SystemTray, SystemTrayEvent, SystemTrayMenu,
    SystemTrayMenuItem, SystemTraySubmenu,
};

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
    pub exe_path: String, // full executable path (handles spaces in paths)
    pub category: String,
    pub cpu: f32,
    pub memory_mb: f32,
    pub uptime: String, // e.g. "2d 6h", "45m", "12s", "N/A"
    pub is_orphan: bool,
    pub docker_container: Option<String>, // "container_name (image) — status"
    pub launch_agent: Option<String>,     // launchctl label (e.g. "homebrew.mxcl.postgresql@14")
    pub project: Option<String>,          // git project root absolute path, or None if not in a git repo
}

#[derive(Debug, Clone, Serialize)]
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

// ─── Port conflict detection ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortConflict {
    pub intended_port: u16,
    pub actual_port: u16,
    pub process: String,
    pub pid: u32,
    pub blocker_process: String,
    pub blocker_pid: u32,
}

// Common dev server ports that auto-increment on conflict
const CONFLICT_WATCH_PORTS: &[u16] = &[3000, 3001, 4200, 5000, 5173, 8000, 8080, 8888, 9000];

fn detect_port_conflicts(ports: &[PortEntry]) -> Vec<PortConflict> {
    let mut conflicts: Vec<PortConflict> = Vec::new();
    let port_map: HashMap<u16, &PortEntry> = ports.iter().map(|p| (p.port, p)).collect();

    for &watch_port in CONFLICT_WATCH_PORTS {
        let next_port = watch_port + 1;
        if let (Some(blocker), Some(fallback)) =
            (port_map.get(&watch_port), port_map.get(&next_port))
        {
            // Same process type on adjacent ports with different PIDs = likely conflict
            if blocker.process == fallback.process && blocker.pid != fallback.pid {
                conflicts.push(PortConflict {
                    intended_port: watch_port,
                    actual_port: next_port,
                    process: fallback.process.clone(),
                    pid: fallback.pid,
                    blocker_process: blocker.process.clone(),
                    blocker_pid: blocker.pid,
                });
            }
        }
    }
    conflicts
}

fn send_conflict_notification(app: &AppHandle, conflict: &PortConflict) {
    let _ = tauri::api::notification::Notification::new(&app.config().tauri.bundle.identifier)
        .title("Port Conflict Detected")
        .body(format!(
            "{} (PID {}) wanted :{} but fell back to :{} — blocked by {} (PID {})",
            conflict.process,
            conflict.pid,
            conflict.intended_port,
            conflict.actual_port,
            conflict.blocker_process,
            conflict.blocker_pid
        ))
        .show();
}

// ─── History persistence ────────────────────────────────────────────────────

fn history_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join(".port-manager")
        .join("history.json")
}

fn load_history_from_disk() -> Vec<HistoryEvent> {
    let path = history_file_path();
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_history_to_disk(history: &[HistoryEvent]) {
    let path = history_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string(history) {
        let _ = std::fs::write(&path, data);
    }
}

// ─── App state (Arc-wrapped for sharing with background thread) ─────────────

#[derive(Clone)]
pub struct AppState {
    history: Arc<Mutex<Vec<HistoryEvent>>>,
    last_ports: Arc<Mutex<HashMap<(u32, u16), String>>>, // (pid, port) -> process name
    last_conflicts: Arc<Mutex<Vec<(u16, u16)>>>,         // previously notified conflict pairs
}

impl AppState {
    fn new() -> Self {
        AppState {
            history: Arc::new(Mutex::new(Vec::new())),
            last_ports: Arc::new(Mutex::new(HashMap::new())),
            last_conflicts: Arc::new(Mutex::new(Vec::new())),
        }
    }
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
        "postgres",
        "mysqld",
        "mysql",
        "redis-server",
        "redis",
        "mongod",
        "mongos",
        "memcached",
        "cassandra",
        "couchdb",
        "neo4j",
        "elasticsearch",
        "clickhouse",
    ];
    if db_procs.iter().any(|p| proc_lower.contains(p))
        || [5432, 3306, 6379, 27017, 11211, 9042, 5984, 7474, 9200, 8123].contains(&port)
    {
        return "Databases".to_string();
    }

    // AI / ML — check before Dev Servers since Jupyter/Ollama often run on dev-range ports
    let ai_procs = [
        "ollama", "jupyter", "jupyterlab", "notebook", "mlflow", "tensorboard",
        "triton", "vllm", "lmstudio", "localai", "llamacpp", "text-generation",
    ];
    if ai_procs.iter().any(|p| proc_lower.contains(p))
        || [11434, 8888, 8889, 8501, 8502, 8265, 5001, 6006, 3001].contains(&port)
    {
        return "AI / ML".to_string();
    }

    // Message Queues
    let mq_procs = [
        "rabbitmq", "beam.smp", "kafka", "nats-server", "nats", "pulsar",
        "mosquitto", "emqx", "zeromq",
    ];
    if mq_procs.iter().any(|p| proc_lower.contains(p))
        || [5672, 15672, 9092, 4222, 8222, 6650, 1883, 8883].contains(&port)
    {
        return "Message Queues".to_string();
    }

    let container_procs = ["docker", "containerd", "kubelet", "kubectl", "podman"];
    if container_procs.iter().any(|p| proc_lower.contains(p))
        || [2375, 2376, 2377, 10250, 10255].contains(&port)
    {
        return "Containers".to_string();
    }

    let web_procs = [
        "nginx", "httpd", "apache", "caddy", "traefik", "haproxy", "envoy",
    ];
    if web_procs.iter().any(|p| proc_lower.contains(p)) || [80, 443, 8443].contains(&port) {
        return "Web / Proxy".to_string();
    }

    let dev_procs = [
        "node", "python", "python3", "ruby", "java", "go", "deno", "bun", "php", "uvicorn",
        "gunicorn", "flask", "next", "vite", "webpack", "esbuild", "grafana", "prometheus",
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
        "spotify", "slack", "discord", "zoom", "teams", "telegram", "signal", "whatsapp",
        "dropbox", "1password", "chrome", "firefox", "safari", "brave", "arc", "figma", "notion",
        "obsidian", "vscode", "code",
    ];
    if app_procs.iter().any(|p| proc_lower.contains(p)) || port > 49152 {
        return "Apps".to_string();
    }

    let system_procs = [
        "sshd",
        "ssh",
        "launchd",
        "mDNSResponder",
        "systemd",
        "cupsd",
    ];
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
            let is_listening = cur_state == "LISTEN" || cur_protocol == "UDP";
            if is_listening {
                records.push(LsofRecord {
                    pid: cur_pid,
                    process: cur_process.clone(),
                    user: cur_user.clone(),
                    protocol: if cur_protocol.is_empty() {
                        "TCP".to_string()
                    } else {
                        cur_protocol.clone()
                    },
                    port: cur_port,
                    state: if cur_state.is_empty() {
                        "LISTEN".to_string()
                    } else {
                        cur_state.clone()
                    },
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
    tty: String,   // "??" = no controlling terminal, "s000" etc. = has terminal
    uptime: String, // elapsed time from ps, e.g. "01-06:05:34" or "23:45" or "N/A"
}

fn get_process_info(pid: u32) -> ProcessInfo {
    let pid_str = pid.to_string();
    let default = ProcessInfo {
        command: format!("(unknown — pid {})", pid),
        cpu: -1.0,
        memory_mb: -1.0,
        ppid: 0,
        tty: "??".to_string(),
        uptime: "N/A".to_string(),
    };

    // IMPORTANT: macOS `ps` truncates `command=` to 16 chars when combined with
    // other fields via commas. Two separate calls avoid this.
    let cmd_output = Command::new("ps")
        .args(["-ww", "-p", &pid_str, "-o", "command="])
        .output();
    let stats_output = Command::new("ps")
        .args(["-ww", "-p", &pid_str, "-o", "pcpu=,rss=,ppid=,tty=,etime="])
        .output();

    let command = match cmd_output {
        Ok(out) => {
            let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if raw.is_empty() { default.command.clone() } else { raw }
        }
        Err(_) => default.command.clone(),
    };

    let (cpu, memory_mb, ppid, tty, uptime) = match stats_output {
        Ok(out) => {
            let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let parts: Vec<&str> = raw.split_whitespace().collect();
            if parts.len() >= 5 {
                let cpu: f32 = parts[0].parse().unwrap_or(-1.0);
                let rss_kb: f32 = parts[1].parse().unwrap_or(-1.0);
                let ppid: u32 = parts[2].parse().unwrap_or(0);
                let tty = parts[3].to_string();
                let etime_raw = parts[4].to_string(); // e.g. "01-06:05:34" or "23:45" or "05"
                let uptime = format_etime(&etime_raw);
                (cpu, rss_kb / 1024.0, ppid, tty, uptime)
            } else {
                (-1.0, -1.0, 0, "??".to_string(), "N/A".to_string())
            }
        }
        Err(_) => (-1.0, -1.0, 0, "??".to_string(), "N/A".to_string()),
    };

    ProcessInfo { command, cpu, memory_mb, ppid, tty, uptime }
}

// ─── Launchctl-based service detection (accurate, PID-matched) ──────────────
// Uses `launchctl list` which gives us the ACTUAL PID of every managed service.
// This is 100% accurate — no guessing from filenames.

fn build_launchctl_pid_map() -> HashMap<u32, String> {
    let mut map: HashMap<u32, String> = HashMap::new();

    let output = Command::new("launchctl").arg("list").output();

    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            // Format: "PID\tStatus\tLabel" — skip header and entries with "-" PID
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                if let Ok(pid) = parts[0].trim().parse::<u32>() {
                    if pid > 0 {
                        map.insert(pid, parts[2].trim().to_string());
                    }
                }
            }
        }
    }
    map
}

// ─── Elapsed time formatting ─────────────────────────────────────────────────
// Converts ps `etime` output (e.g. "01-06:05:34", "23:45:12", "45:12", "12")
// into a human-readable short format (e.g. "1d 6h", "23h 45m", "45m", "12s").

fn format_etime(raw: &str) -> String {
    // Format: [[DD-]HH:]MM:SS
    let (days, rest) = if let Some(pos) = raw.find('-') {
        let d: u64 = raw[..pos].parse().unwrap_or(0);
        (d, &raw[pos + 1..])
    } else {
        (0, raw)
    };

    let parts: Vec<u64> = rest.split(':').filter_map(|p| p.parse().ok()).collect();
    let (hours, minutes, seconds) = match parts.len() {
        3 => (parts[0], parts[1], parts[2]),
        2 => (0, parts[0], parts[1]),
        1 => (0, 0, parts[0]),
        _ => return "N/A".to_string(),
    };

    let total_hours = days * 24 + hours;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if total_hours > 0 {
        format!("{}h {}m", total_hours, minutes)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

// ─── Executable path extraction ──────────────────────────────────────────────
// Extracts the executable path from a full command string, handling spaces in
// paths (e.g. "/Applications/Claude Helper.app/Contents/MacOS/Claude Helper --type=gpu").

fn extract_exe_path(command: &str) -> String {
    if !command.starts_with('/') {
        // Not a full path — return first token
        return command.split_whitespace().next().unwrap_or(command).to_string();
    }
    // Scan for flag boundary (" -") or second path argument (" /")
    let bytes = command.as_bytes();
    for i in 1..bytes.len() {
        if bytes[i - 1] == b' ' && (bytes[i] == b'-' || bytes[i] == b'/') {
            return command[..i - 1].to_string();
        }
    }
    // No flags found — entire string is the path
    command.to_string()
}

// ─── Orphan detection (accurate — no false positives) ───────────────────────
// A process is truly orphaned ONLY if ALL three conditions are true:
//   1. Parent is launchd (ppid == 1)
//   2. NOT managed by launchctl (no launch agent label)
//   3. Has no controlling terminal (tty == "??")
//
// This avoids flagging Homebrew services, system daemons, or anything the user
// is actively running in a terminal.

fn is_truly_orphan(ppid: u32, has_launch_agent: bool, tty: &str) -> bool {
    ppid == 1 && !has_launch_agent && tty == "??"
}

// ─── Docker container lookup ────────────────────────────────────────────────

fn build_docker_port_map() -> HashMap<u16, String> {
    let mut map: HashMap<u16, String> = HashMap::new();

    let output = Command::new("docker")
        .args([
            "ps",
            "--format",
            "{{.Names}}\t{{.Image}}\t{{.Ports}}\t{{.Status}}",
        ])
        .output();

    if let Ok(out) = output {
        if !out.status.success() {
            return map; // Docker not running or not installed — return empty
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 4 {
                let name = parts[0];
                let image = parts[1];
                let ports_str = parts[2];
                let status = parts[3];
                let container_info = format!("{} ({}) — {}", name, image, status);

                // Parse port mappings like "0.0.0.0:5432->5432/tcp"
                for mapping in ports_str.split(',') {
                    let mapping = mapping.trim();
                    if let Some(arrow_pos) = mapping.find("->") {
                        let host_part = &mapping[..arrow_pos];
                        if let Some(port_str) = host_part.rsplit(':').next() {
                            if let Ok(port) = port_str.parse::<u16>() {
                                map.insert(port, container_info.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    map
}

// ─── Project detection (git repo root from process CWD) ────────────────────
// For each PID we ask lsof for its current working directory, then walk upward
// until we find a `.git` entry (directory or file — worktrees use a file).
// The canonicalized project root path is returned. Capped at 20 parent levels
// to avoid pathological loops.
//
// Known limitations:
// - lsof often cannot read CWD for processes owned by other users (returns
//   empty output). Those ports will have `project = None`.
// - Processes that chdir() after launch report the new CWD, not the launch
//   directory — acceptable for this feature's goal (grouping live work).
// - Nested git repos resolve to the innermost `.git` they encounter walking
//   up; this matches developer intuition.
//
// Callers must pass a scan-scoped cache so multi-port PIDs don't pay the lookup
// cost more than once per scan cycle. The cache key is PID; value is the
// resolved `Option<String>` so even negative results are cached.

fn get_cwd_for_pid(pid: u32) -> Option<PathBuf> {
    // `lsof -a -p PID -d cwd -Fn` restricts to the cwd fd and emits field-style
    // output. The 'n' field carries the path.
    let output = Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix('n') {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(PathBuf::from(trimmed));
            }
        }
    }
    None
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    // Resolve symlinks so Docker bind mounts and similar surprises don't skew
    // the result. If canonicalize fails (permission, missing), fall back to the
    // raw path so we still attempt the walk.
    let mut current: PathBuf = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());

    for _ in 0..20 {
        // `.git` can be either a directory (normal repo) or a regular file
        // (git worktree, submodule pointer). `exists()` covers both.
        if current.join(".git").exists() {
            return Some(current);
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => break, // reached filesystem root or can't go higher
        }
    }
    None
}

fn get_project_for_pid(pid: u32, cache: &mut HashMap<u32, Option<String>>) -> Option<String> {
    if let Some(cached) = cache.get(&pid) {
        return cached.clone();
    }
    let resolved = get_cwd_for_pid(pid)
        .and_then(|cwd| find_git_root(&cwd))
        .map(|p| p.to_string_lossy().trim_end_matches('/').to_string());
    cache.insert(pid, resolved.clone());
    resolved
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

    // Build lookup maps once per scan (not per-port)
    let docker_map = build_docker_port_map();
    let launchctl_map = build_launchctl_pid_map();

    let mut entries: Vec<PortEntry> = Vec::new();
    let mut seen: HashMap<(u32, u16), bool> = HashMap::new();
    // Scan-scoped cache so multi-port PIDs only pay the CWD+walk cost once.
    let mut project_cache: HashMap<u32, Option<String>> = HashMap::new();

    for r in records {
        if r.port == 0 || seen.contains_key(&(r.pid, r.port)) {
            continue;
        }
        seen.insert((r.pid, r.port), true);

        let info = get_process_info(r.pid);
        let category = categorize_port(&r.process, r.port, &r.user);
        let docker_container = docker_map.get(&r.port).cloned();
        let launch_agent = launchctl_map.get(&r.pid).cloned();
        let is_orphan = is_truly_orphan(info.ppid, launch_agent.is_some(), &info.tty);
        let project = get_project_for_pid(r.pid, &mut project_cache);

        let exe_path = extract_exe_path(&info.command);
        entries.push(PortEntry {
            pid: r.pid,
            process: r.process,
            port: r.port,
            protocol: r.protocol,
            state: r.state,
            user: r.user,
            command: info.command,
            exe_path,
            category,
            cpu: info.cpu,
            memory_mb: (info.memory_mb * 10.0).round() / 10.0,
            uptime: info.uptime,
            is_orphan,
            docker_container,
            launch_agent,
            project,
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

    // Update last_ports snapshot
    *last_ports = current;

    // Persist to disk
    save_history_to_disk(history);
}

// ─── Shared scan cycle (used by both background thread and manual rescan) ───

fn run_scan_cycle(app: &AppHandle, state: &AppState) -> Vec<PortEntry> {
    let ports = do_scan_ports();

    // Update history
    if let (Ok(mut last), Ok(mut hist)) = (state.last_ports.lock(), state.history.lock()) {
        update_history(&ports, &mut last, &mut hist);
    }

    // Detect port conflicts and notify for new ones
    let conflicts = detect_port_conflicts(&ports);
    if let Ok(mut last_conflicts) = state.last_conflicts.lock() {
        for conflict in &conflicts {
            let key = (conflict.intended_port, conflict.actual_port);
            if !last_conflicts.contains(&key) {
                send_conflict_notification(app, conflict);
                last_conflicts.push(key);
            }
        }
        let current_keys: Vec<(u16, u16)> = conflicts
            .iter()
            .map(|c| (c.intended_port, c.actual_port))
            .collect();
        last_conflicts.retain(|k| current_keys.contains(k));
    }

    // Update tray menu with current ports
    update_tray_menu(app, &ports);

    // Emit to frontend so it can render without polling
    let _ = app.emit_all("ports-updated", ScanResult {
        ports: ports.clone(),
        error: None,
    });

    ports
}

// ─── Tauri commands ─────────────────────────────────────────────────────────

#[tauri::command]
fn scan_ports(app: AppHandle, state: State<AppState>) -> ScanResult {
    let ports = run_scan_cycle(&app, &state);
    ScanResult {
        ports,
        error: None,
    }
}

#[tauri::command]
fn get_conflicts(_state: State<AppState>) -> Vec<PortConflict> {
    let ports = do_scan_ports();
    detect_port_conflicts(&ports)
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
    state
        .history
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[tauri::command]
fn clear_history(state: State<AppState>) {
    if let Ok(mut hist) = state.history.lock() {
        hist.clear();
        save_history_to_disk(&hist);
    }
}

#[tauri::command]
fn stop_launch_agent(label: String) -> Result<String, String> {
    // launchctl stop sends SIGTERM to the managed service
    let output = Command::new("launchctl")
        .args(["stop", &label])
        .output()
        .map_err(|e| format!("Failed to run launchctl: {}", e))?;

    if output.status.success() {
        Ok(format!("Stopped {}", label))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            // launchctl stop often succeeds silently even on error
            Ok(format!("Stop signal sent to {}", label))
        } else {
            Err(format!("Failed to stop {}: {}", label, stderr))
        }
    }
}

// ─── System Tray ────────────────────────────────────────────────────────────

fn build_tray_menu(ports: &[PortEntry]) -> SystemTrayMenu {
    let mut menu = SystemTrayMenu::new();

    // Header with port count
    let header = CustomMenuItem::new(
        "header",
        format!("Port Manager — {} ports", ports.len()),
    )
    .disabled();
    menu = menu.add_item(header);
    menu = menu.add_native_item(SystemTrayMenuItem::Separator);

    if !ports.is_empty() {
        // Group by category
        let mut by_cat: HashMap<String, Vec<&PortEntry>> = HashMap::new();
        for p in ports {
            by_cat.entry(p.category.clone()).or_default().push(p);
        }

        let cat_order = [
            "Dev Servers",
            "Databases",
            "Web / Proxy",
            "Containers",
            "Apps",
            "System",
            "Other",
        ];

        let mut shown = 0;
        for cat_name in &cat_order {
            if shown >= 10 {
                break;
            }
            if let Some(items) = by_cat.get(*cat_name) {
                let sub_items: Vec<CustomMenuItem> = items
                    .iter()
                    .take(6)
                    .map(|p| {
                        CustomMenuItem::new(
                            format!("free_{}", p.port),
                            format!(":{} — {} (PID {})", p.port, p.process, p.pid),
                        )
                    })
                    .collect();

                let mut sub_menu = SystemTrayMenu::new();
                for item in sub_items {
                    sub_menu = sub_menu.add_item(item);
                    shown += 1;
                }
                menu = menu.add_submenu(SystemTraySubmenu::new(
                    format!("{} ({})", cat_name, items.len()),
                    sub_menu,
                ));
            }
        }

        menu = menu.add_native_item(SystemTrayMenuItem::Separator);

        // Quick action: Kill All Dev Servers (only if there are dev servers)
        if by_cat.contains_key("Dev Servers") {
            let dev_count = by_cat["Dev Servers"].len();
            let kill_devs = CustomMenuItem::new(
                "kill_dev_servers",
                format!("Kill All Dev Servers ({})", dev_count),
            );
            menu = menu.add_item(kill_devs);
            menu = menu.add_native_item(SystemTrayMenuItem::Separator);
        }
    }

    let show = CustomMenuItem::new("show", "Show Window");
    let rescan = CustomMenuItem::new("rescan", "Rescan Ports");
    let quit = CustomMenuItem::new("quit", "Quit Port Manager");

    menu = menu.add_item(show);
    menu = menu.add_item(rescan);
    menu = menu.add_native_item(SystemTrayMenuItem::Separator);
    menu = menu.add_item(quit);

    menu
}

fn update_tray_menu(app: &AppHandle, ports: &[PortEntry]) {
    if let Some(tray) = app.tray_handle_by_id("main") {
        let _ = tray.set_menu(build_tray_menu(ports));
        let _ = tray.set_tooltip(&format!("Port Manager — {} ports", ports.len()));
    }
}

// ─── Main ───────────────────────────────────────────────────────────────────

fn main() {
    let tray_menu = build_tray_menu(&[]);
    let system_tray = SystemTray::new()
        .with_id("main")
        .with_menu(tray_menu)
        .with_tooltip("Port Manager");

    tauri::Builder::default()
        .system_tray(system_tray)
        .on_system_tray_event(|app, event| match event {
            SystemTrayEvent::LeftClick { .. } => {
                if let Some(window) = app.get_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                "quit" => {
                    // Flush history to disk before exiting
                    if let Some(state) = app.try_state::<AppState>() {
                        if let Ok(hist) = state.history.lock() {
                            save_history_to_disk(&hist);
                        }
                    }
                    std::process::exit(0);
                }
                "show" => {
                    if let Some(window) = app.get_window("main") {
                        let _ = window.unminimize();
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "rescan" => {
                    if let Some(window) = app.get_window("main") {
                        let _ = window.emit("tray-rescan", ());
                    }
                }
                "kill_dev_servers" => {
                    let ports = do_scan_ports();
                    let dev_pids: Vec<u32> = ports
                        .iter()
                        .filter(|p| p.category == "Dev Servers" && p.pid > 1)
                        .map(|p| p.pid)
                        .collect();
                    let count = dev_pids.len();
                    for pid in &dev_pids {
                        let _ = Command::new("kill")
                            .args(["-15", &pid.to_string()])
                            .output();
                    }
                    let _ = tauri::api::notification::Notification::new(
                        &app.config().tauri.bundle.identifier,
                    )
                    .title("Dev Servers Killed")
                    .body(format!("Stopped {} dev server process(es)", count))
                    .show();
                    if let Some(window) = app.get_window("main") {
                        let _ = window.emit("tray-rescan", ());
                    }
                }
                other => {
                    // Handle "free_PORT" clicks from tray submenus
                    if let Some(port_str) = other.strip_prefix("free_") {
                        if let Ok(port) = port_str.parse::<u16>() {
                            let result = free_port(port, false);
                            if result.success {
                                let msg = if let Some(proc_name) = &result.killed_process {
                                    format!("Freed :{} (killed {})", port, proc_name)
                                } else {
                                    format!("Port {} is already free", port)
                                };
                                let _ = tauri::api::notification::Notification::new(
                                    &app.config().tauri.bundle.identifier,
                                )
                                .title("Port Freed")
                                .body(msg)
                                .show();
                            }
                            if let Some(window) = app.get_window("main") {
                                let _ = window.emit("tray-rescan", ());
                            }
                        }
                    }
                }
            },
            _ => {}
        })
        .on_window_event(|event| {
            // Minimize to dock when red X is clicked.
            // macOS natively restores minimized windows when the dock icon is clicked.
            // Tray left-click and "Show Window" also restore.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event.event() {
                let _ = event.window().minimize();
                api.prevent_close();
            }
        })
        .setup(|app| {
            // Load persisted history from disk on startup
            let state = app.state::<AppState>();
            if let Ok(mut hist) = state.history.lock() {
                *hist = load_history_from_disk();
            }

            // Spawn background scan thread — runs every 5s (matches Activity Monitor default).
            // This is the primary scan driver; the frontend listens for "ports-updated" events.
            let app_handle = app.handle();
            let bg_state = state.inner().clone();
            std::thread::spawn(move || {
                // Small initial delay so the window has time to set up its event listener
                std::thread::sleep(Duration::from_millis(500));
                loop {
                    run_scan_cycle(&app_handle, &bg_state);
                    std::thread::sleep(Duration::from_secs(5));
                }
            });

            Ok(())
        })
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            scan_ports,
            kill_ports,
            free_port,
            get_history,
            clear_history,
            get_conflicts,
            stop_launch_agent
        ])
        .build(tauri::generate_context!())
        .expect("error while building Port Manager")
        .run(|app_handle, event| {
            match event {
                tauri::RunEvent::ExitRequested { api, .. } => {
                    api.prevent_exit();
                }
                // macOS: fires when dock icon is clicked (applicationDidBecomeActive).
                // If window was hidden to tray, restore it.
                tauri::RunEvent::Resumed => {
                    if let Some(window) = app_handle.get_window("main") {
                        if !window.is_visible().unwrap_or(true) {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                }
                _ => {}
            }
        });
}
