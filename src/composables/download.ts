import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface DownloadProgressPayload {
  stage: string;
  percent: number;
  message: string;
  /** 实时下载速度（MB/s），仅下载阶段携带 */
  speed_mb_s?: number;
}

/**
 * 下载任务的模块级单例状态：
 * 1. 路由切换会销毁重建 DownloadView，组件内的 ref 无法跨页面保留，
 *    导致下载中切走再回来进度条消失（后端实际仍在下载），故状态放在模块级。
 * 2. download_release 是"启动即返回"的命令（后端 spawn 线程后立即 Ok），
 *    invoke 的 resolve 不代表下载完成 —— installing / finishedTag / downloadError
 *    必须由 download-progress 事件（stage=done/error）驱动，不能用 finally 复位。
 */
export const installing = ref(false);
export const progress = ref(0);
export const progressMsg = ref("");
/** 实时下载速度（MB/s），非下载阶段为 0 */
export const speed = ref(0);
/** 最近一次后端报错（stage=error 时写入，"done" 时清空），供页面持久展示 */
export const downloadError = ref("");
/** 下载成功完成的版本 tag（startDownload 时清空），供页面弹成功提示、刷新设置 */
export const finishedTag = ref("");

let listened = false;
/** 进行中任务的 tag，done 事件到达时写入 finishedTag */
let pendingTag = "";

/** 全局注册一次 download-progress 监听（App 启动时调用，幂等） */
export function initDownloadListener() {
  if (listened) return;
  listened = true;
  listen<DownloadProgressPayload>("download-progress", (e) => {
    const { stage, percent, message, speed_mb_s } = e.payload ?? {};
    progress.value = Math.floor(percent ?? 0);
    progressMsg.value = message ?? "";
    speed.value = speed_mb_s ?? 0;
    if (stage === "done") {
      installing.value = false;
      downloadError.value = "";
      speed.value = 0;
      finishedTag.value = pendingTag;
    } else if (stage === "error") {
      installing.value = false;
      speed.value = 0;
      downloadError.value = (message ?? "下载失败").replace(/^失败: /, "");
    } else if (stage === "cancelled") {
      // 用户主动取消：不算错误，不弹 toast，卡片随 installing=false 消失
      installing.value = false;
      downloadError.value = "";
      speed.value = 0;
    }
  });
}

/** 发起下载前重置状态并记录任务 tag */
export function startDownload(tag: string) {
  installing.value = true;
  progress.value = 0;
  progressMsg.value = "开始下载…";
  downloadError.value = "";
  finishedTag.value = "";
  speed.value = 0;
  pendingTag = tag;
}

/** 请求取消当前下载：后端置位取消标记，循环轮询到后中断并清理临时文件 */
export function cancelDownload(): Promise<void> {
  progressMsg.value = "正在取消…";
  return invoke("cancel_download");
}

/**
 * 后端拒绝下载（重入等）时手动复位：这种情况不会有任何进度事件来复位状态，
 * 不复位会导致 installing 永远为 true、按钮永久禁用。
 */
export function cancelDownloadState() {
  installing.value = false;
}
