<template>
  <div class="page">
    <t-card title="模型目录" class="mb-16">
      <t-space break-line>
        <t-input v-model="modelsDir" readonly style="width: min(480px, 100%)" placeholder="未设置模型目录" class="mono" />
        <t-button @click="pickDir">选择目录</t-button>
        <t-button theme="primary" variant="outline" @click="scan">扫描 .gguf 文件</t-button>
      </t-space>
    </t-card>

    <t-card title="已发现的 GGUF 模型">
      <t-table
        :data="models"
        :columns="columns"
        row-key="path"
        :loading="loading"
        :hover="true"
        :pagination="{ pageSize: 20 }"
      />
    </t-card>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { MessagePlugin } from "tdesign-vue-next";
import { appStore, formatSize, type ModelInfo } from "../composables/server";

const modelsDir = ref("");
const models = ref<ModelInfo[]>([]);
const loading = ref(false);

const columns = [
  { colKey: "name", title: "文件名", ellipsis: true },
  {
    colKey: "size",
    title: "大小",
    width: 120,
    cell: (_h: any, { row }: any) => formatSize(row.size),
  },
  { colKey: "path", title: "路径", ellipsis: true, className: "mono" },
];

async function pickDir() {
  const dir = await open({ directory: true });
  if (typeof dir === "string") {
    modelsDir.value = dir;
    await save();
  }
}

async function save() {
  const s: any = await invoke("get_settings");
  s.models_dir = modelsDir.value;
  await invoke("save_settings", { settings: s });
}

async function scan() {
  loading.value = true;
  try {
    await save();
    models.value = await invoke("list_models", { dir: modelsDir.value });
    if (models.value.length === 0) {
      MessagePlugin.warning("该目录下没有找到 .gguf 文件");
    }
  } catch (e) {
    MessagePlugin.error(String(e));
  } finally {
    loading.value = false;
  }
}

onMounted(async () => {
  try {
    const s: any = await invoke("get_settings");
    modelsDir.value = s.models_dir || "";
    if (modelsDir.value) await scan();
  } catch (e) {
    MessagePlugin.error("读取设置失败: " + String(e));
  }
});
</script>
