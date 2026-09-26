//! 统一 Runtime State：Metrics + Log Events + Diagnostics + 运行事实的后端聚合存储。
//!
//! 数据流：
//! ```text
//! llama-server ──┬── /metrics ──→ Metrics Collector ──┐
//!                └── stdout/stderr ──→ Log Parser ────┤
//!                                                     ▼
//!                                              RuntimeShared
//!                                                     ▼
//!                                       runtime:updated (Tauri Event)
//!                                                     ▼
//!                                            前端 runtimeStore
//! ```

use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;

use serde::Serialize;

use crate::log_parser::{LogFacts, RuntimeDiagnostic, RuntimeEvent};
use crate::metrics::RuntimeMetrics;

/// 事件环形缓冲上限（内存保护，不无限保存历史）
pub const MAX_EVENTS: usize = 1000;
/// 诊断环形缓冲上限
pub const MAX_DIAGNOSTICS: usize = 100;

/// 后端聚合的运行时状态（AppState 的一部分，Mutex 保护）。
#[derive(Default)]
pub struct RuntimeShared {
    /// llama-server 启动时刻（计算 uptime）
    pub started: Option<Instant>,
    /// 最近一次成功归一化的 metrics
    pub metrics: Option<RuntimeMetrics>,
    /// 最近一次 /metrics 原始解析表（llama.cpp 新增字段无需改 Rust 即可透传）
    pub raw_metrics: BTreeMap<String, f64>,
    /// "connecting"（启动中尚未首次成功）| "connected" | "unavailable" | "disabled"
    pub metrics_status: String,
    /// 最近一次 metrics 请求失败原因
    pub metrics_error: Option<String>,
    /// 日志中观察到的运行事实（Effective Configuration 来源）
    pub facts: LogFacts,
    /// 结构化事件缓冲
    pub events: VecDeque<RuntimeEvent>,
    /// 诊断缓冲
    pub diagnostics: VecDeque<RuntimeDiagnostic>,
    /// 已推送给前端的事件数（用于增量 new_events）
    pub emitted: usize,
}

impl RuntimeShared {
    pub fn push_event(&mut self, ev: RuntimeEvent) {
        if self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
            if self.emitted > 0 {
                self.emitted -= 1;
            }
        }
        self.events.push_back(ev);
    }

    pub fn push_diagnostic(&mut self, d: RuntimeDiagnostic) {
        // 去抖：连续相同消息不重复记录
        if let Some(last) = self.diagnostics.back() {
            if last.message == d.message && last.level == d.level {
                return;
            }
        }
        if self.diagnostics.len() >= MAX_DIAGNOSTICS {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(d);
    }
}

/// runtime:updated 事件负载（前端 RuntimeUpdate 类型对应此结构）。
#[derive(Serialize, Clone)]
pub struct RuntimeUpdate {
    pub server: crate::ServerStatus,
    pub uptime_seconds: Option<u64>,
    pub metrics_status: String,
    pub metrics_error: Option<String>,
    pub metrics: Option<RuntimeMetrics>,
    pub raw_metrics: BTreeMap<String, f64>,
    pub observed: LogFacts,
    /// 本次启动的请求配置（serde_json::Value，前端已有完整类型）
    pub requested: Option<serde_json::Value>,
    /// 请求的上下文长度（用于"生效上下文 < 请求上下文"诊断）
    pub context_requested: Option<u32>,
    /// 自上次推送以来的新事件（全量拉取时为缓冲内全部事件）
    pub new_events: Vec<RuntimeEvent>,
    pub diagnostics: Vec<RuntimeDiagnostic>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_parser::parse_line;

    #[test]
    fn event_ring_buffer_keeps_max() {
        let mut rt = RuntimeShared::default();
        for i in 0..(MAX_EVENTS + 50) {
            rt.push_event(parse_line(&format!("INFO [main] line {i}")));
        }
        assert_eq!(rt.events.len(), MAX_EVENTS);
        // 最老的事件被挤出
        assert!(!rt.events.front().unwrap().message.contains("line 0"));
        assert!(rt.events.back().unwrap().message.contains(&format!("line {}", MAX_EVENTS + 49)));
    }

    #[test]
    fn diagnostic_dedup_consecutive() {
        let mut rt = RuntimeShared::default();
        let ev = parse_line("ERROR [x] CUDA error: out of memory");
        let d1 = crate::log_parser::diagnose(&ev).unwrap();
        rt.push_diagnostic(d1);
        let d2 = crate::log_parser::diagnose(&parse_line("ERROR [x] CUDA error: out of memory")).unwrap();
        rt.push_diagnostic(d2);
        assert_eq!(rt.diagnostics.len(), 1);
        // 不同消息不合并
        let d3 = crate::log_parser::diagnose(&parse_line("ERROR [x] something else")).unwrap();
        rt.push_diagnostic(d3);
        assert_eq!(rt.diagnostics.len(), 2);
    }

    #[test]
    fn emitted_index_tracks_ring_buffer() {
        let mut rt = RuntimeShared::default();
        rt.push_event(parse_line("INFO [main] a"));
        rt.push_event(parse_line("INFO [main] b"));
        rt.emitted = 2;
        rt.push_event(parse_line("INFO [main] c"));
        // 环未满：emitted 不变
        assert_eq!(rt.emitted, 2);
        assert_eq!(rt.events.len(), 3);
    }
}
