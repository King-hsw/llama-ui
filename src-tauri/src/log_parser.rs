//! P0-3：Structured Log Parser —— 原始日志 → RuntimeEvent（结构化事件）+ LogFacts
//! （运行事实提取）+ RuntimeDiagnostic（诊断规则）。
//!
//! 原则：
//! - 原始日志保留不变（Raw Log + Parsed Event 双轨）
//! - 可扩展的关键词/规则驱动分类，不为覆盖所有日志写几百个正则
//! - 只提取真实存在于 llama.cpp 日志中的事实，提取不到就保持 None（不伪造）

use serde::Serialize;
use serde_json::{Map, Value};

/// 日志级别。旧版日志无级别标记时按 Info 处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// 结构化运行事件。
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeEvent {
    pub timestamp_ms: u64,
    pub level: LogLevel,
    /// 类别："server" / "model" / "gpu" / "cuda" / "context" / "kv_cache" /
    /// "batch" / "http" / "sampling" / "memory" / "shutdown" / "launcher" /
    /// "warning" / "error" / "unknown"
    pub category: String,
    /// 去掉级别/组件前缀后的消息正文
    pub message: String,
    /// 原始日志行（保留）
    pub raw: String,
    /// 从该行提取的结构化数据（键值对，可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Map<String, Value>>,
}

/// 运行诊断。
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeDiagnostic {
    pub timestamp_ms: u64,
    /// "warning" | "error"
    pub level: String,
    pub category: String,
    pub message: String,
    /// "log"（来自日志规则）| "metrics"（来自 Metrics Collector）
    pub source: String,
}

/// 从日志行中观察到的运行事实（Effective Runtime Configuration 的数据来源）。
/// 所有字段 best-effort 提取，提取不到保持 None。
#[derive(Debug, Clone, Default, Serialize)]
pub struct LogFacts {
    /// llama.cpp 编译版本号，如 "4795"
    pub build_number: Option<String>,
    /// llama.cpp commit 短 hash
    pub build_commit: Option<String>,
    /// 实际加载的模型路径
    pub model_path: Option<String>,
    /// 模型架构（general.architecture），如 "qwen3"
    pub model_arch: Option<String>,
    /// 模型文件大小（字节，由 "file size = X MiB" 换算）
    pub model_size_bytes: Option<f64>,
    /// 实际生效的上下文长度（n_ctx）
    pub n_ctx: Option<u64>,
    /// 模型训练上下文长度
    pub n_ctx_train: Option<u64>,
    pub n_batch: Option<u64>,
    pub n_ubatch: Option<u64>,
    pub n_parallel: Option<u64>,
    /// 实际 offload 到 GPU 的层数（"offloaded 41/43 layers to GPU"）
    pub n_gpu_layers_offloaded: Option<u64>,
    /// 模型总层数
    pub n_gpu_layers_total: Option<u64>,
    /// HTTP 监听地址（"server listening on http://..."）
    pub listening_addr: Option<String>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 从行首附近识别日志级别（llama.cpp: "INFO [...]", "WARN [...]", "ERROR [...]"；
/// JSON 日志：{"level":"INFO",...}）
fn detect_level(line: &str) -> LogLevel {
    let head_len = line.len().min(96);
    let head = &line[..head_len];
    let lower = head.to_ascii_lowercase();
    // 顺序敏感：先 error/warn 再 info（避免误匹配消息文本，只看行首区域）
    if lower.contains("error") || lower.contains("[err]") {
        return LogLevel::Error;
    }
    if lower.contains("warn") {
        return LogLevel::Warn;
    }
    if lower.contains("trace") {
        return LogLevel::Trace;
    }
    if lower.contains("debug") {
        return LogLevel::Debug;
    }
    if lower.contains("info") {
        return LogLevel::Info;
    }
    LogLevel::Info
}

/// 去掉 "INFO [ component] " / "2026/09/26 12:00:00 INFO [c] " 类前缀，返回消息正文。
fn strip_prefix(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut msg = line;
    // 找 "] "（组件括号结束）且前面 96 字符内出现过级别标记
    if let Some(idx) = line.find("] ") {
        let head_len = idx.min(96);
        let head = line[..head_len].to_ascii_lowercase();
        if head.contains("info") || head.contains("warn") || head.contains("error")
            || head.contains("debug") || head.contains("trace")
        {
            msg = &line[idx + 2..];
        }
    }
    // 去掉尾部 \r（进度条行）
    let _ = bytes;
    msg.trim_end().to_string()
}

/// 提取 [component] 组件名（仅在行首 96 字符内找）
fn detect_component(line: &str) -> Option<String> {
    let head_len = line.len().min(96);
    let head = &line[..head_len];
    let open = head.find('[')?;
    let close = head[open..].find(']')? + open;
    let name = head[open + 1..close].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_ascii_lowercase())
    }
}

/// 关键词驱动的类别判定。顺序即优先级。
fn detect_category(component: Option<&str>, message: &str, level: LogLevel, raw: &str) -> String {
    let m = message.to_ascii_lowercase();
    let c = component.unwrap_or("").to_string();

    if raw.starts_with("[llama-ui]") {
        return "launcher".into();
    }
    if m.contains("loading model") {
        return "model".into();
    }
    if m.contains("cuda") || c.contains("cuda") {
        return "cuda".into();
    }
    if m.contains("vulkan") {
        return "gpu".into();
    }
    if m.contains("offload") || m.contains("n_gpu_layers") || m.contains("gpu layers") {
        return "gpu".into();
    }
    if m.contains("kv_cache") || m.contains("kv cache") || m.contains("kv self") {
        return "kv_cache".into();
    }
    if m.contains("batch") {
        return "batch".into();
    }
    if m.contains("n_ctx") || m.contains("context") || m.contains("rope") {
        return "context".into();
    }
    if m.contains("memory") || m.contains("buffer size") {
        return "memory".into();
    }
    if m.contains("sampling") || m.contains("sampler") {
        return "sampling".into();
    }
    if m.contains("shutdown") || m.contains("exiting") || m.contains("terminat") {
        return "shutdown".into();
    }
    if m.contains("listening") || m.contains("http") {
        return "http".into();
    }
    if m.contains("model_loader") || m.contains("load_model") || m.contains("load_tensors")
        || m.contains("gguf") || m.contains("ggml") || m.contains("model")
    {
        return "model".into();
    }
    // 组件名兜底
    if c.contains("model") || c.contains("loader") {
        return "model".into();
    }
    if c.contains("server") || c.contains("srv") || c.contains("main") {
        return "server".into();
    }
    match level {
        LogLevel::Error => "error".into(),
        LogLevel::Warn => "warning".into(),
        _ => "server".into(),
    }
}

/// 解析单行日志为 RuntimeEvent（原始行始终完整保留在 raw 中）。
pub fn parse_line(raw: &str) -> RuntimeEvent {
    let level = detect_level(raw);
    let component = detect_component(raw);
    let message = strip_prefix(raw);
    let category = detect_category(component.as_deref(), &message, level, raw);
    RuntimeEvent {
        timestamp_ms: now_ms(),
        level,
        category,
        message,
        raw: raw.to_string(),
        data: None,
    }
}

/// 从行中取 "needle" 之后的整段（trim 后）
fn after<'a>(line: &'a str, needle: &str) -> Option<&'a str> {
    let idx = line.find(needle)?;
    Some(line[idx + needle.len()..].trim())
}

/// 从位置起解析连续数字
fn take_number(s: &str) -> Option<u64> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// 解析 "key = value"（key 精确匹配 needle，value 是数字）
fn kv_number(line: &str, key: &str) -> Option<u64> {
    let v = after(line, &format!("{key} = "))?;
    take_number(v)
}

/// "file size = 12345.67 MiB" → 字节
fn file_size_bytes(line: &str) -> Option<f64> {
    let v = after(line, "file size = ")?;
    let mut it = v.split_whitespace();
    let num: f64 = it.next()?.parse().ok()?;
    let unit = it.next()?.to_ascii_lowercase();
    let mult = match unit.as_str() {
        "kib" | "kb" => 1024.0,
        "mib" | "mb" => 1024.0 * 1024.0,
        "gib" | "gb" => 1024.0 * 1024.0 * 1024.0,
        "tib" | "tb" => 1024.0f64.powi(4),
        _ => return None,
    };
    Some(num * mult)
}

/// 提取该行中的运行事实。每行只命中其中少量模式，返回的 struct 中
/// 仅命中的字段为 Some。
pub fn extract_facts(raw: &str) -> LogFacts {
    let mut f = LogFacts::default();

    // "build: 4795 (b6125) with ..."
    if let Some(rest) = after(raw, "build: ") {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.parse::<u64>().is_ok() {
            f.build_number = Some(digits);
            if let (Some(o), rest2) = (rest.find('('), &rest[..]) {
                if let Some(c) = rest2[o + 1..].find(')') {
                    f.build_commit = Some(rest2[o + 1..o + 1 + c].to_string());
                }
            }
        }
    }

    // "loading model from D:\models\x.gguf"（可能带 "- n_ctx = ..." 尾巴，取到行尾）
    if let Some(p) = after(raw, "loading model from ") {
        let path = p.split(" - ").next().unwrap_or(p).trim().trim_matches('"');
        if !path.is_empty() && !path.starts_with(char::is_whitespace) {
            f.model_path = Some(path.to_string());
        }
    }

    if let Some(b) = file_size_bytes(raw) {
        f.model_size_bytes = Some(b);
    }

    // "general.architecture str = qwen3" / "arch = qwen3"
    if let Some(p) = after(raw, "general.architecture") {
        // 可能形如 " str = qwen3"
        let v = p.trim_start().trim_start_matches("str").trim();
        let v = v.strip_prefix('=').unwrap_or(v).trim();
        let name: String = v.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        if !name.is_empty() {
            f.model_arch = Some(name);
        }
    }

    // n_ctx_train 必须在 n_ctx 之前检查（needle 精确匹配已避免互吞，但语义上独立处理）
    f.n_ctx_train = kv_number(raw, "n_ctx_train").or(f.n_ctx_train);
    f.n_ctx = kv_number(raw, "n_ctx").or(f.n_ctx);
    f.n_batch = kv_number(raw, "n_batch").or(f.n_batch);
    f.n_ubatch = kv_number(raw, "n_ubatch").or(f.n_ubatch);
    f.n_parallel = kv_number(raw, "n_parallel").or(f.n_parallel);

    // "offloaded 41/43 layers to GPU"
    if let Some(p) = after(raw, "offloaded ") {
        if let Some(off) = take_number(p) {
            let rest = &p[off.to_string().len()..];
            if let Some(stripped) = rest.strip_prefix('/') {
                if let Some(total) = take_number(stripped) {
                    let tail = &stripped[total.to_string().len()..];
                    if tail.starts_with(" layers") {
                        f.n_gpu_layers_offloaded = Some(off);
                        f.n_gpu_layers_total = Some(total);
                    }
                }
            }
        }
    }

    // "server listening on http://127.0.0.1:8080"
    if let Some(p) = after(raw, "server listening on ") {
        let addr: String = p.split_whitespace().next().unwrap_or("").to_string();
        if !addr.is_empty() {
            f.listening_addr = Some(addr);
        }
    }

    f
}

/// 合并新事实：只覆盖非 None 的新值（后者优先——越新的日志越代表当前状态）。
pub fn merge_facts(dst: &mut LogFacts, src: LogFacts) {
    if src.build_number.is_some() { dst.build_number = src.build_number; }
    if src.build_commit.is_some() { dst.build_commit = src.build_commit; }
    if src.model_path.is_some() { dst.model_path = src.model_path; }
    if src.model_arch.is_some() { dst.model_arch = src.model_arch; }
    if src.model_size_bytes.is_some() { dst.model_size_bytes = src.model_size_bytes; }
    if src.n_ctx.is_some() { dst.n_ctx = src.n_ctx; }
    if src.n_ctx_train.is_some() { dst.n_ctx_train = src.n_ctx_train; }
    if src.n_batch.is_some() { dst.n_batch = src.n_batch; }
    if src.n_ubatch.is_some() { dst.n_ubatch = src.n_ubatch; }
    if src.n_parallel.is_some() { dst.n_parallel = src.n_parallel; }
    if src.n_gpu_layers_offloaded.is_some() { dst.n_gpu_layers_offloaded = src.n_gpu_layers_offloaded; }
    if src.n_gpu_layers_total.is_some() { dst.n_gpu_layers_total = src.n_gpu_layers_total; }
    if src.listening_addr.is_some() { dst.listening_addr = src.listening_addr; }
}

/// 诊断规则表：对单条事件做规则匹配，命中返回诊断。
/// 规则可扩展（未来：KV Cache too large / GPU offload insufficient / ...）。
pub fn diagnose(ev: &RuntimeEvent) -> Option<RuntimeDiagnostic> {
    let m = ev.message.to_ascii_lowercase();
    let mk = |level: &str, message: String| -> RuntimeDiagnostic {
        RuntimeDiagnostic {
            timestamp_ms: ev.timestamp_ms,
            level: level.into(),
            category: ev.category.clone(),
            message,
            source: "log".into(),
        }
    };
    // 显式规则优先于级别透传
    if m.contains("out of memory") {
        return Some(mk("error", "显存/内存不足（out of memory）".into()));
    }
    if m.contains("cuda error") || m.contains("cudnn error") {
        return Some(mk("error", "CUDA 运行时错误".into()));
    }
    if m.contains("address already in use") || m.contains("failed to listen") || m.contains("failed to bind") {
        return Some(mk("error", "端口被占用，HTTP 服务监听失败".into()));
    }
    if m.contains("unauthorized") && m.contains("api key") {
        return Some(mk(
            "warning",
            "服务端开启了 --api-key 鉴权，请求被拒绝（unauthorized: Invalid API Key）".into(),
        ));
    }
    if m.contains("context too small") || m.contains("context size is too small") {
        return Some(mk("error", "上下文长度过小，无法容纳模型/请求".into()));
    }
    if m.contains("invalid argument") {
        return Some(mk("error", "启动参数无效（invalid argument）".into()));
    }
    if m.contains("failed to load") || (m.contains("load") && m.contains("failed")) {
        return Some(mk("error", "模型加载失败".into()));
    }
    // 级别透传
    match ev.level {
        LogLevel::Error => Some(mk("error", ev.message.clone())),
        LogLevel::Warn => Some(mk("warning", ev.message.clone())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modern_info_line() {
        let ev = parse_line("INFO [            main] build: 4795 (b6125) with MSVC 19.42 for x86_64");
        assert_eq!(ev.level, LogLevel::Info);
        assert_eq!(ev.category, "server");
        assert!(ev.message.starts_with("build: 4795"));
        assert_eq!(ev.raw, "INFO [            main] build: 4795 (b6125) with MSVC 19.42 for x86_64");
        let f = extract_facts("INFO [            main] build: 4795 (b6125) with MSVC 19.42 for x86_64");
        assert_eq!(f.build_number.as_deref(), Some("4795"));
        assert_eq!(f.build_commit.as_deref(), Some("b6125"));
    }

    #[test]
    fn parse_cuda_offload() {
        let raw = "INFO [          ggml_cuda] offloaded 41/43 layers to GPU";
        let ev = parse_line(raw);
        assert_eq!(ev.level, LogLevel::Info);
        assert_eq!(ev.category, "cuda");
        let f = extract_facts(raw);
        assert_eq!(f.n_gpu_layers_offloaded, Some(41));
        assert_eq!(f.n_gpu_layers_total, Some(43));
    }

    #[test]
    fn parse_context_facts() {
        let raw = "main: n_ctx = 98304";
        let f = extract_facts(raw);
        assert_eq!(f.n_ctx, Some(98304));
        // n_ctx_train 不应误报为 n_ctx
        let raw2 = "main: n_ctx_train = 32768";
        let f2 = extract_facts(raw2);
        assert_eq!(f2.n_ctx_train, Some(32768));
        assert_eq!(f2.n_ctx, None);
    }

    #[test]
    fn parse_batch_and_parallel() {
        let f = extract_facts("INFO [main] n_batch = 2048, n_ubatch = 512, n_parallel = 2");
        assert_eq!(f.n_batch, Some(2048));
        assert_eq!(f.n_ubatch, Some(512));
        assert_eq!(f.n_parallel, Some(2));
    }

    #[test]
    fn parse_model_loader_line() {
        let raw = "INFO [llama_model_loader] loading model from D:\\models\\qwen3.gguf";
        let ev = parse_line(raw);
        assert_eq!(ev.category, "model");
        let f = extract_facts(raw);
        assert_eq!(f.model_path.as_deref(), Some("D:\\models\\qwen3.gguf"));
    }

    #[test]
    fn parse_file_size() {
        let f = extract_facts("INFO [llama_model_loader] - file size = 15234.56 MiB");
        let b = f.model_size_bytes.unwrap();
        assert!((b - 15234.56 * 1024.0 * 1024.0).abs() < 1.0);
    }

    #[test]
    fn parse_arch() {
        let f = extract_facts("INFO [llama_model_loader] - kv   0 :  general.architecture str = qwen3");
        assert_eq!(f.model_arch.as_deref(), Some("qwen3"));
    }

    #[test]
    fn parse_listening() {
        let f = extract_facts("INFO [            main] server listening on http://127.0.0.1:8080");
        assert_eq!(f.listening_addr.as_deref(), Some("http://127.0.0.1:8080"));
    }

    #[test]
    fn parse_warn_and_error_levels() {
        assert_eq!(parse_line("WARN [ggml_cuda] something odd").level, LogLevel::Warn);
        assert_eq!(parse_line("ERROR [main] load failed").level, LogLevel::Error);
        let ev = parse_line("ERROR [main] CUDA error: out of memory");
        assert_eq!(ev.category, "cuda");
    }

    #[test]
    fn parse_launcher_line() {
        let ev = parse_line("[llama-ui] 启动: C:\\llama-server.exe -m x.gguf");
        assert_eq!(ev.category, "launcher");
    }

    #[test]
    fn parse_old_format_without_level() {
        // 旧版无级别标记的行：按 Info，按关键词分类
        let ev = parse_line("main: server is listening on http://127.0.0.1:8080 - starting the main loop");
        assert_eq!(ev.level, LogLevel::Info);
        assert_eq!(ev.category, "http");
    }

    #[test]
    fn diagnose_oom_rule() {
        let ev = parse_line("ERROR [ggml_cuda] ggml_backend_cuda_alloc_buffer: failed to allocate ... CUDA error: out of memory");
        let d = diagnose(&ev).expect("应命中 OOM 规则");
        assert_eq!(d.level, "error");
        assert!(d.message.contains("out of memory") || d.message.contains("内存不足"));
        assert_eq!(d.source, "log");
    }

    #[test]
    fn diagnose_port_in_use() {
        let ev = parse_line("ERROR [main] bind() failed, address already in use");
        let d = diagnose(&ev).unwrap();
        assert!(d.message.contains("端口"));
    }

    #[test]
    fn diagnose_passthrough() {
        let warn = parse_line("WARN [x] KV cache quantization is experimental");
        let d = diagnose(&warn).unwrap();
        assert_eq!(d.level, "warning");
        // 普通信息行不产生诊断
        let info = parse_line("INFO [main] server listening on http://127.0.0.1:8080");
        assert!(diagnose(&info).is_none());
    }

    #[test]
    fn merge_facts_overrides_non_none() {
        let mut dst = LogFacts { n_ctx: Some(4096), ..Default::default() };
        merge_facts(&mut dst, LogFacts { n_ctx: Some(8192), model_arch: Some("qwen3".into()), ..Default::default() });
        assert_eq!(dst.n_ctx, Some(8192));
        assert_eq!(dst.model_arch.as_deref(), Some("qwen3"));
        merge_facts(&mut dst, LogFacts::default());
        assert_eq!(dst.n_ctx, Some(8192)); // None 不覆盖
    }
}
