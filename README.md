# llama-ui

[![version](https://img.shields.io/badge/version-0.1.0--alpha.1-orange)](https://github.com/King-hsw/llama-ui/releases)
[![platform](https://img.shields.io/badge/platform-Windows%20x64-blue)](https://github.com/King-hsw/llama-ui/releases)
[![build](https://github.com/King-hsw/llama-ui/actions/workflows/build-windows.yml/badge.svg)](https://github.com/King-hsw/llama-ui/actions/workflows/build-windows.yml)

**llama.cpp 可视化启动器** —— 一个纯启动器，不是聊天界面。

用图形界面管理 llama.cpp 的完整生命周期：下载预编译包 → 选择 GGUF 模型 → 配置启动参数 → 拉起 `llama-server` → 实时查看日志。命令行参数从此不用背，鼠标点几下就能跑。

## 功能

### 🖥️ 服务管理
- 一键启动 / 停止 `llama-server`，展示运行状态、PID、端口、当前模型
- 启动参数可视化配置，Rust 端拼装实际命令行
- 实时日志流：后端事件推送 + 前端 250ms 批量缓冲，高吞吐日志不卡界面（上限 2000 行）

### 📦 模型管理
- 自动扫描模型目录，列出 GGUF 模型（名称 / 路径 / 大小）
- 选中模型供服务启动使用

### ⬇️ 下载与更新
- 从 GitHub Releases 下载 llama.cpp 预编译包，支持进度、实时速度（MB/s）、取消
- **硬件推荐**：自动探测本机 CPU / GPU，推荐匹配的后端版本
- 应用启动时自动检查 llama.cpp 新版本，弹窗提示（每次启动仅一次，不打扰）
- 双来源模式：`custom`（用户自备 llama.cpp 目录，只读）/ `managed`（托管下载，独占写入）

### ⚙️ 设置
- 模型目录、下载镜像源配置
- 配置档案（profile）：为不同使用场景保存多套配置，增删改查
- 自动检查更新开关

## 技术栈

| 层 | 技术 |
|----|------|
| 前端 | Vue 3 + TypeScript + TDesign Vue Next + Vue Router（hash 模式） |
| 桌面框架 | Tauri 2 |
| 后端 | Rust（19 个 Tauri command：进程管理 / 文件扫描 / 硬件探测 / 下载 / 配置持久化） |
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

当前版本 `0.1.0-alpha.1`（semver 预发布）：功能已成型，但打包链路与安装体验尚未经充分验证，**不建议在生产环境依赖**。

- 推送 `v*` tag 会触发 GitHub Actions 构建 NSIS 安装包并自动发布 Release（alpha 后缀自动标记为 pre-release）
- 版本节奏：`alpha`（内部验证）→ `beta`（外部可测）→ `0.1.0`（首个正式版）

## Roadmap

- [ ] 启动参数详尽覆盖 + 参数说明（对应 llama-server 全部命令行选项）
- [ ] 多模型批量 / 切换优化
- [ ] 应用内自动更新（Tauri updater，需签名密钥）
- [ ] 安装包代码签名（消除 SmartScreen 警告）

## 许可证

MIT（LICENSE 文件待补充）
