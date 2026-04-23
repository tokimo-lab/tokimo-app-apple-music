# tokimo-app-apple-music

Apple Music 多进程 app（方案 3 形态：内嵌 axum + UDS）。

## BE 设计要点

- **无自己的 DB schema**：所有 user 配置（音乐用户 token、音质偏好）通过主 server 的
  `/openapi/user/preferences/*` 读写，本 binary 完全无 PostgreSQL 依赖
- **token 缓存仅在内存**：`MusicKit` 开发者 token + webplayback stream URL 走
  `OnceLock<RwLock<...>>`，进程重启即重建（合理，token 本来就是 1h TTL）
- **依赖 `rust-apple-music` crate**：复用 tokimo 主仓库已有的 HLS / ALAC 解密栈

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

UI 实装放在子任务 5；当前 `ui/dist/index.js` 仅占位，避免 rust-embed 编译失败。
