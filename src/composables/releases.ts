import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

export interface AssetInfo {
  name: string;
  url: string;
  edition: string;
  arch: string;
  cuda_major: number | null;
  size: number;
}

export interface ReleaseInfo {
  tag: string;
  date: string;
  assets: AssetInfo[];
}

/**
 * llama.cpp 版本列表的模块级单例状态：
 * 路由切换会销毁重建 DownloadView，组件内的 ref 无法跨页面保留，
 * 因此列表数据放在这里 —— 成功拉取过一次后，再次进入菜单直接复用，不重复请求。
 */
export const releases = ref<ReleaseInfo[]>([]);
export const loadingReleases = ref(false);
/** 最近一次拉取失败的原因（成功后清空），供页面展示错误提示 */
export const loadError = ref("");

/** 仅在成功拉取后置 true；失败后下次进入菜单允许自动重试 */
let fetched = false;
/** 进行中的请求共享同一个 Promise，避免自动拉取与手动刷新并发时重复请求 */
let inflight: Promise<void> | null = null;

async function fetchOnce(): Promise<void> {
  loadError.value = "";
  try {
    releases.value = await invoke<ReleaseInfo[]>("list_releases");
    fetched = true;
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loadingReleases.value = false;
    inflight = null;
  }
}

/** 手动刷新：无论之前是否拉取过都重新请求 GitHub */
export function refreshReleases(): Promise<void> {
  if (inflight) return inflight;
  loadingReleases.value = true;
  inflight = fetchOnce();
  return inflight;
}

/** 进入页面时的自动拉取：已成功拉取过或正在拉取则直接复用，不发起请求 */
export function ensureReleases(): Promise<void> {
  if (fetched || inflight) return Promise.resolve();
  return refreshReleases();
}
