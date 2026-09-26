<template>
  <div class="page">
    <!-- 托管目录未指定：下载功能不可用，引导先去设置（旧版 install_dir 已废弃，需重新下载） -->
    <t-alert v-if="!managedReady" theme="warning" class="mb-16">
      <template #message>
        尚未指定托管下载目录，无法下载。请先到「设置 → 程序来源」指定专用目录（旧版「安装目录」配置已废弃，需重新下载 llama.cpp）。
      </template>
    </t-alert>
    <t-card title="llama.cpp 版本" class="mb-16">
      <t-space direction="vertical" style="width: 100%">
        <t-space break-line>
          <t-tag v-if="currentTag" theme="success" variant="light">
            当前已安装: {{ currentTag }}
          </t-tag>
          <t-tag v-else theme="warning" variant="light">尚未安装（自带本地 llama.cpp 可在「设置 → 程序来源」切换为自定义模式）</t-tag>
          <t-button variant="outline" :loading="loadingReleases" @click="manualRefresh">
            {{ releases.length ? "刷新版本列表" : "从 GitHub 获取版本列表" }}
          </t-button>
        </t-space>
        <!-- 拉取失败提示（保留按钮可手动重试） -->
        <t-alert v-if="loadError" theme="error">
          <template #message>
            获取版本列表失败：{{ loadError }}。可点击上方按钮重试。
          </template>
        </t-alert>
        <!-- 本机检测结果与推荐版本 -->
        <t-alert v-if="rec" theme="success">
          <template #message>
            已自动匹配推荐版本：<b>[{{ rec.edition }}]</b> · {{ rec.reason }}
          </template>
        </t-alert>
        <t-alert v-else theme="info">正在检测本机环境（显卡 / CUDA）…</t-alert>
        <t-alert theme="info">
          安装 CUDA 版时会自动匹配并下载同版本的 cudart 附加包（含 CUDA Runtime / cuBLAS DLL），一并解压到安装目录，无需手动处理。
        </t-alert>
      </t-space>
    </t-card>

    <t-card v-if="installing || downloadError" title="下载进度" class="mb-16">
      <t-progress
        :percentage="progress"
        :status="downloadError ? 'error' : progress >= 100 ? 'success' : 'active'"
      />
      <div
        style="margin-top: 8px; display: flex; justify-content: space-between; align-items: center; gap: 12px"
      >
        <div class="mono">
          {{ progressMsg }}
          <span v-if="installing && speed > 0" style="color: var(--td-brand-color, #0052d9)">
            ｜{{ speed.toFixed(1) }} MB/s
          </span>
        </div>
        <t-button
          v-if="installing"
          theme="danger"
          variant="outline"
          size="small"
          @click="onCancelClick"
        >
          取消下载
        </t-button>
      </div>
      <t-alert v-if="downloadError" theme="error" style="margin-top: 8px">
        <template #message>{{ downloadError }}。可重新点击「下载并安装」重试。</template>
      </t-alert>
    </t-card>

    <t-card v-for="r in releases" :key="r.tag" :title="r.tag" class="mb-16">
      <template #actions>
        <t-tag v-if="r.tag === currentTag" theme="success" variant="outline">当前版本</t-tag>
        <t-tag v-else-if="latestTag === r.tag" theme="primary" variant="outline">最新版</t-tag>
        <span class="mono" style="color: var(--td-text-color-placeholder, #999)">{{ r.date }}</span>
      </template>
      <t-space direction="vertical" style="width: 100%">
        <t-select v-model="assetPick[r.tag]" placeholder="选择 Windows x64 预编译包" style="width: min(640px, 100%)">
          <t-option
            v-for="a in mainAssets(r)"
            :key="a.url"
            :value="a.url"
            :label="`[${assetLabel(a)}]${isRecommended(a) ? ' ★ 推荐' : ''} ${a.name} (${formatSize(a.size)})`"
          />
        </t-select>
        <t-space break-line>
          <t-button
            theme="primary"
            :disabled="installing || !managedReady || !assetPick[r.tag]"
            @click="onDownloadClick(r.tag, assetPick[r.tag])"
          >
            下载并安装
          </t-button>
          <t-tag v-if="pickedEdition(r.tag) === 'CUDA'" theme="warning" variant="light">
            CUDA 版将自动下载匹配的 cudart 附加包
          </t-tag>
          <t-tag v-if="pickIsMismatched(r.tag)" theme="danger" variant="light">
            与本机推荐配置不符
          </t-tag>
        </t-space>
      </t-space>
    </t-card>

    <t-empty
      v-if="!loadingReleases && !loadError && releases.length === 0"
      description="点击上方按钮获取 llama.cpp Release 列表"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, h, onMounted, reactive, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { DialogPlugin, MessagePlugin } from "tdesign-vue-next";
import { appStore, formatSize } from "../composables/server";
import {
  cancelDownload,
  cancelDownloadState,
  downloadError,
  finishedTag,
  installing,
  progress,
  progressMsg,
  speed,
  startDownload,
} from "../composables/download";
import {
  ensureReleases,
  loadError,
  loadingReleases,
  refreshReleases,
  releases,
  type AssetInfo,
} from "../composables/releases";

interface Recommendation {
  edition: string;
  arch: string;
  cuda_major: number | null;
  reason: string;
}

const currentTag = computed(() => appStore.settings?.current_tag || "");
const latestTag = computed(() => releases.value[0]?.tag || "");
/** 托管下载目录是否已指定（未指定时禁止下载，提示先去设置） */
const managedReady = computed(() => !!appStore.settings?.managed_dir);
// installing / progress / progressMsg / downloadError 来自 composables/download（模块级单例），
// 路由切换销毁本组件后状态不丢，切走再回来进度条仍在
const assetPick = reactive<Record<string, string>>({});
const rec = ref<Recommendation | null>(null);

/** 从文件名提取完整 CUDA 版本号（如 "12.4"），与后端 cuda_full_version 逻辑一致 */
function cudaVersionOf(name: string): string {
  const lower = name.toLowerCase();
  const idx = lower.indexOf("cuda");
  if (idx < 0) return "";
  const tail = name.slice(idx + 4).replace(/^[^0-9]*/, "");
  return tail.match(/^[\d.]+/)?.[0] || "";
}

/** 在同一 Release 中查找与 CUDA 主包版本、架构完全一致的 cudart 附加包 */
function findCudart(tag: string, asset: AssetInfo): AssetInfo | null {
  const rel = releases.value.find((r) => r.tag === tag);
  if (!rel) return null;
  const ver = cudaVersionOf(asset.name);
  return (
    rel.assets.find(
      (a) =>
        a.edition === "CUDA-cudart" &&
        a.arch === asset.arch &&
        cudaVersionOf(a.name) === ver
    ) || null
  );
}

/** 可手动选择的安装包：cudart 附加包随 CUDA 主包自动下载，不单独列出 */
function mainAssets(r: { assets: AssetInfo[] }): AssetInfo[] {
  return r.assets.filter((a) => a.edition !== "CUDA-cudart");
}

/** 展示标签：CUDA 包带大版本，ARM 包额外标注 */
function assetLabel(a: AssetInfo): string {
  let label = a.edition;
  if (a.edition.startsWith("CUDA") && a.cuda_major != null) label += ` ${a.cuda_major}`;
  if (a.arch === "arm64") label += " · ARM64";
  return label;
}

/** 是否与本机推荐完全匹配（后端 + 架构 + CUDA 大版本） */
function isRecommended(a: AssetInfo): boolean {
  return mismatchReasons(a).length === 0;
}

/** 与推荐配置的不匹配点（空数组 = 完全匹配） */
function mismatchReasons(a: AssetInfo): string[] {
  if (!rec.value) return [];
  const rs: string[] = [];
  if (a.edition !== rec.value.edition) {
    rs.push(`推理后端不符（推荐 ${rec.value.edition}，当前 ${a.edition}）`);
  }
  if (a.arch !== rec.value.arch) {
    rs.push(`CPU 架构不符（本机 ${rec.value.arch}，安装包为 ${a.arch}），可能无法运行`);
  }
  if (
    a.edition === "CUDA" &&
    rec.value.edition === "CUDA" &&
    rec.value.cuda_major != null &&
    a.cuda_major != null &&
    a.cuda_major !== rec.value.cuda_major
  ) {
    rs.push(
      `CUDA 大版本不符（本机驱动支持 CUDA ${rec.value.cuda_major}，安装包为 CUDA ${a.cuda_major}），启动可能报缺少 cudart DLL`
    );
  }
  return rs;
}

function pickedEdition(tag: string): string {
  const url = assetPick[tag];
  if (!url) return "";
  return releases.value.find((r) => r.tag === tag)?.assets.find((a) => a.url === url)?.edition || "";
}

/** 当前选中的安装包是否与本机推荐不符（用于卡片上的警示 tag） */
function pickIsMismatched(tag: string): boolean {
  const url = assetPick[tag];
  if (!url || !rec.value) return false;
  const asset = releases.value.find((r) => r.tag === tag)?.assets.find((a) => a.url === url);
  return !!asset && mismatchReasons(asset).length > 0;
}

async function loadSettings() {
  appStore.settings = await invoke("get_settings");
}

/** 为每个版本自动选中推荐安装包（不覆盖用户已手动选择的项） */
function autoPick() {
  for (const r of releases.value) {
    const mains = mainAssets(r);
    if (mains.length > 0 && !assetPick[r.tag]) {
      // 自动匹配：完全匹配 → 同后端同架构 → 同后端 → vulkan / cpu 兜底
      const exact = rec.value ? mains.find((a) => isRecommended(a)) : null;
      const sameArch = rec.value
        ? mains.find((a) => a.edition === rec.value!.edition && a.arch === rec.value!.arch)
        : null;
      const sameEdition = rec.value
        ? mains.find((a) => a.edition === rec.value!.edition)
        : null;
      const vulkan = mains.find((a) => a.edition === "Vulkan");
      const cpu = mains.find((a) => a.edition === "CPU");
      assetPick[r.tag] = (exact || sameArch || sameEdition || vulkan || cpu || mains[0]).url;
    }
  }
}

// 列表数据与本机推荐结果哪个先到都可能，双源监听后自动选中
watch([releases, rec], autoPick);

// 拉取失败统一提示（composable 内不弹 UI，避免共享 Promise 时重复弹 toast）
watch(loadError, (v) => {
  if (v) MessagePlugin.error(`获取版本列表失败：${v}`);
});

/** 手动点「刷新版本列表」按钮：强制重新拉取 */
function manualRefresh() {
  refreshReleases();
}

// ---------- 非推荐安装包提醒 ----------

function riskText(chosen: string): string {
  switch (chosen) {
    case "CUDA":
      return "CUDA 版要求 NVIDIA 显卡并安装较新驱动；若本机没有 NVIDIA GPU，llama-server 将无法启动，且安装包体积明显更大。";
    case "Vulkan":
      return "Vulkan 版依赖显卡的 Vulkan 支持；在无独立显卡或驱动过旧的机器上可能无法使用，性能也可能低于专用后端。";
    case "CPU":
      return "CPU 版不使用 GPU 加速，生成速度明显低于 GPU 版本，模型越大差距越明显。";
    default:
      return "所选版本与本机硬件环境可能不匹配，存在启动失败或性能不佳的风险。";
  }
}

function onDownloadClick(tag: string, url: string) {
  const asset = releases.value
    .find((r) => r.tag === tag)
    ?.assets.find((a) => a.url === url);
  if (!asset) return;

  // 与本机推荐配置不符（后端 / 架构 / CUDA 大版本）→ 弹窗说明原因与风险，确认后继续
  const reasons = mismatchReasons(asset);
  if (rec.value && reasons.length > 0) {
    const chosen = asset.edition;
    const reason = rec.value.reason;
    const recLabel = assetLabel({
      name: "",
      url: "",
      edition: rec.value.edition,
      arch: rec.value.arch,
      cuda_major: rec.value.cuda_major,
      size: 0,
    });
    const dlg = DialogPlugin.confirm({
      header: "安装包与本机环境不符",
      body: () =>
        h("div", { style: "display:flex;flex-direction:column;gap:8px;font-size:14px;line-height:1.6" }, [
          h("div", `本机检测结果：${reason}`),
          h("div", [
            h("span", "推荐安装 "),
            h("b", `[${recLabel}]`),
            h("span", " 版本，而当前选择的是 "),
            h("b", { style: "color:#d54941" }, `[${assetLabel(asset)}]`),
            h("span", " 版本。"),
          ]),
          h(
            "div",
            { style: "color:#e37318" },
            `不匹配项：${reasons.join("；")}。`
          ),
          h("div", { style: "color:#e37318" }, `潜在风险：${riskText(chosen)}`),
          h("div", { style: "color:var(--td-text-color-placeholder,#999);font-size:12px" },
            "如确有需要（例如自行测试），确认了解风险后可继续安装。"),
        ]),
      confirmBtn: { content: "仍要安装", theme: "warning" },
      cancelBtn: "取消",
      onConfirm: () => {
        dlg.destroy();
        download(tag, asset);
      },
    });
    return;
  }
  download(tag, asset);
}

// 后端 stage=error 事件统一在此弹 toast（成功/失败还会经全局悬浮窗与进度卡片展示）
watch(downloadError, (v) => {
  if (v) MessagePlugin.error(v);
});

/** 取消下载：后端中断下载/解压并清理临时文件，前端状态由 cancelled 事件复位 */
async function onCancelClick() {
  try {
    await cancelDownload();
  } catch (e) {
    MessagePlugin.error(String(e));
  }
}

// 安装成功（stage=done）：弹提示 + 刷新 current_tag。
// 注意不能放在 invoke 之后 —— download_release 启动线程即返回，那时下载才刚开始
let lastDownloadWasCuda = false;
watch(finishedTag, (tag) => {
  if (!tag) return;
  MessagePlugin.success(
    lastDownloadWasCuda
      ? `已安装 ${tag}（已一并下载并解压匹配的 cudart 附加包）`
      : `已安装 ${tag}`
  );
  loadSettings();
});

async function download(tag: string, asset: AssetInfo) {
  // CUDA 主包自动匹配同版本 cudart 附加包，匹配不到则明确报错终止
  let cudart: AssetInfo | null = null;
  if (asset.edition === "CUDA") {
    cudart = findCudart(tag, asset);
    if (!cudart) {
      const ver = cudaVersionOf(asset.name);
      MessagePlugin.error(
        `未能在 ${tag} 中找到与 CUDA ${ver || "?"}（${asset.arch}）匹配的 cudart 附加包，已取消安装。` +
          `请到 GitHub Release 页面确认该版本是否提供 cudart-llama-bin-win-cuda-${ver || "?"}-${asset.arch}.zip。`,
        8000
      );
      return;
    }
  }
  lastDownloadWasCuda = asset.edition === "CUDA";
  startDownload(tag);
  try {
    await invoke("download_release", {
      tag,
      name: asset.name,
      url: asset.url,
      cudartName: cudart?.name ?? null,
      cudartUrl: cudart?.url ?? null,
    });
    // 任务已成功启动；成功/失败与状态复位均由 download-progress 事件（done/error）驱动
  } catch (e) {
    // 后端拒绝（重入等），不会有任何进度事件，必须手动复位，否则按钮永久禁用
    cancelDownloadState();
    const msg = String(e);
    if (msg.includes("已有下载任务进行中")) {
      MessagePlugin.warning("已有下载任务进行中，可在本页进度卡片或右下角悬浮窗查看进度");
    } else {
      MessagePlugin.error(msg);
    }
  }
}

onMounted(async () => {
  await loadSettings();
  // 先检测本机环境（NVIDIA / 其他 GPU / 纯 CPU）
  rec.value = await invoke<Recommendation>("get_recommended_edition").catch(() => null);
  // 后台异步拉取版本列表：已成功拉取过则直接复用缓存，不重复请求；
  // 不 await，页面先渲染，列表数据到达后由 watch 触发自动选中并渲染
  ensureReleases();
  // download-progress 事件由 composables/download 全局监听（App 启动时注册一次），
  // 组件内不再重复监听，避免路由往返时丢失进度状态
});
</script>
