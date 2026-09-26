<template>
  <t-layout class="app-layout">
    <t-aside :width="collapsed ? '68px' : '220px'">
      <div class="brand" :class="{ 'brand--collapsed': collapsed }">
        <span>🦙</span>
        <span v-if="!collapsed">llama-ui</span>
      </div>
      <t-menu :value="active" :collapsed="collapsed" @change="onMenu">
        <t-menu-item value="/server">
          <template #icon><t-icon name="server" /></template>
          服务管理
        </t-menu-item>
        <t-menu-item value="/models">
          <template #icon><t-icon name="folder-open" /></template>
          模型管理
        </t-menu-item>
        <t-menu-item value="/download">
          <template #icon><t-icon name="download" /></template>
          下载与更新
        </t-menu-item>
        <t-menu-item value="/settings">
          <template #icon><t-icon name="setting" /></template>
          设置
        </t-menu-item>
      </t-menu>
    </t-aside>
    <t-layout>
      <t-content>
        <router-view />
      </t-content>
    </t-layout>
    <!-- 全局下载悬浮窗：任何页面都能看到下载进度，点击跳转下载页（下载页自身有完整卡片，不重复显示） -->
    <div
      v-if="installing && route.path !== '/download'"
      class="dl-float"
      title="点击前往下载与更新页"
      @click="router.push('/download')"
    >
      <div class="dl-float__head">
        <span class="dl-float__title">
          llama.cpp 下载中 {{ progress }}%
          <span v-if="speed > 0" class="dl-float__speed">{{ speed.toFixed(1) }} MB/s</span>
        </span>
        <t-button
          size="small"
          variant="text"
          theme="danger"
          @click.stop="onCancelClick"
        >
          取消
        </t-button>
      </div>
      <t-progress :percentage="progress" />
      <div class="dl-float__msg">{{ progressMsg }}</div>
    </div>
  </t-layout>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { appStore, initServerListener, type Settings } from "./composables/server";
import { runUpdateCheck } from "./composables/update";
import {
  cancelDownload,
  initDownloadListener,
  installing,
  progress,
  progressMsg,
  speed,
} from "./composables/download";

initServerListener();
initDownloadListener();

/** 取消下载（悬浮窗按钮）：后端中断并清理，前端状态由 cancelled 事件复位 */
async function onCancelClick() {
  try {
    await cancelDownload();
  } catch {
    // 取消失败不影响页面，进度事件会继续驱动状态
  }
}

const route = useRoute();
const router = useRouter();
const active = computed(() => route.path);

// 窗口较窄时侧边栏自动收起为图标模式
const collapsed = ref(false);
function syncCollapsed() {
  collapsed.value = window.innerWidth < 960;
}
onMounted(() => {
  syncCollapsed();
  window.addEventListener("resize", syncCollapsed);
  // 开启了自动检查更新时，启动后台静默检查一次；
  // 发现新版本由 composable 内弹窗提示（每次启动最多弹一次），失败不打扰用户
  invoke<Settings>("get_settings")
    .then((s) => {
      appStore.settings = s;
      if (s.auto_check_update) runUpdateCheck(false);
    })
    .catch(() => {});
});
onBeforeUnmount(() => {
  window.removeEventListener("resize", syncCollapsed);
});

function onMenu(value: string | number) {
  router.push(String(value));
}
</script>

<style scoped>
/* 收起为图标模式时，brand 仅剩图标，居中显示 */
.brand--collapsed {
  justify-content: center;
  padding-left: 0;
  padding-right: 0;
}

/* 全局下载悬浮窗：右下角常驻，下载中切到任何页面都可见 */
.dl-float {
  position: fixed;
  right: 16px;
  bottom: 16px;
  width: min(380px, calc(100vw - 32px));
  background: var(--td-bg-color-container, #fff);
  border: 1px solid var(--td-component-stroke, #e7e7e7);
  border-radius: 9px;
  box-shadow: var(--td-shadow-2, 0 3px 14px rgba(0, 0, 0, 0.1));
  padding: 10px 14px;
  cursor: pointer;
  z-index: 2600;
  font-size: 12px;
  color: var(--td-text-color-primary, rgba(0, 0, 0, 0.9));
}
.dl-float__head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}
.dl-float__title {
  font-weight: 600;
  white-space: nowrap;
}
.dl-float__speed {
  color: var(--td-brand-color, #0052d9);
  font-weight: 400;
  margin-left: 6px;
}
.dl-float__msg {
  color: var(--td-text-color-placeholder, #999);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
