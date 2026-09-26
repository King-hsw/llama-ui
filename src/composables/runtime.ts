/**
 * P0 统一 Runtime State 前端 store。
 *
 * 数据流：Rust RuntimeShared → runtime:updated 事件 → runtimeStore → Dashboard。
 * 前端不自行轮询 /metrics / PID / 日志，统一由后端聚合推送。
 */
import { reactive } from "vue";
import { listen } from "@tauri-apps/api/event";

export interface RuntimeMetrics {
  timestamp_ms: number;
  prompt_tokens_total: number | null;
  prompt_seconds_total: number | null;
  tokens_predicted_total: number | null;
  tokens_predicted_seconds_total: number | null;
  prompt_tokens_per_second: number | null;
  predicted_tokens_per_second: number | null;
  kv_cache_usage_ratio: number | null;
  kv_cache_tokens: number | null;
  requests_processing: number | null;
  requests_deferred: number | null;
  n_decode_total: number | null;
}

export interface RuntimeEvent {
  timestamp_ms: number;
  level: "trace" | "debug" | "info" | "warn" | "error";
  category: string;
  message: string;
  raw: string;
  data?: Record<string, unknown>;
}

export interface RuntimeDiagnostic {
  timestamp_ms: number;
  level: "warning" | "error";
  category: string;
  message: string;
  source: "log" | "metrics";
}

/** 日志中观察到的运行事实（Effective Configuration） */
export interface LogFacts {
  build_number: string | null;
  build_commit: string | null;
  model_path: string | null;
  model_arch: string | null;
  model_size_bytes: number | null;
  n_ctx: number | null;
  n_ctx_train: number | null;
  n_batch: number | null;
  n_ubatch: number | null;
  n_parallel: number | null;
  n_gpu_layers_offloaded: number | null;
  n_gpu_layers_total: number | null;
  listening_addr: string | null;
}

/** runtime:updated 事件负载（与 Rust RuntimeUpdate 对应） */
export interface RuntimeUpdate {
  server: {
    running: boolean;
    pid: number | null;
    model: string | null;
    port: number | null;
  };
  uptime_seconds: number | null;
  metrics_status: "connecting" | "connected" | "unavailable" | "disabled" | string;
  metrics_error: string | null;
  metrics: RuntimeMetrics | null;
  raw_metrics: Record<string, number>;
  observed: LogFacts;
  requested: Record<string, any> | null;
  context_requested: number | null;
  new_events: RuntimeEvent[];
  diagnostics: RuntimeDiagnostic[];
}

export interface TimelinePoint {
  /** 生成 TPS */
  tps: number | null;
  /** KV cache 使用率 0~1 */
  kv: number | null;
  /** 请求中的采样时刻（本地补充 GPU/VRAM 用） */
  ts: number;
}

/** 事件缓冲上限：只保留最近 N 条结构化事件，不无限保存 */
const MAX_EVENTS = 300;
/** Runtime Timeline 采样窗口：约 60 个采样点（1.5s × 60 ≈ 90s） */
const MAX_TIMELINE = 60;

export const runtimeStore = reactive({
  server: { running: false, pid: null as number | null, model: null as string | null, port: null as number | null },
  uptimeSeconds: null as number | null,
  metricsStatus: "unavailable" as string,
  metricsError: null as string | null,
  metrics: null as RuntimeMetrics | null,
  rawMetrics: {} as Record<string, number>,
  observed: null as LogFacts | null,
  requested: null as Record<string, any> | null,
  contextRequested: null as number | null,
  diagnostics: [] as RuntimeDiagnostic[],
  events: [] as RuntimeEvent[],
  /** Runtime Timeline 环形缓冲（有限窗口） */
  timeline: [] as TimelinePoint[],
});

function applyUpdate(u: RuntimeUpdate) {
  runtimeStore.server = u.server;
  runtimeStore.uptimeSeconds = u.uptime_seconds;
  runtimeStore.metricsStatus = u.metrics_status;
  runtimeStore.metricsError = u.metrics_error;
  runtimeStore.metrics = u.metrics;
  runtimeStore.rawMetrics = u.raw_metrics;
  runtimeStore.observed = u.observed;
  runtimeStore.requested = u.requested;
  runtimeStore.contextRequested = u.context_requested;
  runtimeStore.diagnostics = u.diagnostics;

  // 事件增量入缓冲（尾部追加 + 头部裁剪）
  if (u.new_events.length) {
    runtimeStore.events.push(...u.new_events);
    if (runtimeStore.events.length > MAX_EVENTS) {
      runtimeStore.events.splice(0, runtimeStore.events.length - MAX_EVENTS);
    }
  }

  // Timeline：每次 metrics 更新记一个采样点（有限窗口，不无限保存）；服务停止即清空
  if (!u.server.running) {
    runtimeStore.timeline = [];
  } else if (u.metrics) {
    runtimeStore.timeline.push({
      tps: u.metrics.predicted_tokens_per_second,
      kv: u.metrics.kv_cache_usage_ratio,
      ts: Date.now(),
    });
    if (runtimeStore.timeline.length > MAX_TIMELINE) {
      runtimeStore.timeline.splice(0, runtimeStore.timeline.length - MAX_TIMELINE);
    }
  }
}

let inited = false;

export async function initRuntimeListener() {
  if (inited) return;
  inited = true;
  await listen<RuntimeUpdate>("runtime:updated", (e) => {
    applyUpdate(e.payload);
  });
}
