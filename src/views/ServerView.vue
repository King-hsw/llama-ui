<template>
  <div class="page">
    <!-- 状态与操作 -->
    <t-card class="mb-16">
      <t-space align="center" break-line>
        <t-tag :theme="serverState.running ? 'success' : 'default'" variant="light">
          {{ serverState.running ? "● 运行中" : "○ 已停止" }}
        </t-tag>
        <t-tag v-if="serverState.running && serverState.pid" variant="outline">
          PID {{ serverState.pid }}
        </t-tag>
        <t-tag v-if="serverState.running && serverState.port" variant="outline">
          http://{{ displayHost }}:{{ serverState.port }}
        </t-tag>
        <t-tooltip :content="cfg.noWebui ? '已通过 --no-webui 禁用内置 WebUI' : '在浏览器中打开 llama-server 内置页面'">
          <t-button
            variant="outline"
            :disabled="!serverState.running || !serverState.port || cfg.noWebui"
            @click="openWebui"
          >
            打开 WebUI
          </t-button>
        </t-tooltip>
        <t-button theme="primary" :disabled="serverState.running || !exePath" @click="start">
          启动服务
        </t-button>
        <t-button theme="danger" :disabled="!serverState.running" @click="stop">
          停止服务
        </t-button>
        <t-button variant="outline" @click="loadModels">刷新模型列表</t-button>
      </t-space>
      <div v-if="!exePath && exeMessage" style="margin-top: 12px">
        <t-tag theme="warning" variant="light">{{ exeMessage }}</t-tag>
      </div>
    </t-card>

    <!-- Runtime Dashboard：统一 Runtime State（metrics + 日志解析 + 诊断 + 系统资源对比） -->
    <RuntimeDashboard
      :stats="current"
      :monitor-rows="monitorRows"
      :baseline-locked="baselineLocked"
      :gpu-hint="gpuHint"
      @reset-baseline="captureBaseline(true)"
    />

    <!-- 参数配置 + 命令预览 -->
    <div class="config-row mb-16">
      <t-card title="启动参数" class="config-card">
        <template #actions>
          <t-space size="small" break-line>
            <t-select
              v-model="activeProfile"
              placeholder="加载已保存配置…"
              style="width: 180px"
              size="small"
              clearable
              :loading="profilesLoading"
              @change="applyProfile"
            >
              <t-option v-for="p in profiles" :key="p.name" :value="p.name" :label="p.name" />
            </t-select>
            <t-button size="small" variant="outline" @click="openSaveDlg">存为配置</t-button>
            <t-button size="small" variant="outline" @click="openImportDlg">导入命令</t-button>
            <t-button
              size="small"
              variant="outline"
              theme="danger"
              :disabled="!activeProfile"
              @click="removeProfile"
            >
              删除
            </t-button>
          </t-space>
        </template>
      <t-collapse :default-value="['base']">
        <!-- 基础 -->
        <t-collapse-panel value="base" header="基础 · 模型与上下文">
          <div class="grid">
            <t-form-item label="模型文件 (-m)">
              <t-select
                v-model="cfg.model"
                placeholder="在「模型管理」设置模型目录"
                filterable
                clearable
              >
                <t-option v-for="m in models" :key="m.path" :value="m.path" :label="m.name" />
              </t-select>
            </t-form-item>
            <t-form-item label="模型别名 (--alias)">
              <t-input v-model="cfg.alias" placeholder="可选，显示在 API 的 model 字段" />
            </t-form-item>
            <t-form-item label="监听地址 (--host)">
              <t-input v-model="cfg.host" placeholder="127.0.0.1" />
            </t-form-item>
            <t-form-item label="端口 (--port)">
              <t-input-number v-model="cfg.port" :min="1024" :max="65535" theme="column" />
            </t-form-item>
            <t-form-item label="上下文长度 (-c)">
              <t-input-number v-model="cfg.ctxSize" :min="0" :step="512" theme="column" placeholder="0 = 模型上限" />
            </t-form-item>
            <t-form-item label="并行槽位 (--parallel)">
              <t-input-number v-model="cfg.parallel" :min="1" :max="64" theme="column" placeholder="默认 1" />
            </t-form-item>
          </div>
        </t-collapse-panel>

        <!-- GPU -->
        <t-collapse-panel value="gpu" header="GPU 与显存">
          <div class="grid">
            <t-form-item label="GPU 层数 (-ngl)">
              <t-input-number v-model="cfg.gpuLayers" :min="0" :max="999" theme="column" placeholder="留空 = 不 offload" />
            </t-form-item>
            <t-form-item label="Flash Attention (-fa)">
              <t-select v-model="cfg.flashAttn" clearable>
                <t-option value="auto" label="auto（新版默认）" />
                <t-option value="on" label="on" />
                <t-option value="off" label="off" />
              </t-select>
            </t-form-item>
            <t-form-item label="多卡切分模式 (-sm)">
              <t-select v-model="cfg.splitMode" clearable>
                <t-option value="layer" label="layer（按层，默认）" />
                <t-option value="row" label="row（按行）" />
                <t-option value="none" label="none（仅主 GPU）" />
              </t-select>
            </t-form-item>
            <t-form-item label="显存比例 (-ts)">
              <t-input v-model="cfg.tensorSplit" placeholder="多卡如 3,2" />
            </t-form-item>
            <t-form-item label="主 GPU (-mg)">
              <t-input-number v-model="cfg.mainGpu" :min="0" :max="16" theme="column" />
            </t-form-item>
            <t-form-item label="KV Cache 量化 K (--cache-type-k)">
              <t-select v-model="cfg.cacheTypeK" clearable>
                <t-option v-for="t in kvTypes" :key="t" :value="t" :label="t" />
              </t-select>
            </t-form-item>
            <t-form-item label="KV Cache 量化 V (--cache-type-v)">
              <t-select v-model="cfg.cacheTypeV" clearable>
                <t-option v-for="t in kvTypes" :key="t" :value="t" :label="t" />
              </t-select>
            </t-form-item>
            <t-form-item label="统一 KV 缓冲 (--kv-unified)">
              <t-switch v-model="cfg.kvUnified" />
              <span class="hint">K/V 共享同一量化缓冲区</span>
            </t-form-item>
          </div>
        </t-collapse-panel>

        <!-- CPU 与内存 -->
        <t-collapse-panel value="cpu" header="CPU 与内存">
          <div class="grid">
            <t-form-item label="推理线程 (-t)">
              <t-input-number v-model="cfg.threads" :min="1" :max="256" theme="column" placeholder="留空 = 自动" />
            </t-form-item>
            <t-form-item label="批处理线程 (-tb)">
              <t-input-number v-model="cfg.threadsBatch" :min="1" :max="256" theme="column" />
            </t-form-item>
            <t-form-item label="逻辑批大小 (-b)">
              <t-input-number v-model="cfg.batchSize" :min="1" :step="512" theme="column" placeholder="默认 2048" />
            </t-form-item>
            <t-form-item label="物理批大小 (-ub)">
              <t-input-number v-model="cfg.ubatchSize" :min="1" :step="512" theme="column" placeholder="默认 512" />
            </t-form-item>
            <t-form-item label="保留 Token (--keep)">
              <t-input-number v-model="cfg.keep" :min="-1" theme="column" placeholder="-1 = 全保留" />
            </t-form-item>
            <t-form-item label="NUMA 策略 (--numa)">
              <t-select v-model="cfg.numa" clearable>
                <t-option value="distribute" label="distribute" />
                <t-option value="isolate" label="isolate" />
                <t-option value="numactl" label="numactl" />
              </t-select>
            </t-form-item>
            <t-form-item label="内存锁定 (--mlock)">
              <t-switch v-model="cfg.mlock" />
              <span class="hint">禁止系统换出模型内存</span>
            </t-form-item>
            <t-form-item label="禁用 mmap (--no-mmap)">
              <t-switch v-model="cfg.noMmap" />
              <span class="hint">启动时全量读入内存</span>
            </t-form-item>
            <t-form-item label="显存自动适配 (--fit)">
              <t-select v-model="cfg.fit" clearable placeholder="默认 auto">
                <t-option value="on" label="on · 自动降载适配显存" />
                <t-option value="off" label="off · 关闭" />
              </t-select>
            </t-form-item>
            <t-form-item label="加载模式 (--load-mode)">
              <t-select v-model="cfg.loadMode" clearable placeholder="留空 = 默认">
                <t-option value="mmap" label="mmap（内存映射）" />
                <t-option value="default" label="default" />
              </t-select>
            </t-form-item>
            <t-form-item label="跳过预热 (--no-warmup)">
              <t-switch v-model="cfg.noWarmup" />
              <span class="hint">启动时不跑一次空推理</span>
            </t-form-item>
          </div>
        </t-collapse-panel>

        <!-- 采样默认值 -->
        <t-collapse-panel value="sampling" header="采样默认值（API 可覆盖）">
          <div class="grid">
            <t-form-item label="温度 (--temp)">
              <t-input-number v-model="cfg.temp" :min="0" :max="2" :step="0.05" theme="column" />
            </t-form-item>
            <t-form-item label="Top-K (--top-k)">
              <t-input-number v-model="cfg.topK" :min="0" theme="column" placeholder="0 = 禁用" />
            </t-form-item>
            <t-form-item label="Top-P (--top-p)">
              <t-input-number v-model="cfg.topP" :min="0" :max="1" :step="0.05" theme="column" />
            </t-form-item>
            <t-form-item label="Min-P (--min-p)">
              <t-input-number v-model="cfg.minP" :min="0" :max="1" :step="0.01" theme="column" />
            </t-form-item>
            <t-form-item label="重复惩罚 (--repeat-penalty)">
              <t-input-number v-model="cfg.repeatPenalty" :min="0.5" :max="2" :step="0.05" theme="column" />
            </t-form-item>
            <t-form-item label="惩罚窗口 (--repeat-last-n)">
              <t-input-number v-model="cfg.repeatLastN" :min="-1" theme="column" />
            </t-form-item>
            <t-form-item label="出现惩罚 (--presence-penalty)">
              <t-input-number v-model="cfg.presencePenalty" :min="-2" :max="2" :step="0.1" theme="column" />
            </t-form-item>
            <t-form-item label="频率惩罚 (--frequency-penalty)">
              <t-input-number v-model="cfg.frequencyPenalty" :min="-2" :max="2" :step="0.1" theme="column" />
            </t-form-item>
            <t-form-item label="Mirostat (--mirostat)">
              <t-select v-model="cfg.mirostat" clearable placeholder="0 = 关闭">
                <t-option :value="0" label="0 · 关闭" />
                <t-option :value="1" label="1 · Mirostat v1" />
                <t-option :value="2" label="2 · Mirostat v2" />
              </t-select>
            </t-form-item>
            <t-form-item label="Mirostat 学习率 (--mirostat-lr)">
              <t-input-number v-model="cfg.mirostatLr" :min="0" :max="1" :step="0.05" theme="column" />
            </t-form-item>
            <t-form-item label="Mirostat 熵 (--mirostat-ent)">
              <t-input-number v-model="cfg.mirostatEnt" :min="0" :step="0.1" theme="column" />
            </t-form-item>
            <t-form-item label="随机种子 (--seed)">
              <t-input-number v-model="cfg.seed" :min="-1" theme="column" placeholder="-1 = 随机" />
            </t-form-item>
          </div>
        </t-collapse-panel>

        <!-- 服务行为 -->
        <t-collapse-panel value="server" header="服务行为">
          <div class="grid">
            <t-form-item label="API Key (--api-key)">
              <t-input v-model="cfg.apiKey" placeholder="留空 = 不鉴权" />
            </t-form-item>
            <t-form-item label="HTTP 线程 (--threads-http)">
              <t-input-number v-model="cfg.threadsHttp" :min="1" :max="128" theme="column" />
            </t-form-item>
            <t-form-item label="禁用内置 WebUI (--no-webui)">
              <t-switch v-model="cfg.noWebui" />
            </t-form-item>
            <t-form-item label="指标端点 (--metrics)">
              <t-switch v-model="cfg.metrics" />
              <span class="hint">/metrics Prometheus 格式</span>
            </t-form-item>
            <t-form-item label="槽位端点 (--slots)">
              <t-switch v-model="cfg.slots" />
            </t-form-item>
            <t-form-item label="Jinja 模板 (--jinja)">
              <t-switch v-model="cfg.jinja" />
              <span class="hint">使用模型自带 chat template</span>
            </t-form-item>
            <t-form-item label="Chat 模板 (--chat-template)">
              <t-input v-model="cfg.chatTemplate" placeholder="内置名（如 chatml）或内联 Jinja 模板，留空 = 自动识别" />
            </t-form-item>
            <t-form-item label="Chat 模板文件 (--chat-template-file)">
              <t-space break-line style="width: 100%">
                <t-input
                  v-model="cfg.chatTemplateFile"
                  placeholder="选择 .jinja 模板文件，留空 = 不指定"
                  class="mono"
                  style="width: min(420px, 100%)"
                />
                <t-button variant="outline" @click="pickTemplateFile">选择文件</t-button>
              </t-space>
            </t-form-item>
            <t-form-item label="日志格式 (--log-format)">
              <t-select v-model="cfg.logFormat" clearable>
                <t-option value="text" label="text" />
                <t-option value="json" label="json" />
              </t-select>
            </t-form-item>
          </div>
        </t-collapse-panel>

        <!-- 多模态与推理 -->
        <t-collapse-panel value="mm" header="多模态 · 推理输出">
          <div class="grid">
            <t-form-item label="多模态投影 (--mmproj)">
              <t-space break-line style="width: 100%">
                <t-input
                  v-model="cfg.mmproj"
                  placeholder="mmproj .gguf 路径，纯文本模型留空"
                  class="mono"
                  style="width: min(420px, 100%)"
                />
                <t-button variant="outline" @click="pickMmprojFile">选择文件</t-button>
              </t-space>
            </t-form-item>
            <t-form-item label="投影不上 GPU (--no-mmproj-offload)">
              <t-switch v-model="cfg.noMmprojOffload" />
              <span class="hint">mmproj 留在 CPU，节省显存</span>
            </t-form-item>
            <t-form-item label="图像最少 Token (--image-min-tokens)">
              <t-input-number v-model="cfg.imageMinTokens" :min="0" theme="column" />
            </t-form-item>
            <t-form-item label="图像最多 Token (--image-max-tokens)">
              <t-input-number v-model="cfg.imageMaxTokens" :min="0" theme="column" />
            </t-form-item>
            <t-form-item label="思维链输出 (--reasoning)">
              <t-select v-model="cfg.reasoning" clearable placeholder="默认 auto">
                <t-option value="on" label="on · 强制开启" />
                <t-option value="off" label="off · 关闭" />
              </t-select>
            </t-form-item>
            <t-form-item label="思维链格式 (--reasoning-format)">
              <t-select v-model="cfg.reasoningFormat" clearable>
                <t-option value="deepseek" label="deepseek" />
                <t-option value="deepseek-2" label="deepseek-2" />
                <t-option value="none" label="none" />
              </t-select>
            </t-form-item>
            <t-form-item label="保留思维链 (--reasoning-preserve)">
              <t-switch v-model="cfg.reasoningPreserve" />
              <span class="hint">不从回复中剥离思考内容</span>
            </t-form-item>
            <t-form-item label="模板参数 (--chat-template-kwargs)">
              <t-input
                v-model="cfg.chatTemplateKwargs"
                placeholder='JSON，如 {"preserve-thinking": true}'
                class="mono"
              />
            </t-form-item>
          </div>
        </t-collapse-panel>

        <!-- Embedding 与 RoPE -->
        <t-collapse-panel value="adv" header="Embedding · RoPE · 高级">
          <div class="grid">
            <t-form-item label="Embedding 模式 (--embedding)">
              <t-switch v-model="cfg.embedding" />
              <span class="hint">仅推理向量时开启</span>
            </t-form-item>
            <t-form-item label="池化方式 (--pooling)">
              <t-select v-model="cfg.pooling" clearable>
                <t-option value="none" label="none" />
                <t-option value="mean" label="mean" />
                <t-option value="cls" label="cls" />
                <t-option value="last" label="last" />
                <t-option value="rank" label="rank" />
              </t-select>
            </t-form-item>
            <t-form-item label="RoPE 缩放 (--rope-scaling)">
              <t-select v-model="cfg.ropeScaling" clearable>
                <t-option value="none" label="none" />
                <t-option value="linear" label="linear" />
                <t-option value="yarn" label="yarn" />
              </t-select>
            </t-form-item>
            <t-form-item label="RoPE 频率基数 (--rope-freq-base)">
              <t-input-number v-model="cfg.ropeFreqBase" :min="0" :step="1000" theme="column" />
            </t-form-item>
            <t-form-item label="RoPE 频率缩放 (--rope-freq-scale)">
              <t-input-number v-model="cfg.ropeFreqScale" :min="0" :step="0.05" theme="column" />
            </t-form-item>
            <t-form-item label="额外参数">
              <t-input v-model="cfg.extraArgs" placeholder="--grammar-file xx.gbnf 等，空格分隔" class="mono" />
            </t-form-item>
          </div>
        </t-collapse-panel>
      </t-collapse>
      </t-card>

      <div class="preview-col">
        <!-- 实时命令预览 -->
        <t-card title="命令预览" class="preview-card">
          <template #actions>
            <t-button variant="outline" size="small" @click="copyCommand">
              <template #icon><copy-icon /></template>
              {{ copied ? "已复制" : "复制" }}
            </t-button>
          </template>
          <div class="preview-cmd" :class="{ 'is-empty': !exePath }">{{ previewDisplay }}</div>
          <div class="preview-meta">
            共 {{ previewLines.length - 1 }} 个参数 · 随参数修改实时更新 · 复制为单行命令
          </div>
        </t-card>

        <!-- 本机配置信息 -->
        <t-card title="本机配置" class="sys-card">
          <div v-if="sysInfo" class="sys-list">
            <div class="sys-row">
              <span class="sys-label">操作系统</span>
              <span class="sys-value">{{ sysInfo.os }}</span>
            </div>
            <div class="sys-row">
              <span class="sys-label">处理器</span>
              <span class="sys-value">{{ sysInfo.cpu }}</span>
            </div>
            <div class="sys-row">
              <span class="sys-label">系统架构</span>
              <span class="sys-value">{{ sysInfo.arch || "未知" }}</span>
            </div>
            <div class="sys-row">
              <span class="sys-label">核心/线程</span>
              <span class="sys-value">
                {{ sysInfo.physical_cores ?? "—" }} 物理核 /
                {{ sysInfo.logical_cores ?? "—" }} 逻辑线程
              </span>
            </div>
            <div class="sys-row">
              <span class="sys-label">内存</span>
              <span class="sys-value">{{ sysInfo.ram_gb > 0 ? sysInfo.ram_gb + " GB" : "未知" }}</span>
            </div>
            <div class="sys-row">
              <span class="sys-label">显卡</span>
              <span class="sys-value">
                <template v-if="sysInfo.gpus.length">
                  <div v-for="g in sysInfo.gpus" :key="g.name" class="sys-gpu">
                    {{ g.name }}<span v-if="fmtVram(g)"> · {{ fmtVram(g) }}</span>
                  </div>
                </template>
                <template v-else>未检测到独立/集成显卡信息</template>
              </span>
            </div>
            <div class="sys-row">
              <span class="sys-label">推理引擎</span>
              <span class="sys-value mono-val">
                {{ exePath || "未找到 llama-server.exe" }}
                <t-tag v-if="exePath && exeSource === 'custom'" theme="primary" variant="light" size="small">
                  自定义
                </t-tag>
                <t-tag v-else-if="exePath && exeSource === 'managed'" theme="success" variant="light" size="small">
                  托管 {{ exeTag }}
                </t-tag>
              </span>
            </div>
            <div class="sys-row">
              <span class="sys-label">模型目录</span>
              <span class="sys-value mono-val">{{ modelsDir || "未设置" }}</span>
            </div>
          </div>
          <t-loading v-else size="small" text="正在识别本机配置…" />
        </t-card>
      </div>
    </div>

    <!-- 日志 -->
    <t-card title="运行日志">
      <template #actions>
        <t-button variant="text" size="small" @click="appStore.logs = []">清空</t-button>
      </template>
      <div ref="logRef" class="log-panel">{{ renderedLogs || "等待服务启动…" }}</div>
    </t-card>

    <!-- 导入命令对话框：粘贴启动命令逆向解析回填 -->
    <t-dialog
      v-model:visible="importDlgVisible"
      header="导入启动命令"
      :confirm-btn="{ content: '解析并回填', theme: 'primary' }"
      :close-on-overlay-click="false"
      width="640px"
      @confirm="confirmImport"
    >
      <t-textarea
        v-model="importText"
        :autosize="{ minRows: 4, maxRows: 10 }"
        placeholder='粘贴完整启动命令，如：&#10;llama-server.exe -m D:\models\qwen.gguf --port 8080 -ngl 99 --temp 0.7 --jinja'
        class="mono"
      />
      <div class="dlg-hint">
        支持带引号路径与 --flag=value 写法；可识别的参数回填到对应配置项，无法识别的参数原样追加到「额外参数」。
      </div>
    </t-dialog>

    <!-- 保存配置对话框 -->
    <t-dialog
      v-model:visible="saveDlgVisible"
      header="保存启动配置"
      :confirm-btn="{ content: '保存', theme: 'primary' }"
      :close-on-overlay-click="false"
      @confirm="confirmSaveProfile"
    >
      <t-form label-align="left" label-width="80px" @submit.prevent>
        <t-form-item label="配置名称">
          <t-input
            v-model="saveDlgName"
            placeholder="如：7B-CPU 日常 / 32B-GPU 高速"
            @enter="confirmSaveProfile"
          />
        </t-form-item>
      </t-form>
      <div class="dlg-hint">将保存当前全部启动参数；同名配置会被覆盖。</div>
    </t-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CopyIcon } from "tdesign-icons-vue-next";
import RuntimeDashboard from "../components/RuntimeDashboard.vue";
import {
  appStore,
  initServerListener,
  serverState,
  type ModelInfo,
  type ResolvedExe,
} from "../composables/server";
import { initRuntimeListener } from "../composables/runtime";
import { MessagePlugin, DialogPlugin } from "tdesign-vue-next";

const kvTypes = ["f16", "bf16", "f32", "q8_0", "q5_1", "q5_0", "q4_1", "q4_0"];

function defaultCfg(): Record<string, any> {
  return {
  model: "",
  alias: null,
  host: "127.0.0.1",
  port: 8080,
  ctxSize: 4096,
  parallel: null,
  gpuLayers: null,
  flashAttn: null,
  splitMode: null,
  tensorSplit: null,
  mainGpu: null,
  cacheTypeK: null,
  cacheTypeV: null,
  kvUnified: false,
  fit: null,
  loadMode: null,
  noWarmup: false,
  mmproj: "",
  noMmprojOffload: false,
  imageMinTokens: null,
  imageMaxTokens: null,
  reasoning: null,
  chatTemplateKwargs: null,
  reasoningFormat: null,
  reasoningPreserve: false,
  threads: null,
  threadsBatch: null,
  batchSize: null,
  ubatchSize: null,
  keep: null,
  numa: null,
  mlock: false,
  noMmap: false,
  temp: null,
  topK: null,
  topP: null,
  minP: null,
  repeatPenalty: null,
  repeatLastN: null,
  presencePenalty: null,
  frequencyPenalty: null,
  mirostat: null,
  mirostatLr: null,
  mirostatEnt: null,
  seed: null,
  apiKey: null,
  threadsHttp: null,
  noWebui: false,
  metrics: false,
  slots: false,
  jinja: false,
  chatTemplate: null,
  chatTemplateFile: null,
  logFormat: null,
  embedding: false,
  pooling: null,
  ropeScaling: null,
  ropeFreqBase: null,
  ropeFreqScale: null,
  extraArgs: "",
  };
}

const cfg = ref<any>(defaultCfg());

/** 浏览器访问地址：0.0.0.0 / :: 是监听地址而非可访问地址，改用 localhost */
const displayHost = computed(() => {
  const h = String(cfg.value.host ?? "").trim();
  return h === "0.0.0.0" || h === "::" ? "localhost" : h || "localhost";
});

async function openWebui() {
  if (!serverState.running || !serverState.port) return;
  const url = `http://${displayHost.value}:${serverState.port}`;
  try {
    await openUrl(url);
  } catch (e) {
    MessagePlugin.error("打开浏览器失败: " + String(e));
  }
}

const models = ref<ModelInfo[]>([]);
const exePath = ref<string>("");
/** exe 来源："custom"（本地自备）/ "managed"（托管下载） */
const exeSource = ref<string>("");
const exeTag = ref<string | null>(null);
/** 后端解析失败的引导信息（来源未配置/目录缺失/版本不存在等） */
const exeMessage = ref<string>("");
const modelsDir = ref<string>("");
const logRef = ref<HTMLElement>();

// ---------- 本机配置信息 ----------

interface GpuInfo {
  name: string;
  vram_mb: number | null;
  source: string;
}
interface SystemInfo {
  os: string;
  cpu: string;
  arch: string;
  physical_cores: number | null;
  logical_cores: number | null;
  ram_gb: number;
  gpus: GpuInfo[];
}
const sysInfo = ref<SystemInfo | null>(null);

function fmtVram(g: GpuInfo): string {
  if (!g.vram_mb || g.vram_mb <= 0) return "";
  return g.vram_mb >= 1024
    ? `${(g.vram_mb / 1024).toFixed(1)} GB 显存`
    : `${g.vram_mb} MB 显存`;
}

watch(
  () => appStore.logs.length,
  async () => {
    await nextTick();
    logRef.value?.scrollTo({ top: logRef.value.scrollHeight });
  }
);

/** 日志面板只渲染最后 400 行：全量 join 上千行会拖垮渲染 */
const renderedLogs = computed(() =>
  appStore.logs.length > 400
    ? appStore.logs.slice(-400).join("\n")
    : appStore.logs.join("\n")
);

/** 前端驼峰 → Rust snake_case；"" 是"未设置"哨兵值，转 null */
const KEY_MAP: Record<string, string> = {
  ctxSize: "ctx_size",
  gpuLayers: "gpu_layers",
  flashAttn: "flash_attn",
  splitMode: "split_mode",
  tensorSplit: "tensor_split",
  mainGpu: "main_gpu",
  cacheTypeK: "cache_type_k",
  cacheTypeV: "cache_type_v",
  kvUnified: "kv_unified",
  fit: "fit",
  loadMode: "load_mode",
  noWarmup: "no_warmup",
  mmproj: "mmproj",
  noMmprojOffload: "no_mmproj_offload",
  imageMinTokens: "image_min_tokens",
  imageMaxTokens: "image_max_tokens",
  reasoning: "reasoning",
  chatTemplateKwargs: "chat_template_kwargs",
  reasoningFormat: "reasoning_format",
  reasoningPreserve: "reasoning_preserve",
  threadsBatch: "threads_batch",
  batchSize: "batch_size",
  ubatchSize: "ubatch_size",
  noMmap: "no_mmap",
  topK: "top_k",
  topP: "top_p",
  minP: "min_p",
  repeatPenalty: "repeat_penalty",
  repeatLastN: "repeat_last_n",
  presencePenalty: "presence_penalty",
  frequencyPenalty: "frequency_penalty",
  mirostatLr: "mirostat_lr",
  mirostatEnt: "mirostat_ent",
  apiKey: "api_key",
  threadsHttp: "threads_http",
  noWebui: "no_webui",
  chatTemplate: "chat_template",
  chatTemplateFile: "chat_template_file",
  logFormat: "log_format",
  ropeScaling: "rope_scaling",
  ropeFreqBase: "rope_freq_base",
  ropeFreqScale: "rope_freq_scale",
  extraArgs: "extra_args",
};

/** Rust ServerConfig 中必填的 String 字段（exe 由 exePath 单独提供），空串不能转 null */
const REQUIRED_STR_KEYS = new Set(["model", "host", "extraArgs"]);

function toPayload(c: Record<string, any>) {
  const out: Record<string, any> = { exe: exePath.value };
  for (const [k, v] of Object.entries(c)) {
    const key = KEY_MAP[k] ?? k;
    out[key] = v === "" && !REQUIRED_STR_KEYS.has(k) ? null : v;
  }
  return out;
}

// ---------- 命令预览 ----------

/** 含空格的参数加引号，仅用于展示（实际进程以 argv 数组传递，无需转义） */
function quoteArg(v: string): string {
  return /\s/.test(v) ? `"${v.replace(/"/g, '\\"')}"` : v;
}

/**
 * 每行一个参数，与 Rust 端 start_server 的拼装顺序和"空值跳过"规则保持一致。
 * 第 0 行是 exe 本身；复制时合并为单行可执行命令。
 */
const previewLines = computed<string[]>(() => {
  const c = cfg.value;
  const lines: string[] = [quoteArg(exePath.value || "llama-server.exe")];
  const pushS = (flag: string, v: any) => {
    if (v !== null && v !== undefined && String(v).trim() !== "") {
      lines.push(`${flag} ${quoteArg(String(v).trim())}`);
    }
  };
  const pushN = (flag: string, v: any) => {
    if (v !== null && v !== undefined && v !== "") lines.push(`${flag} ${v}`);
  };
  const pushF = (flag: string, on: boolean) => {
    if (on) lines.push(flag);
  };

  lines.push(
    `-m ${quoteArg(String(c.model ?? ""))}`,
    `--host ${quoteArg(String(c.host ?? ""))}`,
    `--port ${c.port ?? ""}`,
    `-c ${c.ctxSize ?? 0}`
  );
  pushS("--alias", c.alias);
  pushN("--parallel", c.parallel);
  pushN("-b", c.batchSize);
  pushN("-ub", c.ubatchSize);
  pushN("--keep", c.keep);
  pushN("-ngl", c.gpuLayers);
  pushS("-fa", c.flashAttn);
  pushS("-sm", c.splitMode);
  pushS("-ts", c.tensorSplit);
  pushN("-mg", c.mainGpu);
  pushN("-t", c.threads);
  pushN("-tb", c.threadsBatch);
  pushF("--no-mmap", c.noMmap);
  pushF("--mlock", c.mlock);
  pushS("--numa", c.numa);
  pushS("--cache-type-k", c.cacheTypeK);
  pushS("--cache-type-v", c.cacheTypeV);
  pushS("--rope-scaling", c.ropeScaling);
  pushN("--rope-freq-base", c.ropeFreqBase);
  pushN("--rope-freq-scale", c.ropeFreqScale);
  pushN("--temp", c.temp);
  pushN("--top-k", c.topK);
  pushN("--top-p", c.topP);
  pushN("--min-p", c.minP);
  pushN("--repeat-penalty", c.repeatPenalty);
  pushN("--repeat-last-n", c.repeatLastN);
  pushN("--presence-penalty", c.presencePenalty);
  pushN("--frequency-penalty", c.frequencyPenalty);
  pushN("--mirostat", c.mirostat);
  pushN("--mirostat-lr", c.mirostatLr);
  pushN("--mirostat-ent", c.mirostatEnt);
  pushN("--seed", c.seed);
  pushS("--api-key", c.apiKey);
  pushN("--threads-http", c.threadsHttp);
  pushF("--no-webui", c.noWebui);
  pushF("--metrics", c.metrics);
  pushF("--slots", c.slots);
  pushF("--jinja", c.jinja);
  pushS("--chat-template", c.chatTemplate);
  pushS("--chat-template-file", c.chatTemplateFile);
  pushS("--mmproj", c.mmproj);
  pushF("--no-mmproj-offload", c.noMmprojOffload);
  pushN("--image-min-tokens", c.imageMinTokens);
  pushN("--image-max-tokens", c.imageMaxTokens);
  pushF("--kv-unified", c.kvUnified);
  pushS("--fit", c.fit);
  pushS("--load-mode", c.loadMode);
  pushF("--no-warmup", c.noWarmup);
  pushS("--reasoning", c.reasoning);
  pushS("--chat-template-kwargs", c.chatTemplateKwargs);
  pushS("--reasoning-format", c.reasoningFormat);
  pushF("--reasoning-preserve", c.reasoningPreserve);
  pushF("--embedding", c.embedding);
  pushS("--pooling", c.pooling);
  pushS("--log-format", c.logFormat);
  if (c.extraArgs && String(c.extraArgs).trim() !== "") {
    lines.push(String(c.extraArgs).trim());
  }

  return lines;
});

/** 复制用：单行可执行命令 */
const previewCommand = computed(() => previewLines.value.join(" "));
/** 展示用：每行一个参数 */
const previewDisplay = computed(() => previewLines.value.join("\n"));

const copied = ref(false);
let copiedTimer: ReturnType<typeof setTimeout> | undefined;

async function copyCommand() {
  const text = previewCommand.value;
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
    } else {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
    }
    copied.value = true;
    MessagePlugin.success("命令已复制到剪贴板");
    clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => (copied.value = false), 1500);
  } catch (e) {
    MessagePlugin.error("复制失败: " + String(e));
  }
}

async function pickTemplateFile() {
  try {
    const f = await open({
      multiple: false,
      title: "选择聊天模板文件（.jinja）",
      filters: [
        { name: "Jinja 聊天模板", extensions: ["jinja", "jinja2"] },
        { name: "所有文件", extensions: ["*"] },
      ],
    });
    if (typeof f === "string") {
      cfg.value.chatTemplateFile = f;
    }
  } catch (e) {
    MessagePlugin.error(String(e));
  }
}

async function pickMmprojFile() {
  try {
    const f = await open({
      multiple: false,
      title: "选择多模态投影文件（mmproj .gguf）",
      filters: [
        { name: "GGUF 模型", extensions: ["gguf"] },
        { name: "所有文件", extensions: ["*"] },
      ],
    });
    if (typeof f === "string") {
      cfg.value.mmproj = f;
    }
  } catch (e) {
    MessagePlugin.error(String(e));
  }
}

async function loadModels() {
  try {
    const s: any = await invoke("get_settings");
    modelsDir.value = s.models_dir || "";
    models.value = await invoke("list_models", { dir: s.models_dir });
    if (!cfg.value.model && models.value.length > 0) {
      cfg.value.model = models.value[0].path;
    }
  } catch (e) {
    MessagePlugin.error(String(e));
  }
}

async function start() {
  if (!cfg.value.model) {
    MessagePlugin.warning("请先选择模型文件");
    return;
  }
  // 点击启动的这一刻采样并锁定基线，刷新过程保持不变，作为对比基准
  baseline.value = await sampleStats();
  baselineLocked.value = true;
  current.value = null;
  try {
    await invoke("start_server", { cfg: toPayload(cfg.value) });
    MessagePlugin.success("llama-server 已启动");
  } catch (e) {
    MessagePlugin.error(String(e));
  }
}

async function stop() {
  try {
    await invoke("stop_server");
    MessagePlugin.success("已停止");
  } catch (e) {
    MessagePlugin.error(String(e));
  }
}

// ---------- 启动配置方案 ----------

interface ProfileItem {
  name: string;
  cfg: Record<string, any>;
}

const profiles = ref<ProfileItem[]>([]);
const activeProfile = ref<string>("");
const profilesLoading = ref(false);
const saveDlgVisible = ref(false);
const saveDlgName = ref("");

async function loadProfiles() {
  profilesLoading.value = true;
  try {
    profiles.value = await invoke<ProfileItem[]>("list_profiles");
  } finally {
    profilesLoading.value = false;
  }
}

function applyProfile(name: string | unknown) {
  const key = String(name ?? "");
  if (!key) return; // 清除选择时不动作
  const p = profiles.value.find((x) => x.name === key);
  if (!p) return;
  cfg.value = { ...defaultCfg(), ...p.cfg };
  MessagePlugin.success(`已加载配置「${key}」`);
}

function openSaveDlg() {
  saveDlgName.value = activeProfile.value || "";
  saveDlgVisible.value = true;
}

async function confirmSaveProfile() {
  const name = saveDlgName.value.trim();
  if (!name) {
    MessagePlugin.warning("请输入配置名称");
    return;
  }
  const overwrite = profiles.value.some((p) => p.name === name);
  try {
    profiles.value = await invoke("save_profile", {
      name,
      cfg: JSON.parse(JSON.stringify(cfg.value)),
    });
    activeProfile.value = name;
    saveDlgVisible.value = false;
    MessagePlugin.success(overwrite ? `配置「${name}」已覆盖保存` : `配置「${name}」已保存`);
  } catch (e) {
    MessagePlugin.error(String(e));
  }
}

function removeProfile() {
  if (!activeProfile.value) return;
  const name = activeProfile.value;
  const dlg = DialogPlugin.confirm({
    header: "删除配置",
    body: `确定删除配置「${name}」？该操作不可恢复。`,
    confirmBtn: { content: "删除", theme: "danger" },
    cancelBtn: "取消",
    onConfirm: async () => {
      dlg.destroy();
      try {
        profiles.value = await invoke("delete_profile", { name });
        activeProfile.value = "";
        MessagePlugin.success("配置已删除");
      } catch (e) {
        MessagePlugin.error(String(e));
      }
    },
  });
}

// ---------- 命令导入：逆向解析回填 ----------

const importDlgVisible = ref(false);
const importText = ref("");

function openImportDlg() {
  importText.value = "";
  importDlgVisible.value = true;
}

/**
 * 分词：支持双/单引号（用于含空格路径）与 Windows 反斜杠路径。
 * 引号内转义两种风格都兼容：
 * - cmd 双写引号 ""  → 依靠"关闭后立即重开"自然拼接
 * - 反斜杠转义 \" \\ → 与本应用命令预览（quoteArg）导出的格式对称，可无损往返
 * 独立成 token 的 ^ 是 cmd 续行/转义符（常见于从 .bat 复制的命令），在结果中过滤。
 */
function tokenizeCmd(cmd: string): string[] {
  const out: string[] = [];
  let cur = "";
  let quote: string | null = null;
  let escaped = false;
  let started = false;
  const chars = [...cmd];
  const flush = () => {
    if (cur || started) {
      out.push(cur);
      cur = "";
      started = false;
    }
  };
  for (let idx = 0; idx < chars.length; idx++) {
    const ch = chars[idx];
    if (quote === '"') {
      if (escaped) {
        // \" → 字面引号；\\ → 字面反斜杠；\x → 原样保留（兼容 C:\Users 路径）
        cur += ch === '"' || ch === "\\" ? ch : `\\${ch}`;
        escaped = false;
      } else if (ch === "\\") {
        escaped = true;
      } else if (ch === '"') {
        if (chars[idx + 1] === '"') {
          // cmd 转义风格：引号内成对出现的 "" 是一个字面引号
          cur += '"';
          idx++;
        } else {
          quote = null;
        }
      } else {
        cur += ch;
      }
    } else if (quote === "'") {
      if (ch === "'") quote = null;
      else cur += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
      started = true;
    } else if (/\s/.test(ch)) {
      flush();
    } else {
      cur += ch;
      started = true;
    }
  }
  if (escaped) cur += "\\";
  flush();
  // cmd 语法中独立出现的 ^（续行符 / 转义空格）不参与参数解析
  return out.filter((t) => t !== "^");
}

/**
 * flag → [cfg 字段, 类型]；短/长别名同源，与 previewLines、Rust start_server 的拼装一一对应。
 * 类型：s=字符串 n=数字 b=布尔开关
 */
const FLAG_MAP: Record<string, [string, "s" | "n" | "b"]> = {
  "-m": ["model", "s"],
  "--model": ["model", "s"],
  "--alias": ["alias", "s"],
  "--host": ["host", "s"],
  "--port": ["port", "n"],
  "-c": ["ctxSize", "n"],
  "--ctx-size": ["ctxSize", "n"],
  "--parallel": ["parallel", "n"],
  "-n": ["predict", "n"],
  "--predict": ["predict", "n"],
  "-b": ["batchSize", "n"],
  "--batch-size": ["batchSize", "n"],
  "-ub": ["ubatchSize", "n"],
  "--ubatch-size": ["ubatchSize", "n"],
  "--keep": ["keep", "n"],
  "-ngl": ["gpuLayers", "n"],
  "--n-gpu-layers": ["gpuLayers", "n"],
  "--gpu-layers": ["gpuLayers", "n"],
  "-fa": ["flashAttn", "s"],
  "--flash-attn": ["flashAttn", "s"],
  "-sm": ["splitMode", "s"],
  "--split-mode": ["splitMode", "s"],
  "-ts": ["tensorSplit", "s"],
  "--tensor-split": ["tensorSplit", "s"],
  "-mg": ["mainGpu", "n"],
  "--main-gpu": ["mainGpu", "n"],
  "-t": ["threads", "n"],
  "--threads": ["threads", "n"],
  "-tb": ["threadsBatch", "n"],
  "--threads-batch": ["threadsBatch", "n"],
  "--no-mmap": ["noMmap", "b"],
  "--mlock": ["mlock", "b"],
  "--numa": ["numa", "s"],
  "--cache-type-k": ["cacheTypeK", "s"],
  "--cache-type-v": ["cacheTypeV", "s"],
  "--rope-scaling": ["ropeScaling", "s"],
  "--rope-freq-base": ["ropeFreqBase", "n"],
  "--rope-freq-scale": ["ropeFreqScale", "n"],
  "--temp": ["temp", "n"],
  "--temperature": ["temp", "n"],
  "--top-k": ["topK", "n"],
  "--top-p": ["topP", "n"],
  "--min-p": ["minP", "n"],
  "--repeat-penalty": ["repeatPenalty", "n"],
  "--repeat-last-n": ["repeatLastN", "n"],
  "--presence-penalty": ["presencePenalty", "n"],
  "--frequency-penalty": ["frequencyPenalty", "n"],
  "--mirostat": ["mirostat", "n"],
  "--mirostat-lr": ["mirostatLr", "n"],
  "--mirostat-ent": ["mirostatEnt", "n"],
  "--seed": ["seed", "n"],
  "--api-key": ["apiKey", "s"],
  "--threads-http": ["threadsHttp", "n"],
  "--no-webui": ["noWebui", "b"],
  "--metrics": ["metrics", "b"],
  "--slots": ["slots", "b"],
  "--jinja": ["jinja", "b"],
  "--chat-template": ["chatTemplate", "s"],
  "--chat-template-file": ["chatTemplateFile", "s"],
  "--mmproj": ["mmproj", "s"],
  "--no-mmproj-offload": ["noMmprojOffload", "b"],
  "--image-min-tokens": ["imageMinTokens", "n"],
  "--image-max-tokens": ["imageMaxTokens", "n"],
  "--kv-unified": ["kvUnified", "b"],
  "--fit": ["fit", "s"],
  "--load-mode": ["loadMode", "s"],
  "--no-warmup": ["noWarmup", "b"],
  "--reasoning": ["reasoning", "s"],
  "--chat-template-kwargs": ["chatTemplateKwargs", "s"],
  "--reasoning-format": ["reasoningFormat", "s"],
  "--reasoning-preserve": ["reasoningPreserve", "b"],
  "--embedding": ["embedding", "b"],
  "--pooling": ["pooling", "s"],
  "--log-format": ["logFormat", "s"],
};

function confirmImport() {
  const tokens = tokenizeCmd(importText.value);
  if (tokens.length === 0) {
    MessagePlugin.warning("请先粘贴启动命令");
    return;
  }
  const parsed: Record<string, any> = {};
  const unknown: string[] = [];
  let i = 0;
  // 第 0 个 token 是 exe 路径（llama-server.exe），跳过
  if (!tokens[0].startsWith("-")) i = 1;

  while (i < tokens.length) {
    const raw = tokens[i++];
    let flag = raw;
    let inlineVal: string | null = null;
    if (raw.startsWith("--")) {
      const eq = raw.indexOf("=");
      if (eq > 2) {
        flag = raw.slice(0, eq);
        inlineVal = raw.slice(eq + 1);
      }
    }
    const entry = FLAG_MAP[flag];
    if (!entry) {
      // 未知参数：连同一个可能的值一起原样保留
      unknown.push(raw);
      const next = i < tokens.length ? tokens[i] : null;
      if (next && !next.startsWith("-")) {
        unknown.push(next);
        i++;
      }
      continue;
    }
    const [key, kind] = entry;
    if (kind === "b") {
      parsed[key] = inlineVal == null ? true : ["true", "1", "on"].includes(inlineVal.toLowerCase());
      continue;
    }
    let value = inlineVal;
    if (value == null) {
      const next = i < tokens.length ? tokens[i] : null;
      if (next == null) continue;
      // 负数（如 --seed -1）是合法值；其余 - 开头的 token 视为值缺失，跳过该 flag
      if (next.startsWith("-") && !/^-?\d+(\.\d+)?$/.test(next)) continue;
      value = next;
      i++;
    }
    if (kind === "n") {
      const num = Number(value);
      parsed[key] = Number.isFinite(num) ? num : null;
    } else {
      parsed[key] = value;
    }
  }

  // 模板参数要求合法 JSON（llama-server 启动时校验），提前提示避免启动失败
  if (typeof parsed.chatTemplateKwargs === "string" && parsed.chatTemplateKwargs.trim()) {
    try {
      JSON.parse(parsed.chatTemplateKwargs);
    } catch {
      MessagePlugin.warning("「模板参数」不是合法 JSON（注意保留 key 的双引号），已原样填入，请修正后再启动");
    }
  }

  cfg.value = { ...defaultCfg(), ...parsed, extraArgs: unknown.join(" ") };
  activeProfile.value = ""; // 手动修改后与已保存方案解除关联
  importDlgVisible.value = false;
  MessagePlugin.success(
    `已回填 ${Object.keys(parsed).length} 个参数` +
      (unknown.length ? `，${unknown.length} 个未知参数已追加到「额外参数」` : "")
  );
}

// ---------- 参数说明 tooltip（label 文本 → 说明） ----------

/** 每个参数的说明：影响什么 / 推荐范围 / 与哪些参数冲突 */
const PARAM_INFO: Record<string, string> = {
  "模型文件 (-m)": "影响：要加载的 GGUF 模型，决定能力与显存/内存占用。\n推荐：7B 模型约 4-8GB；量化越低占用越小。\n冲突：无。",
  "模型别名 (--alias)": "影响：API 返回的 model 字段名，不影响推理。\n推荐：留空自动用文件名。\n冲突：无。",
  "监听地址 (--host)": "影响：HTTP 服务监听地址。\n推荐：本机用 127.0.0.1；需局域网访问用 0.0.0.0（注意安全）。\n冲突：无。",
  "端口 (--port)": "影响：HTTP 服务端口。\n推荐：8080，避免与常用端口冲突。\n冲突：与已占用端口冲突会启动失败。",
  "上下文长度 (-c)": "影响：单次可处理的 token 总量（含输入+输出），占用随长度线性增长。\n推荐：对话 4096-8192；长文 16k+（需大显存/内存）。0 = 模型上限。\n冲突：过大易 OOM，可与 --parallel 叠加占用。",
  "并行槽位 (--parallel)": "影响：同时处理的请求数，上下文按槽位均分。\n推荐：个人用 1；多人共享 2-4。\n冲突：与 -c 叠加，总 KV = c × parallel，显存占用成倍增长。",
  "GPU 层数 (-ngl)": "影响：多少层模型放进 GPU，越高越快。\n推荐：显存够就 99（全部 offload）；不够则逐步下调。\n冲突：CPU 版无效；与 --no-mmap/--mlock 影响加载行为。",
  "Flash Attention (-fa)": "影响：注意力计算优化，省显存、提速度。\n推荐：GPU 版保持 auto 或 on。\n冲突：旧显卡/某些量化组合可能数值异常，出错时改 off。",
  "多卡切分模式 (-sm)": "影响：多 GPU 时模型如何切分：layer 按层 / row 按行 / none 仅主卡。\n推荐：多卡 layer。\n冲突：单卡无效；与 -ts/-mg 相关。",
  "显存比例 (-ts)": "影响：多卡各自承担的比例，如 3,2。\n推荐：按显存大小比例填写。\n冲突：仅 -sm layer/row 生效；与 -mg 并用时注意主卡归属。",
  "主 GPU (-mg)": "影响：优先使用的 GPU 编号（从 0 起）。\n推荐：单卡填 0。\n冲突：-sm none 时只在此卡运行。",
  "KV Cache 量化 K (--cache-type-k)": "影响：K 缓存量化类型，q4/q8 可省一半以上 KV 显存。\n推荐：显存紧张用 q8_0；追求质量 f16。\n冲突：需 -fa on 才支持低 bit；与 --cache-type-v 配合，过低影响精度。",
  "KV Cache 量化 V (--cache-type-v)": "影响：V 缓存量化类型，作用同 K 侧。\n推荐：与 cache-type-k 保持一致。\n冲突：低 bit 量化需 -fa on；过低影响输出质量。",
  "统一 KV 缓冲 (--kv-unified)": "影响：K/V 共享统一量化缓冲，进一步省显存。\n推荐：KV 量化时开启。\n冲突：仅在 KV 量化（q4/q8）时有意义。",
  "推理线程 (-t)": "影响：CPU 推理线程数。\n推荐：物理核数或物理核数-2。\n冲突：超过逻辑核数反而变慢；GPU 全 offload 时影响很小。",
  "批处理线程 (-tb)": "影响：prompt 处理（批）阶段线程数。\n推荐：留空与 -t 相同。\n冲突：同 -t，勿超逻辑核数。",
  "逻辑批大小 (-b)": "影响：一次送入计算的 token 数，影响首 token 速度与占用。\n推荐：默认 2048 足够。\n冲突：过大增高显存/内存峰值，与 -ub 配套（b ≥ ub）。",
  "物理批大小 (-ub)": "影响：实际硬件单步批大小，显存峰值主要受它影响。\n推荐：默认 512；显存紧张降到 256/128。\n冲突：必须 ≤ -b。",
  "保留 Token (--keep)": "影响：上下文满时保护开头 N 个 token 不被丢弃。\n推荐：有固定 system prompt 时设其长度；-1 全保留（慎用，会 OOM）。\n冲突：仅发生上下文截断时起作用。",
  "NUMA 策略 (--numa)": "影响：多路（双路）服务器内存分配策略。\n推荐：普通台式机/单路忽略此项。\n冲突：仅 NUMA 平台有效。",
  "内存锁定 (--mlock)": "影响：禁止系统把模型内存换出到页面文件。\n推荐：内存充足时开，延迟更稳。\n冲突：内存不足会启动失败或拖垮系统；与 --no-mmap 组合时内存占用最大。",
  "禁用 mmap (--no-mmap)": "影响：启动时全量读入内存而非按需映射。\n推荐：模型在慢速盘（机械盘/网络盘）时开，加载后更快。\n冲突：增大内存峰值；与 --mlock 叠加需内存 ≥ 模型大小。",
  "显存自动适配 (--fit)": "影响：显存不足时自动降载（减小批/缓存）避免 OOM。\n推荐：保持 on。\n冲突：off 时完全按参数执行，易 OOM。",
  "加载模式 (--load-mode)": "影响：模型加载方式，mmap 内存映射 / default 常规读入。\n推荐：留空默认。\n冲突：与 --no-mmap/--mlock 语义相关，重复设置易混淆。",
  "跳过预热 (--no-warmup)": "影响：启动时不跑一次空推理，加快启动但首次请求变慢。\n推荐：一般不勾。\n冲突：无。",
  "温度 (--temp)": "影响：随机性，越高越有创意、越不稳定。\n推荐：0.6-0.8 通用；代码/事实类 0-0.3。\n冲突：--top-k/--top-p/--min-p 都是截断手段，联合起效；mirostat 开启时覆盖 top-p 逻辑。",
  "Top-K (--top-k)": "影响：只在概率前 K 个 token 中采样。\n推荐：40 左右；0 = 禁用。\n冲突：与 --top-p/--min-p 叠加截断；mirostat 开启时失效。",
  "Top-P (--top-p)": "影响：只在累计概率前 P 的 token 中采样（核采样）。\n推荐：0.9-0.95。\n冲突：与 --top-k/--min-p 叠加；mirostat 开启时失效。",
  "Min-P (--min-p)": "影响：丢弃概率低于峰值 P 倍的 token，动态截断。\n推荐：0.05-0.1，常配合高温度。\n冲突：与 top-k/top-p 并存时同时生效。",
  "重复惩罚 (--repeat-penalty)": "影响：惩罚近期重复 token，>1 抑制复读，过高语句不通顺。\n推荐：1.0-1.1；1.1 起。部分模型模板已内置处理，建议 1。\n冲突：与 --repeat-last-n 配套；与 presence/frequency 惩罚同类，叠加易过度惩罚。",
  "惩罚窗口 (--repeat-last-n)": "影响：重复惩罚回看的 token 数。\n推荐：64-256；-1 = 全上下文。\n冲突：仅 repeat-penalty ≠ 1 时有意义。",
  "出现惩罚 (--presence-penalty)": "影响：出现过的 token 统一加惩罚，鼓励话题多样。\n推荐：0；对话发散可 0.1-0.5。\n冲突：与 repeat-penalty 叠加，注意过度惩罚。",
  "频率惩罚 (--frequency-penalty)": "影响：按出现次数成比例惩罚，压制复读。\n推荐：0；复读严重可 0.1-0.5。\n冲突：同上，与 repeat-penalty 叠加。",
  "Mirostat (--mirostat)": "影响：自适应采样算法，动态控制困惑度。\n推荐：一般 0（关闭），用 top-p 即可。\n冲突：开启后覆盖 top-k/top-p 截断逻辑。",
  "Mirostat 学习率 (--mirostat-lr)": "影响：mirostat 调整速度。\n推荐：默认 0.1。\n冲突：仅 mirostat ≠ 0 生效。",
  "Mirostat 熵 (--mirostat-ent)": "影响：目标困惑度，越高越随机。\n推荐：默认 5.0。\n冲突：仅 mirostat ≠ 0 生效。",
  "随机种子 (--seed)": "影响：采样随机种子，相同 seed + 相同输入可复现输出。\n推荐：-1 = 随机；调试时固定。\n冲突：无。",
  "API Key (--api-key)": "影响：HTTP 请求需携带该 key 鉴权（Bearer）。\n推荐：暴露到局域网/公网时必设。\n冲突：无。",
  "HTTP 线程 (--threads-http)": "影响：处理 HTTP 请求的线程数，不影响推理速度。\n推荐：默认即可；高并发可加。\n冲突：勿与推理线程 (-t) 混淆。",
  "禁用内置 WebUI (--no-webui)": "影响：关闭 llama-server 自带网页聊天界面。\n推荐：纯 API 用途时关闭。\n冲突：无。",
  "指标端点 (--metrics)": "影响：暴露 /metrics（Prometheus 格式）监控指标。\n推荐：需要监控时开启。\n冲突：无。",
  "槽位端点 (--slots)": "影响：暴露 /slots 端点查看各槽位状态。\n推荐：调试并发时开启。\n冲突：建议与 --metrics 同开。",
  "Jinja 模板 (--jinja)": "影响：用模型自带的 chat template（Jinja）而非内置映射。\n推荐：新模型（Qwen3 等）建议开启，工具调用支持更好。\n冲突：老模型可能模板有 bug，异常时关闭。",
  "Chat 模板 (--chat-template)": "影响：强制指定聊天模板（内置名或内联 Jinja）。\n推荐：留空自动识别。\n冲突：与 --jinja/--chat-template-file 互斥优先级易混淆，三选一。",
  "Chat 模板文件 (--chat-template-file)": "影响：从 .jinja 文件读取聊天模板。\n推荐：留空自动。\n冲突：与 --chat-template/--jinja 三选一。",
  "日志格式 (--log-format)": "影响：日志输出格式，json 便于程序解析。\n推荐：界面查看用 text。\n冲突：无。",
  "多模态投影 (--mmproj)": "影响：视觉模型专用投影文件，使模型能看图。\n推荐：仅多模态模型（Qwen-VL 等）需要。\n冲突：纯文本模型设置了会报错。",
  "投影不上 GPU (--no-mmproj-offload)": "影响：mmproj 留在 CPU 运行。\n推荐：显存极紧时开。\n冲突：仅设置 --mmproj 后有意义。",
  "图像最少 Token (--image-min-tokens)": "影响：单张图至少占用的 token 数。\n推荐：默认即可。\n冲突：仅多模态模型有效；与 image-max-tokens 配套。",
  "图像最多 Token (--image-max-tokens)": "影响：单张图最多 token 数，直接限制图像显存占用。\n推荐：图片多/显存小可调低。\n冲突：需 ≥ min-tokens。",
  "思维链输出 (--reasoning)": "影响：控制推理模型的思维链开关。\n推荐：auto 即可；需要直接答案时 off。\n冲突：仅推理模型（DeepSeek-R1、Qwen3 等）有效。",
  "思维链格式 (--reasoning-format)": "影响：API 返回中思维链的封装格式。\n推荐：默认 deepseek。\n冲突：与 --reasoning-preserve 相关。",
  "保留思维链 (--reasoning-preserve)": "影响：不剥离回复中的思考内容。\n推荐：需要展示思考过程时开。\n冲突：开启后正文会混入 <think> 内容。",
  "模板参数 (--chat-template-kwargs)": "影响：向 chat template 传额外变量（JSON）。\n推荐：按模型文档填写。\n冲突：需模型模板支持对应变量；JSON 格式错误启动失败。",
  "Embedding 模式 (--embedding)": "影响：仅输出向量嵌入（用于检索/相似度），不生成文本。\n推荐：RAG/检索场景开启，且选 embedding 专用模型。\n冲突：开启后不能当聊天模型用；与 --pooling 配套。",
  "池化方式 (--pooling)": "影响：句向量如何从 token 向量汇聚。\n推荐：embedding 模型按模型卡推荐（常为 last/mean）。\n冲突：仅 --embedding 开启时有效。",
  "RoPE 缩放 (--rope-scaling)": "影响：上下文长度扩展方式（linear/yarn）。\n推荐：仅在超出模型训练长度时按模型文档设置。\n冲突：需与 freq-base/freq-scale 配套，乱设会明显降智。",
  "RoPE 频率基数 (--rope-freq-base)": "影响：位置编码基数，影响长程外推。\n推荐：留空用模型内置值。\n冲突：与 rope-scaling 配套。",
  "RoPE 频率缩放 (--rope-freq-scale)": "影响：位置频率缩放系数，<1 扩展上下文。\n推荐：留空或按 yarn 计算值。\n冲突：与 rope-scaling 配套。",
  "额外参数": "影响：原样追加到命令行的其他 llama-server 参数。\n推荐：仅放本界面未覆盖的参数。\n冲突：与界面参数重复设置时以后者为准，注意避免冲突。",
};

/** 给每个表单项标签注入 "?" 提示图标（title 原生 tooltip，轻量无依赖） */
function applyParamTips() {
  const page = document.querySelector(".page");
  if (!page) return;
  page.querySelectorAll<HTMLElement>(".t-form__item").forEach((item) => {
    if (item.querySelector(".param-tip")) return;
    const label = item.querySelector<HTMLElement>(".t-form__label");
    if (!label) return;
    const info = PARAM_INFO[label.textContent?.trim() ?? ""];
    if (!info) return;
    const tip = document.createElement("span");
    tip.className = "param-tip";
    tip.textContent = "?";
    tip.title = info;
    label.appendChild(tip);
  });
}

/** 采样中标签等动态 DOM 之外的表单渲染完成后补一次注入 */
function scheduleParamTips() {
  nextTick(applyParamTips);
  setTimeout(applyParamTips, 800);
}

// ---------- 运行参数监控 ----------

interface StatsSample {
  cpu_percent: number | null;
  ram_used_gb: number | null;
  ram_total_gb: number | null;
  gpu_util_percent: number | null;
  gpu_mem_used_mb: number | null;
  gpu_name: string | null;
  proc_cpu_percent: number | null;
  proc_mem_mb: number | null;
}

const baseline = ref<StatsSample | null>(null);
const current = ref<StatsSample | null>(null);
/** 基线是否已随启动锁定（锁定后轮询不再改动 baseline） */
const baselineLocked = ref(false);
let pollTimer: ReturnType<typeof setInterval> | undefined;

async function sampleStats(): Promise<StatsSample | null> {
  const pid = serverState.running ? serverState.pid : null;
  try {
    return await invoke<StatsSample>("sample_stats", { pid });
  } catch {
    return null;
  }
}

/** 手动重置基线（仅停止状态可点）：以当前时刻为新基线并解除锁定 */
async function captureBaseline(manual = false) {
  baseline.value = await sampleStats();
  if (manual) {
    baselineLocked.value = false;
    current.value = await sampleStats();
  }
}

function startPolling() {
  stopPolling();
  pollTimer = setInterval(async () => {
    if (!serverState.running) {
      stopPolling();
      return;
    }
    current.value = await sampleStats();
  }, 1000);
}

function stopPolling() {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = undefined;
  }
}

watch(
  () => serverState.running,
  (running) => {
    if (running) startPolling();
    else stopPolling();
  }
);

const fmtPct = (v?: number | null) => (v == null ? "—" : v.toFixed(0) + "%");
const fmtGb = (v?: number | null) => (v == null ? "—" : v.toFixed(1) + " GB");
const fmtMb = (v?: number | null) => {
  if (v == null) return "—";
  return v >= 1024 ? (v / 1024).toFixed(2) + " GB" : v.toFixed(0) + " MB";
};

interface MonRow {
  label: string;
  before: string;
  now: string;
  delta: string;
  cls: string;
}

function deltaOf(
  before: number | null | undefined,
  now: number | null | undefined,
  fmt: (n: number) => string
): { delta: string; cls: string } {
  if (before == null || now == null) return { delta: "—", cls: "" };
  const d = now - before;
  if (Math.abs(d) < 0.5) return { delta: "±0", cls: "" };
  return {
    delta: (d > 0 ? "+" : "") + fmt(d),
    cls: d > 0 ? "delta-up" : "delta-down",
  };
}

const monitorRows = computed<MonRow[]>(() => {
  const b = baseline.value;
  const c = current.value;
  return [
    {
      label: "系统 CPU",
      before: fmtPct(b?.cpu_percent),
      now: fmtPct(c?.cpu_percent),
      ...deltaOf(b?.cpu_percent, c?.cpu_percent, (n) => n.toFixed(0) + "%"),
    },
    {
      label: "系统内存",
      before: b ? `${fmtGb(b.ram_used_gb)} / ${fmtGb(b.ram_total_gb)}` : "—",
      now: c ? `${fmtGb(c.ram_used_gb)} / ${fmtGb(c.ram_total_gb)}` : "—",
      ...deltaOf(b?.ram_used_gb, c?.ram_used_gb, (n) => n.toFixed(1) + " GB"),
    },
    {
      label: "GPU 利用率",
      before: fmtPct(b?.gpu_util_percent),
      now: fmtPct(c?.gpu_util_percent),
      ...deltaOf(b?.gpu_util_percent, c?.gpu_util_percent, (n) => n.toFixed(0) + "%"),
    },
    {
      label: "GPU 显存占用",
      before: fmtMb(b?.gpu_mem_used_mb),
      now: fmtMb(c?.gpu_mem_used_mb),
      ...deltaOf(b?.gpu_mem_used_mb, c?.gpu_mem_used_mb, (n) => n.toFixed(0) + " MB"),
    },
    {
      label: "进程 CPU",
      before: "—",
      now: fmtPct(c?.proc_cpu_percent),
      delta: "—",
      cls: "",
    },
    {
      label: "进程内存",
      before: "—",
      now: fmtMb(c?.proc_mem_mb),
      delta: "—",
      cls: "",
    },
  ];
});

const gpuHint = computed(() => {
  if (baseline.value?.gpu_name) return `GPU 指标来源：${baseline.value.gpu_name}（nvidia-smi）`;
  if (sysInfo.value?.gpus.length) return "当前显卡无 nvidia-smi 数据源，GPU 指标不可用";
  return "未检测到独立显卡，GPU 指标不可用";
});

onMounted(async () => {
  await initServerListener().catch((e) => MessagePlugin.error("初始化服务监听失败: " + String(e)));
  await initRuntimeListener().catch((e) => MessagePlugin.error("初始化运行时监听失败: " + String(e)));
  // resolve_server_exe 返回 { path, source, tag, message }，按 exe_source 严格解析，
  // 两种场景零跨场景回退；失败时 message 含引导信息
  const resolved = await invoke<ResolvedExe>("resolve_server_exe").catch(() => null);
  exePath.value = resolved?.path || "";
  exeSource.value = resolved?.source || "";
  exeTag.value = resolved?.tag ?? null;
  exeMessage.value = resolved?.message || "";
  await loadModels();
  sysInfo.value = await invoke<SystemInfo>("get_system_info").catch(() => null);
  await loadProfiles();
  // 页面打开且服务未运行时先采一次基线；运行中则不覆盖（保持"启动前"语义）
  if (!serverState.running) await captureBaseline();
  if (serverState.running) startPolling();
  scheduleParamTips();
});
</script>

<style scoped>
.hint {
  margin-left: 8px;
  color: var(--td-text-color-placeholder, #999);
  font-size: 12px;
}

/* 左配置 + 右预览/本机配置双栏布局：窄窗口自动退化为上下堆叠 */
.config-row {
  display: flex;
  align-items: flex-start;
  gap: 16px;
}
.config-row .config-card {
  flex: 1;
  min-width: 0;
}
.preview-col {
  /* 随窗口宽度伸缩：最窄 400px 保底，默认约占 1/3，最宽 640px */
  width: clamp(400px, 33vw, 640px);
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 16px;
  position: sticky;
  top: 16px;
}

.preview-cmd {
  font-family: Consolas, "JetBrains Mono", "Courier New", monospace;
  font-size: 12.5px;
  line-height: 1.7;
  color: var(--td-text-color-primary, #333);
  background: var(--td-bg-color-secondarycontainer, #f3f3f3);
  border: 1px solid var(--td-component-stroke, #e7e7e7);
  border-radius: 6px;
  padding: 12px;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 52vh;
  overflow-y: auto;
  overflow-x: hidden;
  user-select: text;
  cursor: text;
}
.preview-cmd.is-empty {
  color: var(--td-text-color-placeholder, #999);
}

.preview-meta {
  margin-top: 8px;
  font-size: 12px;
  color: var(--td-text-color-placeholder, #999);
  text-align: right;
}

/* 本机配置信息 */
.sys-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.sys-row {
  display: flex;
  gap: 12px;
  font-size: 13px;
  line-height: 1.5;
}
.sys-label {
  flex: none;
  width: 72px;
  color: var(--td-text-color-secondary, #666);
}
.sys-value {
  flex: 1;
  min-width: 0;
  word-break: break-all;
  color: var(--td-text-color-primary, #333);
}
.sys-gpu + .sys-gpu {
  margin-top: 4px;
}
.mono-val {
  font-family: Consolas, "Courier New", monospace;
  font-size: 12px;
}

.dlg-hint {
  margin-top: 4px;
  font-size: 12px;
  color: var(--td-text-color-placeholder, #999);
}

/* 参数说明 tooltip 图标（原生 title） */
:deep(.param-tip) {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  margin-left: 4px;
  border: 1px solid var(--td-component-stroke, #dcdcdc);
  border-radius: 50%;
  color: var(--td-text-color-placeholder, #999);
  font-size: 10px;
  line-height: 1;
  cursor: help;
  user-select: none;
  vertical-align: middle;
}
:deep(.param-tip:hover) {
  color: var(--td-brand-color, #0052d9);
  border-color: var(--td-brand-color, #0052d9);
}

@media (max-width: 1100px) {
  .config-row {
    flex-direction: column;
  }
  .config-row .config-card {
    width: 100%;
  }
  .preview-col {
    width: 100%;
    position: static;
  }
}
</style>
