# tokimo-app-apple-music

Tokimo 桌面 OS 的 Apple Music 三方 app（多进程方案 3 形态：内嵌 axum + UDS）。

> 这是 [`tokimo-lab/tokimo`](https://github.com/tokimo-lab/tokimo) 主仓库的 submodule。本仓库可独立 clone / 编译，也可以在主仓 `apps/tokimo-app-apple-music` 路径作为 submodule 工作。

## 仓库结构

```
.
├── Cargo.toml              # workspace root；members = [".", "crates/rust-apple-music"]
├── src/                    # binary：axum app server + handlers (token / auth / proxy / audio …)
├── crates/
│   └── rust-apple-music/   # lib：HLS / ALAC / Widevine 解密栈，仅本 app 使用
├── ui/                     # 前端 bundle（Vite 独立打包，运行在主 server 的窗口里）
└── tokimo-app.toml         # app manifest（id / icon / window 类型 / binary 名）
```

## 编译

```bash
cargo build                     # 同时编译 binary + 内置 rust-apple-music crate
cd ui && pnpm install && pnpm build   # 前端 bundle，输出到 ui/dist/
```

主仓 `make dev` 已经在 `packages/rust-server/dev-run.sh` 里追加了 standalone 构建步骤，会用 `CARGO_TARGET_DIR=<主仓>/target` 把 binary 编译到主仓的共享 `target/debug/` 下，主 server `app_loader::resolve_binary` 会自动找到。

## BE 设计要点

- **无自己的 DB schema**：所有 user 配置（音乐用户 token、音质偏好）通过主 server 的
  `/openapi/user/preferences/*` 读写，本 binary 完全无 PostgreSQL 依赖
- **token 缓存仅在内存**：`MusicKit` 开发者 token + webplayback stream URL 走
  `OnceLock<RwLock<...>>`，进程重启即重建（合理，token 本来就是 1h TTL）
- **`rust-apple-music` crate**：HLS / ALAC / Widevine 解密栈，已内嵌到本仓库（原先在主仓
  `packages/rust-apple-music`，因为只被本 app 使用，2025-04 移过来）

## 路由（与原 server `/api/apps/apple-music/*` 1:1 对齐）

| Method | Path                      | 说明                              |
|--------|---------------------------|-----------------------------------|
| GET    | `/token`                  | 抓取 `MusicKit` developer token   |
| GET    | `/auth`                   | 查询当前 user 是否已登录          |
| POST   | `/auth`                   | 保存 `music-user-token`           |
| DELETE | `/auth`                   | 注销                              |
| GET    | `/quality`                | 读音质偏好                        |
| PUT    | `/quality`                | 写音质偏好                        |
| POST   | `/proxy`                  | Apple Music API 透传              |
| POST   | `/get-key`                | Widevine 解密 key                 |
| GET    | `/audio/{track_id}`       | 流式音频（含 Range 支持）         |
| GET    | `/audio-debug/{track_id}` | 调试管线                          |

server 端 `/api/apps/apple-music/<rest>` 透明 UDS 反代到本 sock 的 `/<rest>`。

## 鉴权链

```
浏览器 ──cookie SESSION_ID──▶ server /api/apps/apple-music/<rest>
                                  │ 透明 UDS 反代（cookie 整段透传 + 注入 x-tokimo-user-id）
                                  ▼
                             apple-music binary handler
                                  │ 把 cookie 透传给 reqwest
                                  ▼
                             server /openapi/user/preferences/...
```

## 运行环境（由主 server spawn 时注入）

| Env                     | 来源                                    |
|-------------------------|-----------------------------------------|
| `TOKIMO_BUS_SOCKET`     | broker UDS 路径                         |
| `TOKIMO_SERVER_URL`     | server HTTP base（默认 `http://127.0.0.1:5678`）|
| `TOKIMO_APP_ASSETS_DIR` | dev 模式覆盖嵌入的 UI 资源              |
| `RUST_LOG`              | 日志级别                                |

## UI

`ui/` 是 Vite 独立打包的 React bundle（运行在主 server 的窗口里）。本地开发：

```bash
cd ui && pnpm install && pnpm build      # 一次性产出 ui/dist/
# 或 pnpm dev — 进入 watch 模式自动重建
```

构建产物 `ui/dist/index.{js,css}` 会被主 server 反代到 `/api/apps/apple-music/assets/<file>`。

### 构建配置：`@tokimo/app-builder`

`ui/vite.config.ts` 只有 `defineTokimoApp()` 一行，完整的 library 模式 + externals
（react / react-dom / @tokimo/ui / @tokimo/sdk）由共享预设
[`@tokimo/app-builder`](https://github.com/tokimo-lab/tokimo)（主仓
`packages/tokimo-app-builder/`）提供。这些 external 由主 shell 在运行时通过
`<script type="importmap">` + `window.__TKM_DEPS__` 注入同一份实例，避免重复打包
React 引发 hooks 跨边界失效。

> 当前通过 `workspace:*` 在主 monorepo 内解析。脱离主仓独立开发的方案（git
> 依赖 / dev-only assets 注册接口）由 `@tokimo/app-builder` 后续阶段补齐。
