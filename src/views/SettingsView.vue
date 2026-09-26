<template>
  <div class="page">
    <t-card title="应用设置" class="mb-16">
      <!-- 旧版单一安装目录迁移提示：直接切 managed，旧目录不再使用 -->
      <t-alert v-if="settings.install_dir && !settings.custom_dir && !settings.managed_dir" theme="warning" class="mb-16">
        <template #message>
          检测到旧版「安装目录」配置（{{ settings.install_dir }}），该字段已废弃、不再使用。
          请在下方选择程序来源：托管下载需指定新目录并重新下载 llama.cpp；本地已有 llama.cpp 可切换为自定义模式。
        </template>
      </t-alert>

      <t-form label-align="left" label-width="160px">
        <t-form-item label="程序来源">
          <t-radio-group v-model="settings.exe_source">
            <t-radio value="managed">托管下载（程序自行下载、升级、管理版本）</t-radio>
            <t-radio value="custom">自定义（使用本地已安装的 llama.cpp，程序只读）</t-radio>
          </t-radio-group>
        </t-form-item>

        <t-form-item v-if="settings.exe_source === 'managed'" label="托管下载目录" required-mark>
          <t-space break-line>
            <t-input
              v-model="settings.managed_dir"
              style="width: min(440px, 100%)"
              class="mono"
              placeholder="必填：所有下载 / 解压 / 版本目录仅存放在此目录内"
            />
            <t-button variant="outline" @click="pickManagedDir">浏览</t-button>
          </t-space>
          <template #help>
            建议使用专用空目录（如 D:\llama-ui\runtimes）。版本以「目录名\版本tag」形式存放，互不干扰；
            不会写入或修改自定义目录与系统其他位置。
          </template>
        </t-form-item>

        <t-form-item v-else label="本地 llama.cpp 根目录" required-mark>
          <t-space break-line>
            <t-input
              v-model="settings.custom_dir"
              style="width: min(440px, 100%)"
              class="mono"
              placeholder="必填：包含 llama-server.exe 的根目录，程序对其只读"
            />
            <t-button variant="outline" @click="pickCustomDir">浏览</t-button>
          </t-space>
          <template #help>
            程序仅读取该目录用于启动，绝不下载、解压或修改其中任何文件；
            托管下载的版本存放在独立的托管目录中，互不影响。
          </template>
        </t-form-item>

        <t-form-item label="模型目录 (GGUF)">
          <t-space break-line>
            <t-input v-model="settings.models_dir" style="width: min(440px, 100%)" class="mono" placeholder="存放 .gguf 模型文件的目录" />
            <t-button variant="outline" @click="pickModels">浏览</t-button>
          </t-space>
        </t-form-item>
        <t-form-item label="下载镜像前缀">
          <t-input
            v-model="settings.mirror"
            style="width: min(440px, 100%)"
            class="mono"
            placeholder="直连 GitHub，如慢可填 https://gh-proxy.com/"
          />
          <template #help>
            拼接规则：镜像 + / + 完整 GitHub 链接（https://gh-proxy.com/https://github.com/...）。
            末尾有无斜杠均可，不带协议会自动补 https://，留空则直连。仅对托管下载生效。
          </template>
        </t-form-item>
        <t-form-item label=" ">
          <t-button theme="primary" @click="save">保存设置</t-button>
        </t-form-item>
      </t-form>
    </t-card>

    <t-card title="更新检查" class="mb-16">
      <t-form label-align="left" label-width="160px">
        <t-form-item label="自动检查更新">
          <t-switch v-model="settings.auto_check_update" @change="onAutoCheckChange" />
          <template #help>
            开启后，应用启动时自动对比 GitHub 上 llama.cpp 的最新版本与本机已安装版本，
            发现有新版本时弹窗提示，可选择前往更新；关闭后不再自动检查。
            随时开关，立即生效。
          </template>
        </t-form-item>
      </t-form>
    </t-card>

    <t-card title="关于">
      <t-space direction="vertical">
        <span>llama-ui — llama.cpp 可视化启动器 (Tauri 2 + Vue 3)</span>
        <span class="mono">
          程序来源: {{ sourceLabel }}{{ settings.exe_source === "managed" ? ` · ${settings.current_tag || "未安装"}` : "" }}
        </span>
        <span class="mono">数据目录: {{ dataDir }}</span>
      </t-space>
    </t-card>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { MessagePlugin } from "tdesign-vue-next";
import { appStore, type Settings } from "../composables/server";
import { runUpdateCheck } from "../composables/update";

const settings = reactive<Settings>({
  install_dir: "",
  models_dir: "",
  current_tag: null,
  mirror: "",
  exe_source: "managed",
  custom_dir: "",
  managed_dir: "",
  auto_check_update: false,
});
const dataDir = ref("");

const sourceLabel = computed(() =>
  settings.exe_source === "custom" ? "自定义（本地目录）" : "托管下载"
);

async function save() {
  try {
    // 目录字段去首尾空白；镜像同理，避免误输入空格破坏拼接
    settings.mirror = settings.mirror.trim();
    settings.custom_dir = settings.custom_dir.trim();
    settings.managed_dir = settings.managed_dir.trim();
    // 程序来源必填校验：托管模式必须有托管目录，自定义模式必须有本地根目录
    if (settings.exe_source === "managed" && !settings.managed_dir) {
      MessagePlugin.warning("托管下载模式必须先指定托管下载目录");
      return;
    }
    if (settings.exe_source === "custom" && !settings.custom_dir) {
      MessagePlugin.warning("自定义模式必须先指定本地 llama.cpp 根目录");
      return;
    }
    await invoke("save_settings", { settings: { ...settings } });
    appStore.settings = { ...settings };
    MessagePlugin.success("设置已保存");
  } catch (e) {
    MessagePlugin.error(String(e));
  }
}

async function pickManagedDir() {
  const dir = await open({ directory: true });
  if (typeof dir === "string") {
    settings.managed_dir = dir;
    await save();
  }
}

async function pickCustomDir() {
  const dir = await open({ directory: true });
  if (typeof dir === "string") {
    settings.custom_dir = dir;
    await save();
  }
}

async function pickModels() {
  const dir = await open({ directory: true });
  if (typeof dir === "string") {
    settings.models_dir = dir;
    await save();
  }
}

/** 自动检查更新开关：切换立即持久化；开启时立即手动检查一次，当场给出结果 */
async function onAutoCheckChange(val: boolean | string | number) {
  try {
    await invoke("save_settings", { settings: { ...settings } });
    appStore.settings = { ...settings };
    if (val) {
      await runUpdateCheck(true);
    }
  } catch (e) {
    MessagePlugin.error(String(e));
  }
}

onMounted(async () => {
  const s = await invoke<Settings>("get_settings");
  Object.assign(settings, s);
  appStore.settings = { ...s };
  dataDir.value = await invoke("get_data_dir");
});
</script>
