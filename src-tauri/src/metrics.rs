//! P0-1：llama-server /metrics 接入 —— Prometheus text format 解析与归一化。
//!
//! 数据流：Raw Metrics（原始文本）→ parse_prometheus（key→value 表）
//!        → normalize（RuntimeMetrics）→ RuntimeState → 前端。
//!
//! 设计约束：不同 llama.cpp 版本的 metric 名可能增删改（新版带 `llamacpp:` 前缀，
//! 旧版裸名），因此归一化用"查找表 + 双前缀回退"实现，不在 Rust 端写死全部字段；
//! 任何未识别的 metric 仍保留在 raw_metrics 中传给前端，前端无需改 Rust 即可消费。

use std::collections::BTreeMap;

use serde::Serialize;

/// 解析 Prometheus text format 为 (metric name → value) 表。
/// - 跳过空行与 `# HELP` / `# TYPE` 注释行
/// - 兼容 `name value` 与 `name{labels} value`（标签被剥离，仅保留 metric 名）
/// - 值只取第一个 token（容忍行尾时间戳）；NaN/Inf 等无法解析为 f64 的值跳过
/// - 同名 metric 多行（带不同 label）时后行覆盖前行；当前 llama.cpp 不输出标签，
///   该行为仅为将来兼容
pub fn parse_prometheus(text: &str) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(sp) = line.find(' ') else { continue };
        let (name_part, rest) = line.split_at(sp);
        // 剥离 {labels}
        let name = match name_part.find('{') {
            Some(i) => &name_part[..i],
            None => name_part,
        };
        if name.is_empty() {
            continue;
        }
        let value_tok = rest.trim().split_whitespace().next().unwrap_or("");
        if let Ok(v) = value_tok.parse::<f64>() {
            out.insert(name.to_string(), v);
        }
    }
    out
}

/// 查找 metric：优先新版 `llamacpp:<base>` 前缀，回退旧版裸名。
fn lookup(raw: &BTreeMap<String, f64>, base: &str) -> Option<f64> {
    raw.get(&format!("llamacpp:{base}"))
        .or_else(|| raw.get(base))
        .copied()
}

/// 归一化后的 Runtime Metrics。
///
/// 字段名与 llama.cpp /metrics 的真实输出一一对应（不虚构）：
/// - counters: prompt_tokens_total / prompt_seconds_total /
///   tokens_predicted_total / tokens_predicted_seconds_total / n_decode_total
/// - gauges: prompt_tokens_seconds / predicted_tokens_seconds /
///   kv_cache_usage_ratio / kv_cache_tokens / requests_processing / requests_deferred
/// 不存在的字段保持 None（前端显示 N/A），llama.cpp 未来新增字段走 raw_metrics。
#[derive(Debug, Clone, Serialize, Default)]
pub struct RuntimeMetrics {
    pub timestamp_ms: u64,
    /// 累计处理的 prompt tokens（counter）
    pub prompt_tokens_total: Option<f64>,
    /// 累计 prompt 处理耗时（秒，counter）
    pub prompt_seconds_total: Option<f64>,
    /// 累计生成的 tokens（counter）
    pub tokens_predicted_total: Option<f64>,
    /// 累计生成耗时（秒，counter）
    pub tokens_predicted_seconds_total: Option<f64>,
    /// 平均 prompt 吞吐 tokens/s（gauge，llama.cpp 计算好）
    pub prompt_tokens_per_second: Option<f64>,
    /// 平均生成吞吐 tokens/s（gauge）
    pub predicted_tokens_per_second: Option<f64>,
    /// KV cache 使用率，1 = 100%（gauge）
    pub kv_cache_usage_ratio: Option<f64>,
    /// KV cache 中的 token 数（gauge）
    pub kv_cache_tokens: Option<f64>,
    /// 正在处理的请求数（gauge）
    pub requests_processing: Option<f64>,
    /// 被推迟（等待槽位）的请求数（gauge）
    pub requests_deferred: Option<f64>,
    /// llama_decode() 调用总次数（counter）
    pub n_decode_total: Option<f64>,
}

/// 从 raw 表归一化出 RuntimeMetrics。
pub fn normalize(raw: &BTreeMap<String, f64>, timestamp_ms: u64) -> RuntimeMetrics {
    RuntimeMetrics {
        timestamp_ms,
        prompt_tokens_total: lookup(raw, "prompt_tokens_total"),
        prompt_seconds_total: lookup(raw, "prompt_seconds_total"),
        tokens_predicted_total: lookup(raw, "tokens_predicted_total"),
        tokens_predicted_seconds_total: lookup(raw, "tokens_predicted_seconds_total"),
        prompt_tokens_per_second: lookup(raw, "prompt_tokens_seconds"),
        predicted_tokens_per_second: lookup(raw, "predicted_tokens_seconds"),
        kv_cache_usage_ratio: lookup(raw, "kv_cache_usage_ratio"),
        kv_cache_tokens: lookup(raw, "kv_cache_tokens"),
        requests_processing: lookup(raw, "requests_processing"),
        requests_deferred: lookup(raw, "requests_deferred"),
        n_decode_total: lookup(raw, "n_decode_total"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧版 llama.cpp（无前缀）真实输出样例（字段名来自官方 server.cpp）
    const FIXTURE_LEGACY: &str = r#"
# HELP prompt_tokens_total Number of prompt tokens processed.
# TYPE prompt_tokens_total counter
prompt_tokens_total 4242
# TYPE prompt_seconds_total counter
prompt_seconds_total 12.5
# TYPE tokens_predicted_total counter
tokens_predicted_total 888
# TYPE tokens_predicted_seconds_total counter
tokens_predicted_seconds_total 55.5
# TYPE prompt_tokens_seconds gauge
prompt_tokens_seconds 339.36
# TYPE predicted_tokens_seconds gauge
predicted_tokens_seconds 16.0
# TYPE kv_cache_usage_ratio gauge
kv_cache_usage_ratio 0.25390625
# TYPE kv_cache_tokens gauge
kv_cache_tokens 1040
# TYPE requests_processing gauge
requests_processing 1
# TYPE requests_deferred gauge
requests_deferred 0
# TYPE n_decode_total counter
n_decode_total 111
# TYPE n_busy_slots_per_decode counter
n_busy_slots_per_decode 0.5
"#;

    /// 新版带 `llamacpp:` 前缀的输出（仅抽样关键字段）
    const FIXTURE_PREFIXED: &str = r#"
# TYPE llamacpp:prompt_tokens_total counter
llamacpp:prompt_tokens_total 100
# TYPE llamacpp:predicted_tokens_seconds gauge
llamacpp:predicted_tokens_seconds 42.7
# TYPE llamacpp:kv_cache_usage_ratio gauge
llamacpp:kv_cache_usage_ratio 0.5
"#;

    #[test]
    fn parse_legacy_fixture() {
        let raw = parse_prometheus(FIXTURE_LEGACY);
        assert_eq!(raw.get("prompt_tokens_total"), Some(&4242.0));
        assert_eq!(raw.get("prompt_seconds_total"), Some(&12.5));
        assert_eq!(raw.get("kv_cache_usage_ratio"), Some(&0.25390625));
        assert_eq!(raw.get("n_decode_total"), Some(&111.0));
        assert_eq!(raw.len(), 12);
    }

    #[test]
    fn parse_skips_comments_and_invalid() {
        let raw = parse_prometheus("# HELP x y\n# TYPE x counter\nnot_a_pair\nbroken\nok_name 3.5\n");
        assert_eq!(raw.get("ok_name"), Some(&3.5));
        assert_eq!(raw.len(), 1);
    }

    #[test]
    fn parse_strips_labels_and_timestamp() {
        let raw = parse_prometheus("some_metric{slot=\"0\"} 7 1727300000000\n");
        assert_eq!(raw.get("some_metric"), Some(&7.0));
    }

    #[test]
    fn normalize_legacy() {
        let raw = parse_prometheus(FIXTURE_LEGACY);
        let m = normalize(&raw, 123);
        assert_eq!(m.timestamp_ms, 123);
        assert_eq!(m.prompt_tokens_total, Some(4242.0));
        assert_eq!(m.predicted_tokens_per_second, Some(16.0));
        assert_eq!(m.kv_cache_tokens, Some(1040.0));
        assert_eq!(m.requests_deferred, Some(0.0));
    }

    #[test]
    fn normalize_prefixed() {
        let raw = parse_prometheus(FIXTURE_PREFIXED);
        let m = normalize(&raw, 1);
        assert_eq!(m.prompt_tokens_total, Some(100.0));
        assert_eq!(m.predicted_tokens_per_second, Some(42.7));
        assert_eq!(m.kv_cache_usage_ratio, Some(0.5));
        // 未上报的字段为 None，而不是伪造的 0
        assert_eq!(m.requests_processing, None);
        assert_eq!(m.n_decode_total, None);
    }

    #[test]
    fn normalize_empty() {
        let m = normalize(&parse_prometheus(""), 9);
        assert_eq!(m.timestamp_ms, 9);
        assert_eq!(m.prompt_tokens_total, None);
    }
}
