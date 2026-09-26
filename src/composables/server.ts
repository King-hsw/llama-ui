import { reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface ServerStatus {
  running: boolean;
  pid: number | null;
  model: string | null;
  port: number | null;
}

export interface ModelInfo {
  name: string;
  path: string;
  size: number;
}

export interface Settings {
  /** 已废弃：v1 单一安装目录，仅兼容旧配置保留 */
  install_dir: string;
  models_dir: string;
  current_tag: string | null;
  mirror: string;
  /** 程序来源："custom"（用户自备目录，只读）| "managed"（托管下载，独占写入） */
  exe_source: string;
  /** 场景一：本地 llama.cpp 根目录（含 llama-server.exe），程序只读 */
  custom_dir: string;
  /** 场景二：托管下载根目录，无默认值，首次使用必须显式指定 */
  managed_dir: string;
  /** 自动检查 llama.cpp 更新：开启后应用启动时检查并弹窗提示新版本 */
  auto_check_update: boolean;
}

/** resolve_server_exe 的返回：解析结果 + 来源标签 + 引导信息 */
export interface ResolvedExe {
  path: string | null;
  source: "custom" | "managed";
  tag: string | null;
  message: string;
}

/** 全局共享状态：服务状态 + 选中的模型 */
export const serverState = reactive<ServerStatus>({
  running: false,
  pid: null,
  model: null,
  port: null,
});

export const appStore = reactive({
  selectedModel: "",
  settings: null as Settings | null,
  logs: [] as string[],
});

let inited = false;

/** 日志批量缓冲：llama-server 启动会瞬间吐几百上千行，逐行 push 会引发渲染风暴卡死页面 */
let logBuffer: string[] = [];
let logFlushTimer: ReturnType<typeof setTimeout> | undefined;

function flushLogs() {
  logFlushTimer = undefined;
  if (logBuffer.length === 0) return;
  const batch = logBuffer;
  logBuffer = [];
  appStore.logs.push(...batch);
  if (appStore.logs.length > 2000) {
    appStore.logs.splice(0, appStore.logs.length - 2000);
  }
}

export async function initServerListener() {
  if (inited) return;
  inited = true;
  await listen<string>("server-log", (e) => {
    logBuffer.push(e.payload);
    if (logFlushTimer) return;
    logFlushTimer = setTimeout(flushLogs, 250); // 250ms 内的日志合并成一次响应式更新
  });
  await listen<ServerStatus>("server-status", (e) => {
    Object.assign(serverState, e.payload);
  });
  const s = await invoke<ServerStatus>("get_server_status");
  Object.assign(serverState, s);
}

export function formatSize(bytes: number): string {
  if (bytes >= 1024 ** 3) return (bytes / 1024 ** 3).toFixed(2) + " GB";
  if (bytes >= 1024 ** 2) return (bytes / 1024 ** 2).toFixed(1) + " MB";
  return (bytes / 1024).toFixed(0) + " KB";
}
