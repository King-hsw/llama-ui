import { invoke } from "@tauri-apps/api/core";
import { DialogPlugin, MessagePlugin } from "tdesign-vue-next";
import router from "../router";

export interface UpdateCheck {
  has_update: boolean;
  latest_tag: string;
  current_tag: string | null;
  /** 最新版发布日期（YYYY-MM-DD） */
  date: string;
}

/** 应用本次启动是否已弹过更新提示（自动检查只提示一次，避免每次切页重复打扰） */
let autoPrompted = false;

/**
 * 检查 llama.cpp 更新：
 * - manual=true（设置页手动触发）：无更新提示"已是最新"，失败弹错误；
 * - manual=false（应用启动自动检查）：静默失败，只在发现新版本时弹一次确认框。
 * 弹窗确认后跳转「下载与更新」页——安装包需按本机硬件选择后端/架构，
 * 沿用下载页的推荐选中 + 风险确认流程，不在弹窗里直接开始下载。
 */
export async function runUpdateCheck(manual = false): Promise<void> {
  let r: UpdateCheck;
  try {
    r = await invoke<UpdateCheck>("check_update");
  } catch (e) {
    if (manual) MessagePlugin.error(`检查更新失败：${e}`);
    return;
  }
  if (!r.has_update) {
    if (manual) MessagePlugin.success("当前已是最新版本");
    return;
  }
  if (!manual && autoPrompted) return;
  autoPrompted = true;
  const dlg = DialogPlugin.confirm({
    header: "发现 llama.cpp 新版本",
    body: `最新版本 ${r.latest_tag}（发布于 ${r.date}），当前已安装 ${
      r.current_tag ?? "无"
    }。是否前往更新？`,
    confirmBtn: { content: "前往更新", theme: "primary" },
    cancelBtn: "暂不更新",
    onConfirm: () => {
      dlg.destroy();
      router.push("/download");
    },
  });
}
