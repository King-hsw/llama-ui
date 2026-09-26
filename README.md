# llama-ui

[![version](https://img.shields.io/badge/version-0.1.0--alpha.5-orange)](https://github.com/King-hsw/llama-ui/releases)
[![platform](https://img.shields.io/badge/platform-Windows%20x64-blue)](https://github.com/King-hsw/llama-ui/releases)
[![build](https://github.com/King-hsw/llama-ui/actions/workflows/release.yml/badge.svg)](https://github.com/King-hsw/llama-ui/actions/workflows/release.yml)

**llama.cpp 可视化启动器** —— 一个纯启动器，不是聊天界面。

用图形界面管理 llama.cpp 的完整生命周期：下载预编译包 → 选择 GGUF 模型 → 配置启动参数 → 拉起 `llama-server` → 实时查看日志。命令行参数从此不用背，鼠标点几下就能跑。

## 功能

### 🖥️ 服务管理
- 一键启动 / 停止 `llama-server`，展示运行状态、PID、端口、当前模型
- **一键打开 WebUI**：服务运行中可直接在浏览器打开 llama-server 内置页面（监听 `0.0.0.0` / `::` 时自动改用 localhost 访问；启用 `--no-webui` 时禁用并提示）
- 启动参数可视化配置，Rust 端拼装实际命令行
- 实时日志流：后端事件推送 + 前端 250ms 批量缓冲，高吞吐日志不卡界面（上限 2000 行）
- **运行状态面板**：接入 llama-server `/metrics` 端点，实时展示推理速度（输入 / 生成 TPS）、Token 统计、上下文与 KV 缓存用量、请求值 vs 实际生效配置对比、健康状态一览与最近 90 秒时间线；结构化解析日志，自动诊断 OOM、CUDA 错误、端口占用、上下文不足等问题
- 稳定性：应用无论正常退出还是崩溃，都会通过 Windows Job Object 自动终止 `llama-server` 子进程，不留孤儿进程占用显存

### 📦 模型管理
- 自动扫描模型目录，列出 GGUF 模型（名称 / 路径 / 大小）
- 选中模型供服务启动使用

### ⬇️ 下载与更新
- 从 GitHub Releases 下载 llama.cpp 预编译包，支持进度、实时速度（MB/s）、取消
- **硬件推荐**：自动探测本机 CPU / GPU，推荐匹配的后端版本
- 应用启动时自动检查 llama.cpp 新版本，弹窗提示（每次启动仅一次，不打扰）
- 双来源模式：`custom`（用户自备 llama.cpp 目录，只读）/ `managed`（托管下载，独占写入）
- **应用自更新**：基于 tauri-plugin-updater，从 GitHub Releases 检查新版本，验签（minisign 公钥）后静默安装并自动重启，设置页可手动检查、查看更新进度

### ⚙️ 设置
- 模型目录、下载镜像源配置
- 配置档案（profile）：为不同使用场景保存多套配置，增删改查
- 自动检查更新开关

## 技术栈

| 层 | 技术 |
|----|------|
| 前端 | Vue 3 + TypeScript + TDesign Vue Next（unplugin-vue-components 按需引入，组件与样式均按需打包） + Vue Router（hash 模式） |
| 桌面框架 | Tauri 2 |
| 后端 | Rust（20 个 Tauri command：进程管理 / 文件扫描 / 硬件探测 / 下载 / 运行时指标与诊断 / 配置持久化） |
| 打包 | NSIS（简体中文安装向导），GitHub Actions 自动构建发布 |

## 开发

```bash
# 安装依赖（前端）
npm install

# 开发模式（前端热更新 + Rust 调试构建）
npm run tauri dev

# 类型检查
npm run typecheck

# 构建安装包（NSIS，输出在 src-tauri/target/release/bundle/nsis/）
npm run tauri build
```

环境要求：Node.js ≥ 20、Rust stable（MSVC 工具链）、Windows 10/11。

## 版本说明

当前版本 `0.1.0-alpha.5`（semver 预发布）：功能已成型，但打包链路与安装体验尚未经充分验证，**不建议在生产环境依赖**。

- **alpha.5**：TDesign 组件库由全量引入重构为按需引入（`unplugin-vue-components` + 组件级样式自动注入），前端产物 JS 约 -41%、CSS 约 -49%（gzip 后整体约 -34%），加载更快、安装包更小
- 推送 `v*` tag 会触发 GitHub Actions 构建 NSIS 安装包与 updater 更新包（`latest.json` + 签名产物），自动发布 Release 并更新 `releases/latest` 端点，供应用内自动更新消费
- 版本节奏：`alpha`（内部验证）→ `beta`（外部可测）→ `0.1.0`（首个正式版）

## Roadmap

- [ ] 启动参数详尽覆盖 + 参数说明（对应 llama-server 全部命令行选项）
- [ ] 多模型批量 / 切换优化
- [x] ~~应用内自动更新~~（v0.1.0-alpha.2 已接入 tauri-plugin-updater）
- [ ] 安装包代码签名（消除 SmartScreen 警告）

## 许可证

MIT（LICENSE 文件待补充）
