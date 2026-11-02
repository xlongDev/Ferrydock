# Ferrydock

面向 WorkBuddy 的多提供商 AI 网关。核心价值：**零 API key 使用 OpenCode 免费模型池**（与 OC Ferry 同源机制），同时可接入 OpenRouter / Ollama / 自定义 OpenAI 兼容端点。

- 网关监听 `127.0.0.1:41980`，对 WorkBuddy 暴露 OpenAI 兼容的 `/v1/chat/completions`、`/v1/models`，以及管理用的 `/admin/status`。
- 桌面端自带网关，无需单独起后端；也可作为纯服务端（`ferrydock-server`）独立运行。

## 组件

| 成员 | 说明 | 运行方式 |
| --- | --- | --- |
| `ferrydock-core` | 网关核心：provider 路由、模型目录、本地 OpenCode 运行时管理 | 库，被下面两者依赖 |
| `ferrydock-server` | 纯服务端二进制（无 GUI，可常驻/容器化） | `cargo run -p ferrydock-server` |
| `src-tauri` (`ferrydock`) | Tauri 桌面端，进程内拉起网关并渲染 React 面板 | `cargo run -p ferrydock` |

前端面板在 `src/`（React + Vite + Tailwind），构建产物 `src/dist` 由 Tauri 加载；纯服务端模式不托管前端，需用 WorkBuddy 或其他 HTTP 客户端访问 API。

## 快速开始

### 方式一：桌面端（推荐）

```bash
cd /Users/xiaolong/Documents/Project/ferrydock
cargo run -p ferrydock
```

- GUI 进程内自动拉起网关，窗口标题为 **Ferrydock**（不是 OpenFerry）。
- 首次启动会下载 OpenCode 平台二进制（约 63MB，npm 平台子包 `opencode-<platform>-<arch>`）并拉起 `opencode serve`，约 30–60 秒，期间面板在等 `/health`，属正常等待而非卡死。
- 就绪后 Status 显示 `connected`、Gateway `Running`；Models 页可见 `opencode-local` 的 8 个免费模型（beam-free / glm-5.3-flash-free / deepseek-v4.1-flash-free 等）+ OpenRouter 公开列表。

### 方式二：纯服务端

```bash
cargo run -p ferrydock-server -- --data-dir /path/to/data
```

- `--data-dir` 指定数据与配置目录（不存在则自动创建，并写入 `config.json`）。
- 不要与桌面端同时运行：两者都占 41980，会端口冲突。
- 验证：

```bash
# 模型总数（应含 opencode-local 的免费模型）
curl -s http://127.0.0.1:41980/v1/models | python3 -c "import sys,json;print(len(json.load(sys.stdin)['data']),'models')"

# 真机实测对话（需本机网络能达 OpenCode 免费推理后端）
curl -s -X POST http://127.0.0.1:41980/v1/chat/completions \
  -H 'content-type: application/json' \
  -d '{"model":"Ferrydock · deepseek-v4.1-flash-free [opencode-local]","messages":[{"role":"user","content":"say pong"}]}'
```

## 无 key 机制（opencode-local）

Ferrydock 复刻 OC Ferry 的免费模型路径：

1. 发现或下载**带 `opencode serve` 的真实二进制**——来自 npm 平台子包 `opencode-<platform>-<arch>`（如 `opencode-darwin-arm64`），而非 `opencode-ai` 安装器包装或 GitHub releases CLI。下载经 SHA512 校验 + gzip 解包。
2. 以随机端口 + `OPENCODE_SERVER_PASSWORD` + 隔离 XDG 目录启动 `opencode serve --pure`，轮询 `/global/health` 至就绪。
3. 以原生协议作为内网客户端访问免费池：`GET /provider` 列模型，`POST /session` + `POST /session/:id/message`（`agent:"oc-ferry-chat"`）。
4. 运行时以 `Arc<Mutex<Option<OpenCodeRuntime>>>` 共享：provider 每次请求实时解析当前端口，配套的看门狗每 5 秒探活，runtime 崩溃重拉后在 ≤5 秒内恢复（不会因端口变更卡在死端口）。

`opencode-local` 默认启用、优先级 0；`OpenCodeFree`（OpenCode Zen，外部需 key）默认禁用。

## 配置

配置文件位于数据目录下的 `config.json`（首次运行自动生成）。关键字段：

| 字段 | 默认 | 说明 |
| --- | --- | --- |
| `bind` / `port` | `127.0.0.1` / `41980` | 监听地址与端口 |
| `remove_on_exit` | `true` | 退出时清理本地下载的 opencode 运行时 |
| `use_system_proxy` / `proxy_url` | `false` / `None` | 仅二进制下载使用代理；loopback 通信强制不走代理 |
| `workbuddy_models_path` | `~/.workbuddy/models.json` | WorkBuddy 模型清单写入路径 |
| `providers[]` | 见 `config.rs` | 各 provider 的 `id` / `kind` / `enabled` / `priority` |

`provider.kind` 枚举（`kebab-case`）：`opencode-local`、`opencode-free`、`openrouter`、`ollama`、`custom`。配置示例：

```json
{
  "providers": [
    { "id": "opencode-local", "kind": "opencode-local", "enabled": true, "priority": 0 },
    { "id": "openrouter", "kind": "openrouter", "enabled": true, "priority": 2 },
    { "id": "ollama", "kind": "ollama", "enabled": false, "base_url": "http://127.0.0.1:11434" }
  ]
}
```

## 环境变量

| 变量 | 作用 |
| --- | --- |
| `FERRY_OPENCODE_PATH` | 指定本地 opencode 二进制路径，跳过自动下载（下载失败或离线时可用） |

## 排错

| 现象 | 原因 / 处理 |
| --- | --- |
| 桌面端卡在启动闪屏 | 多半是双击了旧版 `/Applications/OC Ferry.app`（内部叫 OpenFerry）。请运行新构建的 Ferrydock GUI，标题为 Ferrydock。 |
| `/v1/chat/completions` 报连接错误 | 免费推理后端 egress 不可达，或 runtime 正在重启。首启等待 30–60 秒；若在运行中被 `pkill`，看门狗 ≤5 秒自动恢复。 |
| opencode 下载失败 | 设 `FERRY_OPENCODE_PATH` 指向本机的 `opencode`（需含 `serve` 子命令，来自 npm 平台子包）。 |
| 端口冲突 | 桌面端与服务端不能同时跑，确认 41980 仅一个进程占用。 |

## 构建

依赖 Rust 工具链 + pnpm（前端）。

```bash
cargo build --workspace          # 全量构建（含 GUI 与 server）
cargo run -p ferrydock          # 跑桌面端
cargo run -p ferrydock-server -- --data-dir /tmp/fd # 跑服务端
```
