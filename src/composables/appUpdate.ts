import { ref } from "vue";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

/**
 * 应用自更新（区别于 llama.cpp 的更新检查）：
 * 走 tauri-plugin-updater，检查 tauri.conf.json 中配置的 endpoint
 * （GitHub Releases 上的 latest.json），验签通过后静默安装并重启。
 */
export function useAppUpdate() {
  const checking = ref(false);
  const downloading = ref(false);
  /** 当前应用版本（tauri.conf.json 中的 version） */
  const version = ref("");
  /** 是否有可用更新 */
  const available = ref(false);
  const newVersion = ref("");
  /** 新版本发布说明（release body） */
  const notes = ref("");
  const downloadedMb = ref(0);
  const totalMb = ref(0);

  // check() 返回的 Update 对象持有下载句柄，install 时复用
  let pending: Update | null = null;

  async function init() {
    version.value = await getVersion();
  }

  /** 检查更新，返回是否有新版本。失败时向上抛错，由调用方展示 */
  async function checkForUpdate(): Promise<boolean> {
    checking.value = true;
    try {
      pending = (await check()) ?? null;
      available.value = !!pending;
      newVersion.value = pending?.version ?? "";
      notes.value = pending?.body ?? "";
      return available.value;
    } finally {
      checking.value = false;
    }
  }

  /** 下载并静默安装，完成后自动重启应用（Windows 上 NSIS 安装器会接管关闭旧进程） */
  async function install() {
    if (!pending) return;
    downloading.value = true;
    downloadedMb.value = 0;
    totalMb.value = 0;
    try {
      await pending.downloadAndInstall((event) => {
        if (event.event === "Started") {
          totalMb.value = event.data.contentLength
            ? event.data.contentLength / 1048576
            : 0;
        } else if (event.event === "Progress") {
          downloadedMb.value += event.data.chunkLength / 1048576;
        } else if (event.event === "Finished") {
          downloadedMb.value = totalMb.value;
        }
      });
      await relaunch();
    } finally {
      downloading.value = false;
    }
  }

  return {
    checking,
    downloading,
    version,
    available,
    newVersion,
    notes,
    downloadedMb,
    totalMb,
    init,
    checkForUpdate,
    install,
  };
}
