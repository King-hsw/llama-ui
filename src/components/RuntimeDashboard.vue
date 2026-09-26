<template>
  <t-card title="运行状态面板" class="rt-card mb-16">
    <template #actions>
      <t-space size="small" break-line>
        <t-tag v-if="running" theme="success" variant="light" size="small">● 运行中</t-tag>
        <t-tag v-else theme="default" variant="light" size="small">○ 已停止</t-tag>
        <t-tag :theme="metricsTheme" variant="light" size="small">
          Metrics · {{ metricsLabel }}
        </t-tag>
      </t-space>
    </template>

    <div class="rt-grid">
      <!-- A. 服务状态 -->
      <div class="rt-block span-4">
        <div class="rt-title">服务状态</div>
        <div class="rt-model mono">{{ modelDisplay || "未启动" }}</div>
        <div class="kv-list">
          <div class="kv"><span class="k">PID</span><span class="v mono">{{ running ? server.pid ?? "—" : "—" }}</span></div>
          <div class="kv"><span class="k">端口</span><span class="v mono">{{ running ? server.port ?? "—" : "—" }}</span></div>
          <div class="kv"><span class="k">运行时长</span><span class="v mono">{{ running ? uptimeDisplay : "—" }}</span></div>
          <div class="kv">
            <span class="k">llama.cpp</span>
            <span class="v mono">{{ buildDisplay }}</span>
          </div>
          <div class="kv">
            <span class="k">模型架构</span>
            <span class="v mono">{{ observed?.model_arch ?? "—" }}</span>
          </div>
          <div class="kv">
            <span class="k">模型大小</span>
            <span class="v mono">{{ fmtBytes(observed?.model_size_bytes) }}</span>
          </div>
        </div>
      </div>

      <!-- B. 推理性能（核心区） -->
      <div class="rt-block span-5">
        <div class="rt-title">推理性能</div>
        <div class="tps-row">
          <div class="tps-item">
            <div class="tps-num accent-prompt">{{ fmtTps(metrics?.prompt_tokens_per_second) }}</div>
            <div class="tps-label">输入 TPS</div>
          </div>
          <div class="tps-item">
            <div class="tps-num accent-gen">{{ fmtTps(metrics?.predicted_tokens_per_second) }}</div>
            <div class="tps-label">生成 TPS</div>
          </div>
        </div>
        <div class="kv-list">
          <div class="kv"><span class="k">输入 Token 数</span><span class="v mono">{{ fmtNum(metrics?.prompt_tokens_total) }}</span></div>
          <div class="kv"><span class="k">生成 Token 数</span><span class="v mono">{{ fmtNum(metrics?.tokens_predicted_total) }}</span></div>
          <div class="kv"><span class="k">输入耗时</span><span class="v mono">{{ fmtSecs(metrics?.prompt_seconds_total) }}</span></div>
          <div class="kv"><span class="k">生成耗时</span><span class="v mono">{{ fmtSecs(metrics?.tokens_predicted_seconds_total) }}</span></div>
          <div class="kv"><span class="k">处理中请求</span><span class="v mono">{{ fmtNum(metrics?.requests_processing) }}</span></div>
          <div class="kv"><span class="k">排队请求</span><span class="v mono">{{ fmtNum(metrics?.requests_deferred) }}</span></div>
        </div>
      </div>

      <!-- C. 上下文 / KV 缓存 -->
      <div class="rt-block span-3">
        <div class="rt-title">上下文 / KV 缓存</div>
        <div class="kv-list">
          <div class="kv">
            <span class="k">上下文长度</span>
            <span class="v mono">{{ observed?.n_ctx ? fmtNum(observed.n_ctx) : ctxRequestedDisplay }}</span>
          </div>
          <div class="kv">
            <span class="k">上下文请求值</span>
            <span class="v mono">{{ ctxRequestedDisplay }}</span>
          </div>
          <div class="kv"><span class="k">KV Token 数</span><span class="v mono">{{ fmtNum(metrics?.kv_cache_tokens) }}</span></div>
          <div class="kv">
            <span class="k">KV 使用率</span>
            <span class="v mono">{{ kvUsageDisplay }}</span>
          </div>
          <div class="kv"><span class="k">K 量化类型</span><span class="v mono">{{ requested?.cache_type_k || "f16（默认）" }}</span></div>
          <div class="kv"><span class="k">V 量化类型</span><span class="v mono">{{ requested?.cache_type_v || "f16（默认）" }}</span></div>
          <div class="kv"><span class="k">统一 KV 缓冲</span><span class="v mono">{{ requested?.kv_unified ? "开" : "关" }}</span></div>
        </div>
      </div>

      <!-- D+E. 系统资源对比（原「运行监控」卡并入：启动前 / 当前 / 变化） -->
      <div class="rt-block span-4">
        <div class="rt-title mon-title">
          <span>系统资源对比</span>
          <t-space size="small">
            <t-tag v-if="baselineLocked" theme="primary" variant="light" size="small">基线已锁定</t-tag>
            <t-tag v-if="running" theme="success" variant="light" size="small">采样中</t-tag>
            <t-button
              variant="text"
              size="small"
              :disabled="running"
              @click="emit('resetBaseline')"
            >
              重置基线
            </t-button>
          </t-space>
        </div>
        <div class="mon-grid mon-head">
          <span>指标</span><span>启动前</span><span>当前</span><span>变化</span>
        </div>
        <div v-for="r in monitorRows" :key="r.label" class="mon-grid mon-row">
          <span class="mon-label">{{ r.label }}</span>
          <span>{{ r.before }}</span>
          <span class="mon-now">{{ r.now }}</span>
          <span :class="r.cls">{{ r.delta }}</span>
        </div>
        <div class="rt-sub" style="margin-top: 6px">每 1s 采样 · {{ gpuHint }}</div>
      </div>

      <!-- F. 运行时配置（请求值 vs 实际值） -->
      <div class="rt-block span-4">
        <div class="rt-title">运行时配置</div>
        <div class="cfg-grid cfg-head">
          <span>项</span><span>请求值</span><span>实际值</span>
        </div>
        <div v-for="r in cfgRows" :key="r.label" class="cfg-grid cfg-row">
          <span class="cfg-label">{{ r.label }}</span>
          <span class="mono">{{ r.requested }}</span>
          <span class="mono" :class="{ 'cfg-effective': r.effective !== '—' }">{{ r.effective }}</span>
        </div>
      </div>

      <!-- G. 运行健康状态 -->
      <div class="rt-block span-4">
        <div class="rt-title">运行健康状态</div>
        <div class="health-list">
          <div v-for="h in healthItems" :key="h.label" class="health-row">
            <span class="health-dot" :class="'dot-' + h.state"></span>
            <span class="health-label">{{ h.label }}</span>
            <span class="health-value">{{ h.value }}</span>
          </div>
        </div>
      </div>

      <!-- H. 运行时间线（最近约 90s 有限窗口） -->
      <div class="rt-block span-4">
        <div class="rt-title">运行时间线 <span class="rt-sub">最近 {{ timeline.length }} 个采样点</span></div>
        <div class="spark-label">生成 TPS</div>
        <svg class="spark" viewBox="0 0 200 36" preserveAspectRatio="none">
          <polyline :points="sparkPoints((p) => p.tps, 0)" fill="none" stroke="#0052d9" stroke-width="1.5" />
        </svg>
        <div class="spark-label">上下文 / KV 占用</div>
        <svg class="spark" viewBox="0 0 200 36" preserveAspectRatio="none">
          <polyline :points="sparkPoints((p) => p.kv != null ? p.kv * 100 : null, 100)" fill="none" stroke="#2ba471" stroke-width="1.5" />
        </svg>
        <div class="rt-sub" style="margin-top: 6px">曲线为最近约 90 秒窗口，服务停止后清空</div>
      </div>

      <!-- 警告 / 错误 -->
      <div class="rt-block span-8">
        <div class="rt-title">警告 / 错误</div>
        <div v-if="diagnostics.length === 0" class="rt-sub">暂无诊断信息</div>
        <div v-else class="diag-list">
          <div v-for="(d, i) in diagnostics.slice(-20).reverse()" :key="d.timestamp_ms + '-' + i" class="diag-row">
            <t-tag :theme="d.level === 'error' ? 'danger' : 'warning'" variant="light" size="small">
              {{ d.level === "error" ? "错误" : "警告" }}
            </t-tag>
            <span class="diag-msg">{{ d.message }}</span>
            <span class="diag-src">{{ d.source === "metrics" ? "metrics" : "log" }} · {{ fmtTime(d.timestamp_ms) }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- Metrics 不可用时的说明（Server 仍保持 Running，不误报为异常） -->
    <div v-if="running && metricsStatus === 'unavailable' && metricsError" class="rt-metrics-error">
      Metrics 不可用：{{ metricsError }}（llama-server 仍在运行；请在启动参数中开启「指标端点 --metrics」）
    </div>
    <div v-else-if="running && metricsStatus === 'connecting'" class="rt-metrics-hint">
      正在等待 /metrics 就绪（模型加载中）…
    </div>
    <div v-else-if="running && metricsStatus === 'disabled'" class="rt-metrics-hint">
      Metrics 未开启：在「启动参数 → 指标端点 (--metrics)」打开后重启服务即可开始采集（运行中打开开关不生效）
    </div>
  </t-card>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { runtimeStore } from "../composables/runtime";

/** ServerView 每秒采样结果（系统 + 进程维度，与 llama.cpp 推理指标是不同数据源，不混用） */
interface StatsSampleLite {
  cpu_percent: number | null;
  ram_used_gb: number | null;
  ram_total_gb: number | null;
  gpu_util_percent: number | null;
  gpu_mem_used_mb: number | null;
  gpu_name: string | null;
  proc_cpu_percent: number | null;
  proc_mem_mb: number | null;
}

const props = defineProps<{
  stats: StatsSampleLite | null;
  /** 原「运行监控」卡并 入的系统资源对比行（启动前 / 当前 / 变化） */
  monitorRows: MonRow[];
  baselineLocked: boolean;
  gpuHint: string;
}>();

const emit = defineEmits<{ (e: "resetBaseline"): void }>();

interface MonRow {
  label: string;
  before: string;
  now: string;
  delta: string;
  cls: string;
}

const server = computed(() => runtimeStore.server);
const running = computed(() => runtimeStore.server.running);
const metrics = computed(() => runtimeStore.metrics);
const observed = computed(() => runtimeStore.observed);
const requested = computed(() => runtimeStore.requested);
const diagnostics = computed(() => runtimeStore.diagnostics);
const timeline = computed(() => runtimeStore.timeline);
const metricsStatus = computed(() => runtimeStore.metricsStatus);
const metricsError = computed(() => runtimeStore.metricsError);

const modelDisplay = computed(() => {
  const m = runtimeStore.server.model;
  if (!m) return "";
  return m.split(/[\\/]/).pop() ?? m;
});

const buildDisplay = computed(() => {
  const o = observed.value;
  if (o?.build_number) return o.build_commit ? `b${o.build_number} (${o.build_commit})` : `b${o.build_number}`;
  return "—";
});

// ---- Uptime：以 runtime:updated 的秒数为基准 + 本地每秒递增，避免 1.5s 步进感 ----
const uptimeBase = ref<{ at: number; seconds: number } | null>(null);
const uptimeTick = ref(0);
let uptimeTimer: ReturnType<typeof setInterval> | undefined;

watch(
  () => runtimeStore.uptimeSeconds,
  (s) => {
    uptimeBase.value = s == null ? null : { at: Date.now(), seconds: s };
  },
  { immediate: true }
);

onMounted(() => {
  uptimeTimer = setInterval(() => {
    uptimeTick.value++;
  }, 1000);
});
onBeforeUnmount(() => {
  if (uptimeTimer) clearInterval(uptimeTimer);
});

const uptimeDisplay = computed(() => {
  void uptimeTick.value;
  const b = uptimeBase.value;
  if (!b) return "—";
  const total = b.seconds + Math.floor((Date.now() - b.at) / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(h)}:${pad(m)}:${pad(s)}`;
});

// ---- Metrics 状态 ----
const metricsLabel = computed(
  () => ({ connected: "已连接", connecting: "等待中", unavailable: "不可用", disabled: "未开启" }[runtimeStore.metricsStatus] ?? runtimeStore.metricsStatus)
);
const metricsTheme = computed<"success" | "warning" | "danger" | "default">(() => {
  const map: Record<string, "success" | "warning" | "danger" | "default"> = {
    connected: "success",
    connecting: "warning",
    unavailable: "danger",
    disabled: "default",
  };
  return map[runtimeStore.metricsStatus] ?? "default";
});

// ---- 格式化 ----
const fmtTps = (v?: number | null) => (v == null ? "—" : v >= 100 ? v.toFixed(0) : v.toFixed(1));
const fmtNum = (v?: number | null) => (v == null ? "—" : Number.isInteger(v) ? String(v) : v.toFixed(0));
const fmtPct = (v?: number | null) => (v == null ? "—" : v.toFixed(0) + "%");
const fmtMb = (v?: number | null) => {
  if (v == null) return "—";
  return v >= 1024 ? (v / 1024).toFixed(2) + " GB" : v.toFixed(0) + " MB";
};
const fmtSecs = (v?: number | null) => {
  if (v == null) return "—";
  return v >= 60 ? `${(v / 60).toFixed(1)} min` : `${v.toFixed(1)} s`;
};
const fmtBytes = (v?: number | null) => {
  if (v == null) return "—";
  return v >= 1073741824 ? (v / 1073741824).toFixed(2) + " GB" : (v / 1048576).toFixed(1) + " MB";
};
const fmtTime = (ms: number) => {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
};

const ctxRequestedDisplay = computed(() => {
  const c = runtimeStore.contextRequested;
  return c != null && c > 0 ? fmtNum(c) : "模型上限";
});

const kvUsageDisplay = computed(() => {
  const r = metrics.value?.kv_cache_usage_ratio;
  return r == null ? "—" : (r * 100).toFixed(1) + "%";
});

// ---- F. Requested vs Effective 配置表 ----
interface CfgRow {
  label: string;
  requested: string;
  effective: string;
}

const cfgRows = computed<CfgRow[]>(() => {
  const req = requested.value ?? {};
  const obs = observed.value;
  const na = "—";
  const show = (reqV: any, effV: string) => ({
    requested: reqV == null || reqV === "" ? na : String(reqV),
    effective: effV,
  });
  return [
    {
      label: "上下文",
      ...show(runtimeStore.contextRequested, obs?.n_ctx != null ? String(obs.n_ctx) : na),
    },
    { label: "批大小 (-b)", ...show(req.batch_size, obs?.n_batch != null ? String(obs.n_batch) : na) },
    { label: "物理批大小 (-ub)", ...show(req.ubatch_size, obs?.n_ubatch != null ? String(obs.n_ubatch) : na) },
    { label: "并行槽位", ...show(req.parallel, obs?.n_parallel != null ? String(obs.n_parallel) : na) },
    {
      label: "GPU 层数",
      requested: req.gpu_layers == null ? na : String(req.gpu_layers),
      effective:
        obs?.n_gpu_layers_offloaded != null
          ? obs.n_gpu_layers_total != null
            ? `${obs.n_gpu_layers_offloaded}/${obs.n_gpu_layers_total}`
            : String(obs.n_gpu_layers_offloaded)
          : na,
    },
    { label: "Flash Attention", ...show(req.flash_attn, req.flash_attn ? String(req.flash_attn) : na) },
    { label: "K 量化类型", ...show(req.cache_type_k, req.cache_type_k ? String(req.cache_type_k) : "f16") },
    { label: "V 量化类型", ...show(req.cache_type_v, req.cache_type_v ? String(req.cache_type_v) : "f16") },
    { label: "禁用 mmap", requested: req.no_mmap ? "开" : na, effective: na },
    { label: "统一 KV 缓冲", requested: req.kv_unified ? "开" : na, effective: na },
  ];
});

// ---- G. Runtime Health ----
interface HealthItem {
  label: string;
  value: string;
  state: "ok" | "warn" | "err" | "off";
}

const healthItems = computed<HealthItem[]>(() => {
  const st = runtimeStore.metricsStatus;
  const items: HealthItem[] = [
    {
      label: "指标端点",
      value: { connected: "已连接", connecting: "等待中", unavailable: "不可用", disabled: "未开启" }[st] ?? st,
      state: st === "connected" ? "ok" : st === "connecting" ? "warn" : st === "unavailable" ? "err" : "off",
    },
    {
      label: "HTTP API",
      value: st === "connected" ? "正常" : running.value ? "等待确认" : "—",
      state: st === "connected" ? "ok" : running.value ? "warn" : "off",
    },
    { label: "日志", value: running.value ? "接收中" : "—", state: running.value ? "ok" : "off" },
    {
      label: "模型",
      value: observed.value?.model_path ? "已加载" : running.value ? "加载中" : "—",
      state: observed.value?.model_path ? "ok" : running.value ? "warn" : "off",
    },
    {
      label: "GPU",
      value: props.stats?.gpu_name ? "可用" : "无数据源",
      state: props.stats?.gpu_name ? "ok" : "off",
    },
    {
      label: "内存",
      value: props.stats?.proc_mem_mb != null ? "正常" : "—",
      state: props.stats?.proc_mem_mb != null ? "ok" : "off",
    },
  ];
  return items;
});

// ---- H. Sparkline：映射 timeline 到 polyline points ----
function sparkPoints(getter: (p: { tps: number | null; kv: number | null }) => number | null, max: number): string {
  const pts = timeline.value;
  if (pts.length < 2) return "";
  const vals = pts.map(getter).map((v) => (v == null ? null : Math.min(v, max)));
  const hasAny = vals.some((v) => v != null);
  if (!hasAny) return "";
  const step = 200 / (pts.length - 1);
  return vals
    .map((v, i) => (v == null ? null : `${(i * step).toFixed(1)},${(34 - (v / max) * 32).toFixed(1)}`))
    .filter(Boolean)
    .join(" ");
}
</script>

<style scoped>
.rt-card :deep(.t-card__body) {
  padding-top: 12px;
}
.rt-grid {
  display: grid;
  grid-template-columns: repeat(12, 1fr);
  gap: 18px 24px;
}
.span-3 { grid-column: span 3; }
.span-4 { grid-column: span 4; }
.span-5 { grid-column: span 5; }
.span-8 { grid-column: span 8; }
.span-12 { grid-column: span 12; }

.mon-title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

/* 系统资源对比表（原「运行监控」卡样式迁入） */
.mon-grid {
  display: grid;
  grid-template-columns: 1.2fr 1fr 1fr 0.9fr;
  gap: 3px 8px;
  font-size: 12.5px;
  line-height: 1.6;
}
.mon-head {
  color: var(--td-text-color-placeholder, #999);
  font-size: 11.5px;
  border-bottom: 1px solid var(--td-component-stroke, #e7e7e7);
  padding-bottom: 3px;
  margin-bottom: 3px;
}
.mon-label { color: var(--td-text-color-secondary, #666); }
.mon-now { font-weight: 600; }
:deep(.delta-up) { color: #d54941; }
:deep(.delta-down) { color: #2ba471; }

.rt-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--td-text-color-secondary, #666);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  margin-bottom: 8px;
}
.rt-sub {
  font-weight: 400;
  color: var(--td-text-color-placeholder, #999);
  text-transform: none;
  letter-spacing: 0;
}
.rt-model {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 8px;
  word-break: break-all;
}
.mono {
  font-family: Consolas, "JetBrains Mono", "Courier New", monospace;
  font-size: 12.5px;
}

.kv-list {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.kv {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  font-size: 12.5px;
  line-height: 1.5;
}
.kv .k {
  color: var(--td-text-color-secondary, #666);
  flex: none;
}
.kv .v {
  text-align: right;
  word-break: break-all;
  color: var(--td-text-color-primary, #333);
}

/* 性能区：TPS 大数字 */
.tps-row {
  display: flex;
  gap: 24px;
  margin-bottom: 10px;
}
.tps-num {
  font-family: Consolas, "JetBrains Mono", monospace;
  font-size: 30px;
  font-weight: 700;
  line-height: 1.1;
}
.accent-prompt { color: var(--td-brand-color, #0052d9); }
.accent-gen { color: #2ba471; }
.tps-label {
  font-size: 12px;
  color: var(--td-text-color-placeholder, #999);
  margin-top: 2px;
}

/* 配置对比表 */
.cfg-grid {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: 4px 10px;
  font-size: 12.5px;
}
.cfg-head {
  color: var(--td-text-color-placeholder, #999);
  font-size: 11.5px;
  border-bottom: 1px solid var(--td-component-stroke, #e7e7e7);
  padding-bottom: 4px;
  margin-bottom: 4px;
}
.cfg-row span:first-child {
  color: var(--td-text-color-secondary, #666);
}
.cfg-effective {
  font-weight: 600;
}

/* 健康状态 */
.health-list {
  display: flex;
  flex-direction: column;
  gap: 7px;
}
.health-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
}
.health-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex: none;
}
.dot-ok { background: #2ba471; }
.dot-warn { background: #e37318; }
.dot-err { background: #d54941; }
.dot-off { background: #c0c4cc; }
.health-label {
  width: 72px;
  color: var(--td-text-color-secondary, #666);
}
.health-value {
  color: var(--td-text-color-primary, #333);
}

/* Timeline sparkline */
.spark {
  width: 100%;
  height: 36px;
  display: block;
  background: var(--td-bg-color-secondarycontainer, #f3f3f3);
  border-radius: 4px;
  margin-bottom: 6px;
}
.spark-label {
  font-size: 11.5px;
  color: var(--td-text-color-placeholder, #999);
  margin-bottom: 2px;
}

/* 诊断列表 */
.diag-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 180px;
  overflow-y: auto;
}
.diag-row {
  display: flex;
  align-items: baseline;
  gap: 10px;
  font-size: 12.5px;
}
.diag-msg {
  flex: 1;
  word-break: break-all;
  color: var(--td-text-color-primary, #333);
}
.diag-src {
  flex: none;
  font-size: 11.5px;
  color: var(--td-text-color-placeholder, #999);
  font-family: Consolas, monospace;
}

.rt-metrics-error,
.rt-metrics-hint {
  margin-top: 12px;
  padding: 8px 12px;
  border-radius: 6px;
  font-size: 12.5px;
}
.rt-metrics-error {
  background: var(--td-error-color-1, #fff0ed);
  color: var(--td-error-color, #d54941);
}
.rt-metrics-hint {
  background: var(--td-warning-color-1, #fff3e8);
  color: var(--td-warning-color, #e37318);
}

@media (max-width: 1100px) {
  .span-3, .span-4, .span-5 { grid-column: span 6; }
  .span-8, .span-12 { grid-column: span 12; }
}
@media (max-width: 720px) {
  .span-3, .span-4, .span-5, .span-8 { grid-column: span 12; }
}
</style>
