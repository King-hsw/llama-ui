use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

// ---------- 设置持久化 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Settings {
    /// 已废弃：v1 单一安装目录（两种场景混用），仅为兼容旧 settings.json 读写保留
    #[serde(default)]
    install_dir: String,
    #[serde(default)]
    models_dir: String,
    #[serde(default)]
    current_tag: Option<String>,
    #[serde(default)]
    mirror: String,
    /// 已保存的启动配置方案
    #[serde(default)]
    profiles: Vec<Profile>,
    /// 程序来源："custom"（用户自备目录，只读）| "managed"（托管下载，独占写入）
    #[serde(default)]
    exe_source: String,
    /// 场景一：用户自备 llama.cpp 的根目录（含 llama-server.exe），程序对其只读
    #[serde(default)]
    custom_dir: String,
    /// 场景二：托管下载根目录。所有下载/解压/清理仅发生在此目录内，
    /// 无默认值——强制用户首次设置时显式指定
    #[serde(default)]
    managed_dir: String,
    /// 自动检查 llama.cpp 更新：开启后应用启动时对比 GitHub 最新版与已安装版本，
    /// 发现有新版本时前端弹窗提示
    #[serde(default)]
    auto_check_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            install_dir: String::new(),
            models_dir: String::new(),
            current_tag: None,
            mirror: String::new(),
            profiles: Vec::new(),
            exe_source: "managed".into(),
            custom_dir: String::new(),
            managed_dir: String::new(),
            auto_check_update: false,
        }
    }
}

/// 启动配置方案：cfg 保存前端启动参数的原始 JSON（驼峰 key），原样存取
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Profile {
    name: String,
    cfg: Value,
}

fn data_dir(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn settings_path(app: &AppHandle) -> PathBuf {
    data_dir(app).join("settings.json")
}

fn load_settings(app: &AppHandle) -> Settings {
    fs::read_to_string(settings_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings_inner(app: &AppHandle, s: &Settings) {
    let _ = fs::create_dir_all(settings_path(app).parent().unwrap());
    let _ = fs::write(
        settings_path(app),
        serde_json::to_string_pretty(s).unwrap_or_default(),
    );
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Settings {
    let mut s = load_settings(&app);
    // 首次使用：模型目录默认指向应用数据目录。
    // 注意 managed_dir / custom_dir 不给默认值——按设计强制用户在设置页显式指定
    let base = data_dir(&app);
    if s.models_dir.is_empty() {
        s.models_dir = base.join("models").to_string_lossy().into_owned();
    }
    if s.exe_source.is_empty() {
        s.exe_source = "managed".into();
    }
    save_settings_inner(&app, &s);
    s
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) {
    save_settings_inner(&app, &settings);
}

// ---------- 启动配置方案 ----------

#[tauri::command]
fn list_profiles(app: AppHandle) -> Vec<Profile> {
    load_settings(&app).profiles
}

/// 按名称 upsert 一份启动配置
#[tauri::command]
fn save_profile(app: AppHandle, name: String, cfg: Value) -> Vec<Profile> {
    let mut s = load_settings(&app);
    let name = name.trim().to_string();
    if let Some(p) = s.profiles.iter_mut().find(|p| p.name == name) {
        p.cfg = cfg;
    } else {
        s.profiles.push(Profile { name, cfg });
    }
    save_settings_inner(&app, &s);
    s.profiles
}

#[tauri::command]
fn delete_profile(app: AppHandle, name: String) -> Vec<Profile> {
    let mut s = load_settings(&app);
    s.profiles.retain(|p| p.name != name);
    save_settings_inner(&app, &s);
    s.profiles
}

#[tauri::command]
fn get_data_dir(app: AppHandle) -> String {
    data_dir(&app).to_string_lossy().into_owned()
}

// ---------- 模型扫描 ----------

#[derive(Serialize)]
struct ModelInfo {
    name: String,
    path: String,
    size: u64,
}

fn walk_gguf(dir: &Path, depth: usize, out: &mut Vec<ModelInfo>) {
    if depth > 5 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_gguf(&p, depth + 1, out);
        } else if p.extension().map(|x| x == "gguf").unwrap_or(false) {
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(ModelInfo {
                name: p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path: p.to_string_lossy().into_owned(),
                size,
            });
        }
    }
}

#[tauri::command]
fn list_models(dir: String) -> Vec<ModelInfo> {
    let mut out = Vec::new();
    let d = PathBuf::from(&dir);
    if d.is_dir() {
        walk_gguf(&d, 0, &mut out);
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

// ---------- llama-server 定位 ----------

fn find_file(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    if depth > 4 {
        return None;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return None;
    };
    let entries: Vec<_> = rd.flatten().collect();
    for e in &entries {
        let p = e.path();
        if p.is_file() && p.file_name().map(|n| n == name).unwrap_or(false) {
            return Some(p);
        }
    }
    for e in &entries {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_file(&p, name, depth + 1) {
                return Some(f);
            }
        }
    }
    None
}

#[derive(Serialize)]
struct ResolvedExe {
    /// 解析到的 llama-server.exe 完整路径，未找到为 null
    path: Option<String>,
    /// "custom" / "managed"
    source: String,
    /// managed 模式下的版本 tag
    tag: Option<String>,
    /// 未找到时的引导信息，供 UI 直接展示
    message: String,
}

/// 按 exe_source 解析 llama-server.exe，两种场景严格隔离、零跨场景回退：
/// - custom：在用户自备根目录（只读）中查找 llama-server.exe
/// - managed：只认 <managed_dir>/<current_tag>/llama-server.exe，不做全目录递归搜索
///   （递归搜索会让目录里的其他 exe 抢匹配，正是 v1 混用 single install_dir 的污染源）
#[tauri::command]
fn resolve_server_exe(app: AppHandle) -> ResolvedExe {
    let s = load_settings(&app);
    if s.exe_source == "custom" {
        let dir = s.custom_dir.trim();
        if dir.is_empty() {
            return ResolvedExe {
                path: None,
                source: "custom".into(),
                tag: None,
                message: "尚未指定本地 llama.cpp 根目录，请在「设置 → 程序来源」中选择".into(),
            };
        }
        match find_file(Path::new(dir), "llama-server.exe", 0) {
            Some(p) => ResolvedExe {
                path: Some(p.to_string_lossy().into_owned()),
                source: "custom".into(),
                tag: None,
                message: String::new(),
            },
            None => ResolvedExe {
                path: None,
                source: "custom".into(),
                tag: None,
                message: format!("在 {dir} 中未找到 llama-server.exe，请确认目录正确"),
            },
        }
    } else {
        let dir = s.managed_dir.trim();
        if dir.is_empty() {
            return ResolvedExe {
                path: None,
                source: "managed".into(),
                tag: s.current_tag.clone(),
                message: "尚未指定托管下载目录，请在「设置 → 程序来源」中选择后重新下载".into(),
            };
        }
        let Some(tag) = s.current_tag.clone().filter(|t| !t.is_empty()) else {
            return ResolvedExe {
                path: None,
                source: "managed".into(),
                tag: None,
                message: "尚未安装任何版本，请到「下载与更新」下载 llama.cpp".into(),
            };
        };
        let candidate = Path::new(dir).join(&tag).join("llama-server.exe");
        if candidate.is_file() {
            ResolvedExe {
                path: Some(candidate.to_string_lossy().into_owned()),
                source: "managed".into(),
                tag: Some(tag),
                message: String::new(),
            }
        } else {
            ResolvedExe {
                path: None,
                source: "managed".into(),
                tag: Some(tag.clone()),
                message: format!("托管目录中未找到版本 {tag} 的 llama-server.exe，可能已被移动或删除，请重新下载"),
            }
        }
    }
}

// ---------- 本机配置信息 ----------

#[derive(Serialize, Clone)]
struct GpuInfo {
    name: String,
    vram_mb: Option<u64>,
    /// "nvidia-smi"（精确）或 "wmi"（≥4GB 时读不准）
    source: String,
}

#[derive(Serialize, Clone, Default)]
struct SystemInfo {
    os: String,
    cpu: String,
    /// 归一化后的 CPU 架构："x64" / "arm64" / "x86" / "unknown"
    arch: String,
    physical_cores: Option<u32>,
    logical_cores: Option<u32>,
    ram_gb: f64,
    gpus: Vec<GpuInfo>,
}

/// 查询结果进程内缓存，避免每次打开页面都 spawn PowerShell
static SYS_INFO: OnceLock<SystemInfo> = OnceLock::new();

const PS_SYSINFO_SCRIPT: &str = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$ErrorActionPreference = 'SilentlyContinue'
$os = Get-CimInstance Win32_OperatingSystem | Select-Object -First 1 Caption, Version, TotalVisibleMemorySize
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1 Name, NumberOfCores, NumberOfLogicalProcessors
$gpus = @(Get-CimInstance Win32_VideoController | Select-Object Name, AdapterRAM)
$nv = @(& nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits 2>$null)
$parch = $env:PROCESSOR_ARCHITEW6432
if (-not $parch) { $parch = $env:PROCESSOR_ARCHITECTURE }
$reg = @()
$base = 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}'
Get-ChildItem $base -ErrorAction SilentlyContinue | ForEach-Object {
  $p = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
  if ($p -and $p.DriverDesc) {
    $bytes = $null
    if ($p.PSObject.Properties['HardwareInformation.qwMemorySize'] -and $p.'HardwareInformation.qwMemorySize') {
      $bytes = [uint64]$p.'HardwareInformation.qwMemorySize'
    } elseif ($p.PSObject.Properties['HardwareInformation.MemorySize']) {
      $v = $p.'HardwareInformation.MemorySize'
      if ($v -is [byte[]]) {
        if ($v.Length -ge 8) { $bytes = [uint64][BitConverter]::ToInt64($v, 0) }
        elseif ($v.Length -ge 4) { $bytes = [uint64][BitConverter]::ToInt32($v, 0) }
      } else { $bytes = [uint64]($v | Select-Object -First 1) }
    }
    $reg += [pscustomobject]@{ name = [string]$p.DriverDesc; bytes = $bytes }
  }
}
[pscustomobject]@{
  os = $os.Caption
  osver = $os.Version
  ram = [uint64]$os.TotalVisibleMemorySize
  cpu = $cpu.Name
  cores = $cpu.NumberOfCores
  logical = $cpu.NumberOfLogicalProcessors
  parch = $parch
  gpus = $gpus
  nv = $nv
  reg = $reg
} | ConvertTo-Json -Depth 4
"#;

/// 归一化架构标识：Windows env 为 AMD64/ARM64/X86，Rust consts 为 x86_64/aarch64
fn norm_arch(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "amd64" | "x86_64" | "x64" => "x64".into(),
        "arm64" | "aarch64" => "arm64".into(),
        "x86" | "i386" | "i686" => "x86".into(),
        _ => "unknown".into(),
    }
}

fn parse_system_info(json: &str) -> Option<SystemInfo> {
    let v: Value = serde_json::from_str(json).ok()?;
    // PS ConvertTo-Json 对单元素数组可能退化为对象，统一转成数组
    let arr = |x: Option<&Value>| -> Vec<Value> {
        match x {
            Some(Value::Array(a)) => a.clone(),
            Some(Value::Null) | None => Vec::new(),
            Some(o) if o.is_object() => vec![o.clone()],
            _ => Vec::new(),
        }
    };

    let os = v.get("os").and_then(|x| x.as_str()).unwrap_or("Windows").trim().to_string();
    let osver = v.get("osver").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    let mut cpu = v.get("cpu").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    if cpu.is_empty() {
        cpu = "未知".into();
    }
    let cores = v.get("cores").and_then(|x| x.as_u64()).filter(|x| *x > 0);
    let logical = v.get("logical").and_then(|x| x.as_u64()).filter(|x| *x > 0);

    // nvidia-smi 输出逐行："NVIDIA GeForce RTX 4090, 24564"（MB）
    let mut nv: Vec<(String, u64)> = Vec::new();
    for line in arr(v.get("nv")) {
        let Some(s) = line.as_str() else { continue };
        let Some((name, mb)) = s.rsplit_once(',') else { continue };
        if let Ok(mb) = mb.trim().parse::<u64>() {
            nv.push((name.trim().to_string(), mb));
        }
    }
    let find_nv = |name: &str| -> Option<u64> {
        let lname = name.to_lowercase();
        nv.iter()
            .find(|(n, _)| {
                let ln = n.to_lowercase();
                ln == lname || ln.contains(&lname) || lname.contains(&ln)
            })
            .map(|(_, mb)| *mb)
    };

    // 注册表显存（HardwareInformation.qwMemorySize，字节），比 WMI 的 uint32 准确
    let mut reg: Vec<(String, u64)> = Vec::new();
    for r in arr(v.get("reg")) {
        let name = r.get("name").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        if name.is_empty() {
            continue;
        }
        if let Some(bytes) = r.get("bytes").and_then(|x| x.as_u64()).filter(|b| *b > 0) {
            reg.push((name, bytes / 1048576));
        }
    }
    let find_reg = |name: &str| -> Option<u64> {
        let lname = name.to_lowercase();
        reg.iter()
            .find(|(n, _)| {
                let ln = n.to_lowercase();
                ln == lname || ln.contains(&lname) || lname.contains(&ln)
            })
            .map(|(_, mb)| *mb)
    };

    let mut gpus = Vec::new();
    for g in arr(v.get("gpus")) {
        let name = g
            .get("Name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let ln = name.to_lowercase();
        // 过滤虚拟/基础显示适配器
        if ln.contains("basic") || ln.contains("hyper-v") || ln.contains("indirect") {
            continue;
        }
        if let Some(mb) = find_nv(&name).or_else(|| find_reg(&name)) {
            let source = if find_nv(&name).is_some() { "nvidia-smi" } else { "registry" };
            gpus.push(GpuInfo { name, vram_mb: Some(mb), source: source.into() });
        } else {
            let raw = g.get("AdapterRAM").and_then(|x| x.as_u64());
            // AdapterRAM 是 uint32，≥4GB 显卡会封顶为 4293918720，视为未知
            let vram = raw.filter(|b| *b != 4293918720).map(|b| b / 1048576);
            gpus.push(GpuInfo { name, vram_mb: vram, source: "wmi".into() });
        }
    }

    // TotalVisibleMemorySize 单位是 KB
    let ram_kb = v.get("ram").and_then(|x| x.as_u64()).unwrap_or(0);

    // 架构：优先 PowerShell env，异常时回退到进程自身编译目标
    let arch_raw = v
        .get("parch")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let arch = {
        let a = norm_arch(&arch_raw);
        if a == "unknown" {
            norm_arch(std::env::consts::ARCH)
        } else {
            a
        }
    };

    Some(SystemInfo {
        os: if osver.is_empty() { os } else { format!("{os} (v{osver})") },
        cpu,
        arch,
        physical_cores: cores.map(|x| x as u32),
        logical_cores: logical.map(|x| x as u32),
        ram_gb: (ram_kb as f64 * 1024.0 / 1073741824.0 * 10.0).round() / 10.0,
        gpus,
    })
}

fn query_system_info() -> SystemInfo {
    let fallback = SystemInfo {
        os: format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH),
        cpu: "未知".into(),
        arch: norm_arch(std::env::consts::ARCH),
        physical_cores: None,
        logical_cores: None,
        ram_gb: 0.0,
        gpus: Vec::new(),
    };
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        PS_SYSINFO_SCRIPT,
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    match cmd.output() {
        Ok(out) if out.status.success() => {
            parse_system_info(&String::from_utf8_lossy(&out.stdout)).unwrap_or(fallback)
        }
        _ => fallback,
    }
}

/// 异步包装：首次调用会 spawn PowerShell 做系统检测（耗时数百毫秒），避免阻塞主线程
#[tauri::command]
async fn get_system_info() -> SystemInfo {
    tauri::async_runtime::spawn_blocking(|| SYS_INFO.get_or_init(query_system_info).clone())
        .await
        .unwrap_or_default()
}

// ---------- 更新包推荐 ----------

#[derive(Serialize)]
struct Recommendation {
    /// "CUDA" / "Vulkan" / "CPU"
    edition: String,
    /// "x64" / "arm64"
    arch: String,
    /// 本机 CUDA 大版本（优先 nvcc，未装 Toolkit 时取驱动支持的最高版本）；无法识别时为 null（前端默认 12 系列）
    cuda_major: Option<u32>,
    reason: String,
}

/// 调用外部命令并取 stdout（隐藏窗口，失败返回 None）
fn run_quiet(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 检测本机 CUDA 运行时大版本，返回 (大版本, 来源)。
/// 优先 CUDA Toolkit 的 nvcc --version（"release 12.4"，反映实际安装的 Toolkit），
/// 未安装 Toolkit 时回退到 nvidia-smi 头部的 "CUDA Version: 12.7"（= 驱动支持的最高运行时）。
/// 都失败返回 None。
fn detect_cuda_major() -> Option<(u32, &'static str)> {
    if let Some(text) = run_quiet("nvcc", &["--version"]) {
        for line in text.lines() {
            if let Some(idx) = line.find("release ") {
                let tail = &line[idx + "release ".len()..];
                let digits: String = tail
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                if let Some(m) = digits.split('.').next().and_then(|s| s.parse::<u32>().ok()) {
                    if (8..=99).contains(&m) {
                        return Some((m, "nvcc"));
                    }
                }
            }
        }
    }
    if let Some(text) = run_quiet("nvidia-smi", &[]) {
        for line in text.lines() {
            if let Some(idx) = line.find("CUDA Version") {
                let tail = &line[idx + "CUDA Version".len()..];
                let tail = tail.trim_start_matches(|c: char| !c.is_ascii_digit());
                let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(m) = digits.parse::<u32>() {
                    if (8..=99).contains(&m) {
                        return Some((m, "nvidia-smi"));
                    }
                }
            }
        }
    }
    None
}

/// 实际推荐逻辑（首次调用会跑 nvcc / nvidia-smi / wmic 子进程，耗时数百毫秒到数秒），仅供 spawn_blocking 调用
fn recommend_impl() -> Recommendation {
    let info = SYS_INFO.get_or_init(query_system_info);
    // 架构判断异常（unknown/x86）时按 x64 兜底（llama.cpp Windows 仅提供 x64/arm64 包）
    let arch = if info.arch == "arm64" { "arm64" } else { "x64" };
    let arch_note = if info.arch != "x64" && info.arch != "arm64" {
        "（架构未能识别，按 x64 处理）"
    } else {
        ""
    };

    if let Some(g) = info.gpus.iter().find(|g| {
        let n = g.name.to_lowercase();
        n.contains("nvidia") || n.contains("geforce")
    }) {
        let vram = g
            .vram_mb
            .map(|m| format!("，显存约 {:.1} GB", m as f64 / 1024.0))
            .unwrap_or_default();
        return match detect_cuda_major() {
            Some((m, source)) => {
                let source_note = if source == "nvcc" {
                    format!("已安装 CUDA Toolkit {m}（nvcc）")
                } else {
                    format!("驱动支持 CUDA {m}（未检测到 Toolkit）")
                };
                Recommendation {
                    edition: "CUDA".into(),
                    arch: arch.into(),
                    cuda_major: Some(m),
                    reason: format!(
                        "检测到 NVIDIA 显卡：{g_name}{vram}，{source_note}，推荐 CUDA {m} 系列安装包 · {arch}{arch_note}",
                        g_name = g.name
                    ),
                }
            }
            None => Recommendation {
                edition: "CUDA".into(),
                arch: arch.into(),
                cuda_major: None,
                reason: format!(
                    "检测到 NVIDIA 显卡：{}{vram}，但未能识别 CUDA 运行时版本（可能驱动过旧），默认选择 CUDA 12 系列包 · {arch}{arch_note}",
                    g.name
                ),
            },
        };
    }
    if let Some(g) = info.gpus.first() {
        return Recommendation {
            edition: "Vulkan".into(),
            arch: arch.into(),
            cuda_major: None,
            reason: format!(
                "检测到显卡：{}（非 NVIDIA），Vulkan 版兼容性最好 · {arch}{arch_note}",
                g.name
            ),
        };
    }
    Recommendation {
        edition: "CPU".into(),
        arch: arch.into(),
        cuda_major: None,
        reason: format!("未检测到可用 GPU，建议使用 CPU 版本 · {arch}{arch_note}"),
    }
}

/// 异步包装：同 sample_stats，避免首次系统检测阻塞主线程导致 UI 卡死
#[tauri::command]
async fn get_recommended_edition() -> Recommendation {
    tauri::async_runtime::spawn_blocking(recommend_impl)
        .await
        .unwrap_or_else(|_| Recommendation {
            edition: "CPU".into(),
            arch: "x64".into(),
            cuda_major: None,
            reason: "系统检测失败，默认推荐 CPU 版本".into(),
        })
}

// ---------- 运行参数采样 ----------

#[derive(Serialize, Clone, Default)]
struct StatsSample {
    /// 系统 CPU 总使用率（%）
    cpu_percent: Option<f64>,
    ram_used_gb: Option<f64>,
    ram_total_gb: Option<f64>,
    /// GPU 利用率（%，需 NVIDIA + nvidia-smi）
    gpu_util_percent: Option<f64>,
    gpu_mem_used_mb: Option<u64>,
    gpu_name: Option<String>,
    /// llama-server 进程 CPU 占用（%，单核口径，多核可超 100）
    proc_cpu_percent: Option<f64>,
    proc_mem_mb: Option<f64>,
}

const STATS_PS_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$cpu = (Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average
$os = Get-CimInstance Win32_OperatingSystem | Select-Object -First 1 FreePhysicalMemory, TotalVisibleMemorySize
$p = $null
if ($PIDARG) { $p = Get-Process -Id $PIDARG -ErrorAction SilentlyContinue | Select-Object CPU, WorkingSet64 }
[pscustomobject]@{ cpu = $cpu; freeKB = $os.FreePhysicalMemory; totalKB = $os.TotalVisibleMemorySize; proc = $p } | ConvertTo-Json
"#;

/// 进程 CPU% 计算：记录上次采样的累计处理器时间，做差分
static PROC_PREV: Mutex<Option<(u32, f64, std::time::Instant)>> = Mutex::new(None);

fn parse_stats(json: &str, pid: Option<u32>) -> StatsSample {
    let mut s = StatsSample::default();
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return s;
    };
    s.cpu_percent = v.get("cpu").and_then(|x| x.as_f64());
    let free_kb = v.get("freeKB").and_then(|x| x.as_u64()).unwrap_or(0);
    let total_kb = v.get("totalKB").and_then(|x| x.as_u64()).unwrap_or(0);
    if total_kb > 0 {
        let to_gb = |kb: u64| kb as f64 * 1024.0 / 1073741824.0;
        s.ram_total_gb = Some(to_gb(total_kb));
        s.ram_used_gb = Some(to_gb(total_kb - free_kb.min(total_kb)));
    }
    if let Some(p) = v.get("proc").filter(|p| p.is_object()) {
        s.proc_mem_mb = p
            .get("WorkingSet64")
            .and_then(|x| x.as_u64())
            .map(|b| b as f64 / 1048576.0);
        if let (Some(p), Some(secs)) = (pid, p.get("CPU").and_then(|x| x.as_f64())) {
            let logical = SYS_INFO
                .get()
                .and_then(|i| i.logical_cores)
                .filter(|c| *c > 0)
                .map(|c| c as f64)
                .unwrap_or_else(|| {
                    std::thread::available_parallelism()
                        .map(|n| n.get() as f64)
                        .unwrap_or(1.0)
                });
            let now = std::time::Instant::now();
            let mut prev = PROC_PREV.lock().unwrap();
            if let Some((ppid, psecs, pt)) = *prev {
                if ppid == p {
                    let dt = now.duration_since(pt).as_secs_f64();
                    if dt > 0.05 {
                        s.proc_cpu_percent =
                            Some(((secs - psecs) / dt / logical * 100.0).clamp(0.0, logical * 100.0));
                    }
                }
            }
            *prev = Some((p, secs, now));
        }
    } else if pid.is_none() {
        *PROC_PREV.lock().unwrap() = None;
    }
    s
}

fn query_gpu() -> (Option<String>, Option<f64>, Option<u64>) {
    // 仅当系统信息阶段确认 nvidia-smi 可用时才查询，避免每次采样白跑进程
    let ok = SYS_INFO
        .get_or_init(query_system_info)
        .gpus
        .iter()
        .any(|g| g.source == "nvidia-smi");
    if !ok {
        return (None, None, None);
    }
    let mut cmd = Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=name,utilization.gpu,memory.used",
        "--format=csv,noheader,nounits",
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let Ok(out) = cmd.output() else {
        return (None, None, None);
    };
    if !out.status.success() {
        return (None, None, None);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let Some(line) = text.lines().map(|l| l.trim()).find(|l| !l.is_empty()) else {
        return (None, None, None);
    };
    let mut it = line.splitn(3, ',');
    let name = it.next().unwrap_or("").trim().to_string();
    let util = it.next().and_then(|x| x.trim().parse::<f64>().ok());
    let mem = it.next().and_then(|x| x.trim().parse::<u64>().ok());
    (Some(name), util, mem)
}

/// 实际采样逻辑（阻塞式：内部要跑 PowerShell），仅供 spawn_blocking 调用
fn sample_stats_impl(pid: Option<u32>) -> StatsSample {
    let mut s = StatsSample::default();
    let script = STATS_PS_SCRIPT.replace(
        "$PIDARG",
        &pid.map(|p| p.to_string()).unwrap_or_else(|| "$null".into()),
    );
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &script,
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    if let Ok(out) = cmd.output() {
        if out.status.success() {
            s = parse_stats(&String::from_utf8_lossy(&out.stdout), pid);
        }
    }
    let (name, util, mem) = query_gpu();
    s.gpu_name = name;
    s.gpu_util_percent = util;
    s.gpu_mem_used_mb = mem;
    s
}

/// 异步包装：PowerShell 采样耗时数百毫秒，必须放到阻塞线程池，
/// 否则同步 command 在主线程执行，1 秒一次的轮询会把 UI 事件循环卡死。
#[tauri::command]
async fn sample_stats(pid: Option<u32>) -> StatsSample {
    tauri::async_runtime::spawn_blocking(move || sample_stats_impl(pid))
        .await
        .unwrap_or_default()
}

// ---------- GitHub Release ----------

#[derive(Serialize)]
struct AssetInfo {
    name: String,
    url: String,
    edition: String,
    /// "x64" / "arm64"
    arch: String,
    /// CUDA 大版本（如 12 / 13），非 CUDA 包为 null
    cuda_major: Option<u32>,
    size: u64,
}

#[derive(Serialize)]
struct ReleaseInfo {
    tag: String,
    date: String,
    assets: Vec<AssetInfo>,
}

/// 返回 (edition, arch, cuda 大版本)。注意 "cudart" 包含 "cuda" 子串，必须先判断
fn classify_asset(name: &str) -> Option<(&'static str, &'static str, Option<u32>)> {
    let n = name.to_lowercase();
    if !n.ends_with(".zip") || !n.contains("bin-win") {
        return None;
    }
    let arch: &'static str = if n.contains("arm64") {
        "arm64"
    } else if n.contains("x64") || n.contains("x86_64") || n.contains("amd64") {
        "x64"
    } else {
        return None;
    };
    // "cuda-12.4" / "cuda12.4" → 12
    fn cuda_major_of(n: &str) -> Option<u32> {
        let idx = n.find("cuda")?;
        let tail = &n[idx + 4..];
        let tail = tail.trim_start_matches(|c: char| !c.is_ascii_digit());
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<u32>().ok()
    }
    if n.contains("cudart") {
        return Some(("CUDA-cudart", arch, cuda_major_of(&n)));
    }
    if n.contains("cuda") {
        Some(("CUDA", arch, cuda_major_of(&n)))
    } else if n.contains("vulkan") {
        Some(("Vulkan", arch, None))
    } else if n.contains("cpu") {
        Some(("CPU", arch, None))
    } else {
        None
    }
}

/// 实际拉取逻辑（阻塞式：内部用 blocking HTTP 请求 GitHub），仅供 spawn_blocking 调用
fn fetch_releases_impl() -> Result<Vec<ReleaseInfo>, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("llama-ui")
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get("https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=8")
        .send()
        .map_err(|e| format!("请求 GitHub API 失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API 返回 {}", resp.status()));
    }
    let items: Vec<Value> = resp.json().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for item in items {
        let tag = item["tag_name"].as_str().unwrap_or("").to_string();
        let date = item["published_at"]
            .as_str()
            .unwrap_or("")
            .get(0..10)
            .unwrap_or("")
            .to_string();
        let mut assets = Vec::new();
        if let Some(list) = item["assets"].as_array() {
            for a in list {
                let name = a["name"].as_str().unwrap_or("").to_string();
                if let Some((edition, arch, cuda_major)) = classify_asset(&name) {
                    assets.push(AssetInfo {
                        name,
                        url: a["browser_download_url"].as_str().unwrap_or("").to_string(),
                        edition: edition.into(),
                        arch: arch.into(),
                        cuda_major,
                        size: a["size"].as_u64().unwrap_or(0),
                    });
                }
            }
        }
        // 排序：后端类别 → CUDA 大版本升序（12 在 13 前）→ x64 优先于 arm64
        assets.sort_by_key(|a| {
            (
                match a.edition.as_str() {
                    "Vulkan" => 0,
                    "CPU" => 1,
                    "CUDA" => 2,
                    _ => 3,
                },
                a.cuda_major.unwrap_or(99),
                if a.arch == "arm64" { 1 } else { 0 },
            )
        });
        out.push(ReleaseInfo { tag, date, assets });
    }
    Ok(out)
}

/// 异步包装：blocking HTTP 请求可能长达数秒（timeout 20s），
/// 同步 command 会在主线程执行导致 UI 卡死，必须放到阻塞线程池。
#[tauri::command]
async fn list_releases() -> Result<Vec<ReleaseInfo>, String> {
    tauri::async_runtime::spawn_blocking(fetch_releases_impl)
        .await
        .unwrap_or_else(|e| Err(format!("后台任务失败: {e}")))
}

// ---------- 检查 llama.cpp 更新 ----------

#[derive(Serialize)]
struct UpdateCheck {
    has_update: bool,
    latest_tag: String,
    current_tag: Option<String>,
    /// 最新版发布日期（YYYY-MM-DD）
    date: String,
}

/// 对比 GitHub 最新 Release 与本地已安装版本（current_tag）。
/// 未安装任何版本时 has_update 恒为 false（无旧版可言，不做更新提醒）。
#[tauri::command]
async fn check_update(app: AppHandle) -> Result<UpdateCheck, String> {
    let releases = tauri::async_runtime::spawn_blocking(fetch_releases_impl)
        .await
        .map_err(|e| format!("后台任务失败: {e}"))??;
    let latest = releases.first().ok_or("GitHub 未返回任何版本信息")?;
    let current = load_settings(&app).current_tag;
    let has_update = current
        .as_deref()
        .map(|c| c != latest.tag)
        .unwrap_or(false);
    Ok(UpdateCheck {
        has_update,
        latest_tag: latest.tag.clone(),
        current_tag: current,
        date: latest.date.clone(),
    })
}

// ---------- 下载与解压 ----------

#[derive(Clone, Serialize)]
struct DownloadProgress {
    stage: String,
    percent: f64,
    message: String,
    /// 实时下载速度（MB/s），仅下载阶段携带
    #[serde(skip_serializing_if = "Option::is_none")]
    speed_mb_s: Option<f64>,
}

fn emit_progress(app: &AppHandle, stage: &str, percent: f64, message: &str) {
    emit_progress_speed(app, stage, percent, message, None);
}

fn emit_progress_speed(
    app: &AppHandle,
    stage: &str,
    percent: f64,
    message: &str,
    speed_mb_s: Option<f64>,
) {
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            stage: stage.into(),
            percent: percent.clamp(0.0, 100.0),
            message: message.into(),
            speed_mb_s,
        },
    );
}

/// 请求取消当前下载任务：置位标记，下载线程在下载/解压循环中轮询到后中断并清理
#[tauri::command]
fn cancel_download(state: State<AppState>) -> Result<(), String> {
    state.download_cancel.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn download_release(
    app: AppHandle,
    state: State<AppState>,
    tag: String,
    name: String,
    url: String,
    // CUDA 主包对应的 cudart 附加包（前端按同版本自动匹配后传入）
    cudart_name: Option<String>,
    cudart_url: Option<String>,
) -> Result<(), String> {
    // cudart 附加包不需要也不允许单独安装，随 CUDA 主包自动下载
    if name.to_lowercase().contains("cudart") {
        return Err("cudart 附加包会在安装 CUDA 版时自动下载并解压，无需单独安装".into());
    }

    let mut installing = state.installing.lock().unwrap();
    if *installing {
        return Err("已有下载任务进行中".into());
    }
    *installing = true;
    drop(installing);

    let cudart = match (cudart_name, cudart_url) {
        (Some(n), Some(u)) if !n.is_empty() && !u.is_empty() => Some((n, u)),
        _ => None,
    };

    let app2 = app.clone();
    thread::spawn(move || {
        let st = app2.state::<AppState>();
        let res = do_download(&app2, &st.download_cancel, &tag, &name, &url, cudart);
        *st.installing.lock().unwrap() = false;
        // 读取并复位取消标记（无论本次是否取消，避免残留影响下次任务）
        let cancelled = st.download_cancel.load(Ordering::Relaxed);
        st.download_cancel.store(false, Ordering::Relaxed);
        match res {
            Ok(()) => {} // do_download 内部已发 done 事件
            Err(e) => {
                if cancelled {
                    emit_progress(&app2, "cancelled", 0.0, "已取消下载");
                    // 清理未完成产物：目标版本目录与临时下载目录（仅限托管目录内）
                    let managed_dir = PathBuf::from(load_settings(&app2).managed_dir.trim());
                    let dest = managed_dir.join(&tag);
                    if dest.exists() {
                        let _ = fs::remove_dir_all(&dest);
                    }
                    let dl_dir = managed_dir.join("_downloads");
                    if dl_dir.exists() {
                        let _ = fs::remove_dir_all(&dl_dir);
                    }
                } else {
                    emit_progress(&app2, "error", 0.0, &format!("失败: {e}"));
                }
            }
        }
    });
    Ok(())
}

/// 从文件名提取完整 CUDA 版本号（如 "12.4"），用于主包与 cudart 包的精确匹配
fn cuda_full_version(name: &str) -> Option<String> {
    let n = name.to_lowercase();
    let idx = n.find("cuda")?;
    let tail = &n[idx + 4..];
    let tail = tail.trim_start_matches(|c: char| !c.is_ascii_digit());
    let ver: String = tail
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if ver.is_empty() {
        None
    } else {
        Some(ver)
    }
}

/// 下载单个文件到 dl_dir，进度映射到 [from, to] 百分比区间，返回本地 zip 路径。
/// 每 0.5s 计算一次实时速度并随进度事件下发；轮询 cancel 标记，置位即中断。
/// 若响应头含长度则校验写入字节数，拦截"不完整的压缩包"。
#[allow(clippy::too_many_arguments)]
fn download_file(
    app: &AppHandle,
    cancel: &AtomicBool,
    client: &reqwest::blocking::Client,
    real_url: &str,
    dl_dir: &Path,
    name: &str,
    from: f64,
    to: f64,
) -> Result<PathBuf, String> {
    emit_progress(app, "download", from, &format!("开始下载 {name}…"));
    let mut resp = client
        .get(real_url)
        .send()
        .map_err(|e| format!("下载 {name} 失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载 {name} 返回 HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    let zip_path = dl_dir.join(name);
    let mut file = fs::File::create(&zip_path).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = 0;
    // 速度采样：每 0.5s 用增量字节 / 间隔时间估算一次
    let mut last_t = std::time::Instant::now();
    let mut last_b: u64 = 0;
    let mut speed: f64 = 0.0;
    use std::io::Write;
    loop {
        // 用户请求取消：删除半成品 zip 并中断
        if cancel.load(Ordering::Relaxed) {
            let _ = fs::remove_file(&zip_path);
            return Err("已取消下载".into());
        }
        let n = resp
            .read(&mut buf)
            .map_err(|e| format!("下载 {name} 中断: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        downloaded += n as u64;
        let elapsed = last_t.elapsed().as_secs_f64();
        if elapsed >= 0.5 {
            speed = (downloaded - last_b) as f64 / 1048576.0 / elapsed;
            last_t = std::time::Instant::now();
            last_b = downloaded;
        }
        let percent = if total > 0 {
            downloaded as f64 / total as f64
        } else {
            0.0
        };
        emit_progress_speed(
            app,
            "download",
            from + percent * (to - from),
            &format!(
                "下载 {name}: {:.1} / {} · {:.1} MB/s",
                downloaded as f64 / 1048576.0,
                if total > 0 {
                    format!("{:.1} MB", total as f64 / 1048576.0)
                } else {
                    "?".into()
                },
                speed
            ),
            Some(speed),
        );
    }
    if total > 0 && downloaded != total {
        return Err(format!(
            "下载不完整：{name} 预期 {total} 字节，实际写入 {downloaded} 字节"
        ));
    }
    Ok(zip_path)
}

/// 解压单个 zip 到 dest，进度映射到 [from, to]，返回解压出的文件条数。
/// 每个条目前轮询 cancel 标记，置位即中断（残缺目录由取消分支统一清理）。
fn extract_zip(
    app: &AppHandle,
    cancel: &AtomicBool,
    zip_path: &Path,
    dest: &Path,
    from: f64,
    to: f64,
) -> Result<usize, String> {
    let zip_file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(BufReader::new(zip_file))
        .map_err(|e| format!("打开 zip 失败: {e}"))?;
    let count = archive.len();
    let mut files = 0usize;
    let zip_name = zip_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    for i in 0..count {
        if cancel.load(Ordering::Relaxed) {
            return Err("已取消下载".into());
        }
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        let out_path = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = out_path.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut of = fs::File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut of)
                .map_err(|e| format!("解压 {} 失败: {e}", rel.display()))?;
            files += 1;
        }
        emit_progress(
            app,
            "extract",
            from + (i as f64 / count.max(1) as f64) * (to - from),
            &format!("解压 {zip_name}: {}/{}", i + 1, count),
        );
    }
    Ok(files)
}

/// 递归查找目录中文件名满足条件的文件（大小写不敏感），用于解压结果校验。
/// 与上方按精确文件名查找的 find_file 不同，这里接受谓词以支持 cudart64_*.dll 前缀匹配。
fn find_file_matching<F: Fn(&str) -> bool>(dir: &Path, pred: &F) -> Option<PathBuf> {
    let rd = fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(found) = find_file_matching(&p, pred) {
                return Some(found);
            }
        } else if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
            if pred(&name.to_lowercase()) {
                return Some(p);
            }
        }
    }
    None
}

/// 校验解压结果：主包必须有 llama-server.exe；is_cuda 时还必须有 cudart64_*.dll
fn verify_extracted(dest: &Path, is_cuda: bool) -> Result<(), String> {
    if find_file_matching(dest, &|n| n == "llama-server.exe").is_none() {
        return Err("验证失败：解压后未找到 llama-server.exe，安装不完整".into());
    }
    if is_cuda && find_file_matching(dest, &|n| n.starts_with("cudart64_")).is_none() {
        return Err("验证失败：解压后未找到 cudart64_*.dll，cudart 附加包可能未正确解压".into());
    }
    Ok(())
}

fn do_download(
    app: &AppHandle,
    cancel: &AtomicBool,
    tag: &str,
    name: &str,
    url: &str,
    cudart: Option<(String, String)>,
) -> Result<(), String> {
    let settings = load_settings(app);
    // 下载/解压只允许写入托管目录，绝不触碰用户自备目录（custom_dir）
    let managed_dir = PathBuf::from(settings.managed_dir.trim());
    if managed_dir.as_os_str().is_empty() {
        return Err("尚未在「设置 → 程序来源」中指定托管下载目录，请先设置后再下载".into());
    }
    fs::create_dir_all(&managed_dir).map_err(|e| e.to_string())?;

    // 1) 清理残留：上次失败留下的临时下载目录（含不完整压缩包）、目标版本旧目录
    emit_progress(app, "cleanup", 0.0, "清理残留文件…");
    let dl_dir = managed_dir.join("_downloads");
    if dl_dir.exists() {
        fs::remove_dir_all(&dl_dir).map_err(|e| format!("清理临时下载目录失败: {e}"))?;
    }
    fs::create_dir_all(&dl_dir).map_err(|e| e.to_string())?;
    let dest = managed_dir.join(tag);
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| format!("清理旧版本目录失败: {e}"))?;
    }
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

    // 镜像前缀（仅对 github.com 的文件下载生效）。
    // 拼接规则：镜像 + "/" + 完整原始 URL，如 gh-proxy 的标准格式
    //   https://gh-proxy.com/ + https://github.com/a/b → https://gh-proxy.com/https://github.com/a/b
    // 此前用 format!("{}{}", 镜像, url) 直接粘连，产出 "…comhttps://…" 这类畸形 URL 导致下载失败。
    let raw_mirror = settings
        .mirror
        .trim()
        .trim_end_matches('/')
        .to_string();
    let with_mirror = move |u: &str| -> String {
        if raw_mirror.is_empty() || !u.starts_with("https://github.com/") {
            return u.to_string();
        }
        // 便于使用：只填了域名没带协议时自动补 https://
        let m = if raw_mirror.starts_with("http://") || raw_mirror.starts_with("https://") {
            raw_mirror.clone()
        } else {
            format!("https://{}", raw_mirror)
        };
        format!("{}/{}", m, u)
    };

    let client = reqwest::blocking::Client::builder()
        .user_agent("llama-ui")
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;

    let is_cuda = name.to_lowercase().contains("cuda");

    if is_cuda {
        // 2) CUDA 主包：必须匹配到同版本 cudart 附加包，否则直接报错终止
        let main_ver = cuda_full_version(name).ok_or_else(|| {
            format!("无法从主包名 {name} 中识别 CUDA 版本号，无法匹配 cudart 附加包，已取消安装")
        })?;
        let arch = if name.to_lowercase().contains("arm64") {
            "arm64"
        } else {
            "x64"
        };
        let (cudart_name, cudart_url) = cudart.ok_or_else(|| {
            format!(
                "未能在该 Release 中找到与 CUDA {main_ver}（{arch}）匹配的 cudart 附加包，已取消安装。\
                 请到 GitHub Release 页面确认是否存在 cudart-llama-bin-win-cuda-{main_ver}-{arch}.zip"
            )
        })?;
        // 后端复核：cudart 与主包的 CUDA 版本、CPU 架构必须完全一致
        let cd_ver = cuda_full_version(&cudart_name).unwrap_or_default();
        if cd_ver != main_ver {
            return Err(format!(
                "cudart 版本不匹配：主包为 CUDA {main_ver}，附加包为 CUDA {cd_ver}，已取消安装"
            ));
        }
        let cd_arch = if cudart_name.to_lowercase().contains("arm64") {
            "arm64"
        } else {
            "x64"
        };
        if cd_arch != arch {
            return Err(format!(
                "cudart 架构不匹配：主包为 {arch}，附加包为 {cd_arch}，已取消安装"
            ));
        }

        // 3) 两个包依次下载（主包 0–70%，cudart 70–80%）
        emit_progress(app, "download", 0.0, "连接服务器…");
        let main_zip = download_file(
            app,
            cancel,
            &client,
            &with_mirror(url),
            &dl_dir,
            name,
            0.0,
            70.0,
        )?;
        emit_progress(app, "download", 70.0, "主包下载完成，开始下载 cudart 附加包…");
        let cudart_zip = download_file(
            app,
            cancel,
            &client,
            &with_mirror(&cudart_url),
            &dl_dir,
            &cudart_name,
            70.0,
            80.0,
        )?;

        // 4) 一起解压到 install_dir/<tag>/
        emit_progress(app, "extract", 80.0, "解压主包…");
        extract_zip(app, cancel, &main_zip, &dest, 80.0, 88.0)?;
        emit_progress(app, "extract", 88.0, "解压 cudart 附加包…");
        extract_zip(app, cancel, &cudart_zip, &dest, 88.0, 95.0)?;

        // 5) 验证解压结果完整
        emit_progress(app, "verify", 95.0, "验证解压结果…");
        verify_extracted(&dest, true)?;

        let _ = fs::remove_file(&main_zip);
        let _ = fs::remove_file(&cudart_zip);
    } else {
        // 非 CUDA 包：单文件下载 + 解压 + 验证
        emit_progress(app, "download", 0.0, "连接服务器…");
        let main_zip = download_file(
            app,
            cancel,
            &client,
            &with_mirror(url),
            &dl_dir,
            name,
            0.0,
            80.0,
        )?;
        emit_progress(app, "extract", 80.0, "解压中…");
        extract_zip(app, cancel, &main_zip, &dest, 80.0, 95.0)?;
        emit_progress(app, "verify", 95.0, "验证解压结果…");
        verify_extracted(&dest, false)?;
        let _ = fs::remove_file(&main_zip);
    }

    // 更新 current_tag
    let mut s = load_settings(app);
    s.current_tag = Some(tag.to_string());
    save_settings_inner(app, &s);
    emit_progress(app, "done", 100.0, &format!("已安装到 {}", dest.display()));
    Ok(())
}

// ---------- 进程管理 ----------

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct ServerConfig {
    exe: String,
    model: String,
    alias: Option<String>,
    host: String,
    port: u16,
    ctx_size: u32,
    predict: Option<i32>,
    parallel: Option<u32>,
    batch_size: Option<u32>,
    ubatch_size: Option<u32>,
    keep: Option<i32>,
    gpu_layers: Option<i32>,
    flash_attn: Option<String>,
    split_mode: Option<String>,
    tensor_split: Option<String>,
    main_gpu: Option<u32>,
    threads: Option<u32>,
    threads_batch: Option<u32>,
    no_mmap: bool,
    mlock: bool,
    numa: Option<String>,
    cache_type_k: Option<String>,
    cache_type_v: Option<String>,
    kv_unified: bool,
    fit: Option<String>,
    load_mode: Option<String>,
    no_warmup: bool,
    mmproj: Option<String>,
    no_mmproj_offload: bool,
    image_min_tokens: Option<u32>,
    image_max_tokens: Option<u32>,
    reasoning: Option<String>,
    chat_template_kwargs: Option<String>,
    reasoning_format: Option<String>,
    reasoning_preserve: bool,
    rope_scaling: Option<String>,
    rope_freq_base: Option<f64>,
    rope_freq_scale: Option<f64>,
    temp: Option<f64>,
    top_k: Option<i32>,
    top_p: Option<f64>,
    min_p: Option<f64>,
    repeat_penalty: Option<f64>,
    repeat_last_n: Option<i32>,
    presence_penalty: Option<f64>,
    frequency_penalty: Option<f64>,
    mirostat: Option<i32>,
    mirostat_lr: Option<f64>,
    mirostat_ent: Option<f64>,
    seed: Option<i64>,
    api_key: Option<String>,
    threads_http: Option<u32>,
    no_webui: bool,
    metrics: bool,
    slots: bool,
    jinja: bool,
    chat_template: Option<String>,
    chat_template_file: Option<String>,
    embedding: bool,
    pooling: Option<String>,
    log_format: Option<String>,
    extra_args: String,
}

fn opt_s(args: &mut Vec<String>, flag: &str, v: &Option<String>) {
    if let Some(s) = v {
        if !s.trim().is_empty() {
            args.push(flag.into());
            args.push(s.trim().to_string());
        }
    }
}

fn opt_n<T: ToString>(args: &mut Vec<String>, flag: &str, v: &Option<T>) {
    if let Some(n) = v {
        args.push(flag.into());
        args.push(n.to_string());
    }
}

fn flag_if(args: &mut Vec<String>, flag: &str, on: bool) {
    if on {
        args.push(flag.into());
    }
}

#[derive(Clone, Serialize)]
struct ServerStatus {
    running: bool,
    pid: Option<u32>,
    model: Option<String>,
    port: Option<u16>,
}

#[derive(Default)]
struct AppState {
    child: Mutex<Option<Child>>,
    logs: Mutex<VecDeque<String>>,
    installing: Mutex<bool>,
    running_cfg: Mutex<Option<ServerConfig>>,
    /// 下载取消标记：cancel_download 置位，下载线程在循环中轮询，结束后复位
    download_cancel: AtomicBool,
}

fn push_log(app: &AppHandle, line: String) {
    let st = app.state::<AppState>();
    {
        let mut logs = st.logs.lock().unwrap();
        if logs.len() > 2000 {
            logs.pop_front();
        }
        logs.push_back(line.clone());
    }
    let _ = app.emit("server-log", line);
}

fn spawn_reader(app: AppHandle, stream: impl Read + Send + 'static) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let trimmed = line.trim_end().to_string();
                    if !trimmed.is_empty() {
                        push_log(&app, trimmed);
                    }
                }
            }
        }
    });
}

fn status_of(state: &AppState) -> ServerStatus {
    let mut guard = state.child.lock().unwrap();
    let alive = match guard.as_mut() {
        Some(child) => child.try_wait().ok().flatten().is_none(),
        None => false,
    };
    if !alive {
        *guard = None;
    }
    let cfg = state.running_cfg.lock().unwrap().clone();
    match (alive, cfg) {
        (true, Some(c)) => ServerStatus {
            running: true,
            pid: guard.as_ref().map(|c2| c2.id()),
            model: Some(c.model),
            port: Some(c.port),
        },
        (true, None) => ServerStatus {
            running: true,
            pid: guard.as_ref().map(|c2| c2.id()),
            model: None,
            port: None,
        },
        _ => ServerStatus {
            running: false,
            pid: None,
            model: None,
            port: None,
        },
    }
}

#[tauri::command]
fn get_server_status(state: State<AppState>) -> ServerStatus {
    status_of(&state)
}

#[tauri::command]
fn get_logs(state: State<AppState>) -> Vec<String> {
    state.logs.lock().unwrap().iter().cloned().collect()
}

#[tauri::command]
fn start_server(app: AppHandle, state: State<AppState>, cfg: ServerConfig) -> Result<ServerStatus, String> {
    {
        let mut guard = state.child.lock().unwrap();
        if let Some(child) = guard.as_mut() {
            if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                return Err("服务已在运行中，请先停止".into());
            }
        }
    }
    if cfg.exe.is_empty() {
        return Err("未找到 llama-server.exe，请先在「下载与更新」中安装".into());
    }
    if !Path::new(&cfg.exe).exists() {
        return Err(format!("llama-server 不存在: {}", cfg.exe));
    }

    let mut args: Vec<String> = vec![
        "-m".into(),
        cfg.model.clone(),
        "--host".into(),
        cfg.host.clone(),
        "--port".into(),
        cfg.port.to_string(),
        "-c".into(),
        cfg.ctx_size.to_string(),
    ];
    opt_s(&mut args, "--alias", &cfg.alias);
    opt_n(&mut args, "-n", &cfg.predict);
    opt_n(&mut args, "--parallel", &cfg.parallel);
    opt_n(&mut args, "-b", &cfg.batch_size);
    opt_n(&mut args, "-ub", &cfg.ubatch_size);
    opt_n(&mut args, "--keep", &cfg.keep);
    opt_n(&mut args, "-ngl", &cfg.gpu_layers);
    opt_s(&mut args, "-fa", &cfg.flash_attn);
    opt_s(&mut args, "-sm", &cfg.split_mode);
    opt_s(&mut args, "-ts", &cfg.tensor_split);
    opt_n(&mut args, "-mg", &cfg.main_gpu);
    opt_n(&mut args, "-t", &cfg.threads);
    opt_n(&mut args, "-tb", &cfg.threads_batch);
    flag_if(&mut args, "--no-mmap", cfg.no_mmap);
    flag_if(&mut args, "--mlock", cfg.mlock);
    opt_s(&mut args, "--numa", &cfg.numa);
    opt_s(&mut args, "--cache-type-k", &cfg.cache_type_k);
    opt_s(&mut args, "--cache-type-v", &cfg.cache_type_v);
    opt_s(&mut args, "--rope-scaling", &cfg.rope_scaling);
    opt_n(&mut args, "--rope-freq-base", &cfg.rope_freq_base);
    opt_n(&mut args, "--rope-freq-scale", &cfg.rope_freq_scale);
    opt_n(&mut args, "--temp", &cfg.temp);
    opt_n(&mut args, "--top-k", &cfg.top_k);
    opt_n(&mut args, "--top-p", &cfg.top_p);
    opt_n(&mut args, "--min-p", &cfg.min_p);
    opt_n(&mut args, "--repeat-penalty", &cfg.repeat_penalty);
    opt_n(&mut args, "--repeat-last-n", &cfg.repeat_last_n);
    opt_n(&mut args, "--presence-penalty", &cfg.presence_penalty);
    opt_n(&mut args, "--frequency-penalty", &cfg.frequency_penalty);
    opt_n(&mut args, "--mirostat", &cfg.mirostat);
    opt_n(&mut args, "--mirostat-lr", &cfg.mirostat_lr);
    opt_n(&mut args, "--mirostat-ent", &cfg.mirostat_ent);
    opt_n(&mut args, "--seed", &cfg.seed);
    opt_s(&mut args, "--api-key", &cfg.api_key);
    opt_n(&mut args, "--threads-http", &cfg.threads_http);
    flag_if(&mut args, "--no-webui", cfg.no_webui);
    flag_if(&mut args, "--metrics", cfg.metrics);
    flag_if(&mut args, "--slots", cfg.slots);
    flag_if(&mut args, "--jinja", cfg.jinja);
    opt_s(&mut args, "--chat-template", &cfg.chat_template);
    opt_s(&mut args, "--chat-template-file", &cfg.chat_template_file);
    opt_s(&mut args, "--mmproj", &cfg.mmproj);
    flag_if(&mut args, "--no-mmproj-offload", cfg.no_mmproj_offload);
    opt_n(&mut args, "--image-min-tokens", &cfg.image_min_tokens);
    opt_n(&mut args, "--image-max-tokens", &cfg.image_max_tokens);
    flag_if(&mut args, "--kv-unified", cfg.kv_unified);
    opt_s(&mut args, "--fit", &cfg.fit);
    opt_s(&mut args, "--load-mode", &cfg.load_mode);
    flag_if(&mut args, "--no-warmup", cfg.no_warmup);
    opt_s(&mut args, "--reasoning", &cfg.reasoning);
    opt_s(&mut args, "--chat-template-kwargs", &cfg.chat_template_kwargs);
    opt_s(&mut args, "--reasoning-format", &cfg.reasoning_format);
    flag_if(&mut args, "--reasoning-preserve", cfg.reasoning_preserve);
    flag_if(&mut args, "--embedding", cfg.embedding);
    opt_s(&mut args, "--pooling", &cfg.pooling);
    opt_s(&mut args, "--log-format", &cfg.log_format);
    if !cfg.extra_args.trim().is_empty() {
        let extra = shell_words::split(&cfg.extra_args)
            .map_err(|e| format!("额外参数解析失败: {e}"))?;
        args.extend(extra);
    }

    push_log(
        &app,
        format!("[llama-ui] 启动: {} {}", cfg.exe, args.join(" ")),
    );

    let mut cmd = Command::new(&cfg.exe);
    cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("启动失败: {e}"))?;
    let pid = child.id();

    // Windows：把子进程绑定到 Job Object（KILL_ON_JOB_CLOSE）。
    // 本应用无论正常退出、闪退还是被强杀，OS 都会在进程消亡时关闭 Job 句柄，
    // 从而自动终止 llama-server，避免孤儿进程持续占用 GPU/显存。
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let bind = || -> Result<(), String> {
            let job = win32job::Job::create().map_err(|e| e.to_string())?;
            let mut info = job.query_extended_limit_info().map_err(|e| e.to_string())?;
            info.limit_kill_on_job_close();
            job.set_extended_limit_info(&info).map_err(|e| e.to_string())?;
            job.assign_process(child.as_raw_handle() as _)
                .map_err(|e| e.to_string())?;
            // 故意泄漏 Job 句柄：它必须存活到本进程退出，句柄关闭即触发 KILL_ON_JOB_CLOSE
            std::mem::forget(job);
            Ok(())
        };
        if let Err(e) = bind() {
            push_log(
                &app,
                format!(
                    "[llama-ui] Job Object 绑定失败（不影响启动，但应用异常退出时可能残留子进程）: {e}"
                ),
            );
        }
    }

    if let Some(out) = child.stdout.take() {
        spawn_reader(app.clone(), out);
    }
    if let Some(err) = child.stderr.take() {
        spawn_reader(app.clone(), err);
    }

    *state.running_cfg.lock().unwrap() = Some(cfg.clone());
    *state.child.lock().unwrap() = Some(child);

    let status = ServerStatus {
        running: true,
        pid: Some(pid),
        model: Some(cfg.model.clone()),
        port: Some(cfg.port),
    };
    let _ = app.emit("server-status", status.clone());
    Ok(status)
}

#[tauri::command]
fn stop_server(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let mut guard = state.child.lock().unwrap();
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    *state.running_cfg.lock().unwrap() = None;
    drop(guard);
    push_log(&app, "[llama-ui] 服务已停止".into());
    let _ = app.emit(
        "server-status",
        ServerStatus {
            running: false,
            pid: None,
            model: None,
            port: None,
        },
    );
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_data_dir,
            list_models,
            resolve_server_exe,
            list_releases,
            check_update,
            download_release,
            cancel_download,
            get_server_status,
            get_logs,
            get_system_info,
            get_recommended_edition,
            sample_stats,
            list_profiles,
            save_profile,
            delete_profile,
            start_server,
            stop_server
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
