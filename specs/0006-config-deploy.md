---
spec: 0006
title: 配置、构建与部署
status: active
owner: backend/src/config + Dockerfile + docker-compose.yml + .env.example
tags: [config, env, docker, single-binary, context-path]
last_reviewed: 2026-08-05
---

# 配置、构建与部署

## 1. Context（背景）

应用所有配置来自环境变量（或 `data/.env` 文件）。**首次运行自动生成** `WEBSSH_MASTER_KEY`（32B）与 `WEBSSH_SESSION_KEY`（64B）并持久化到 `data/.env`。生产形态为单二进制：前端构建产物被 rust-embed 嵌入后端，Docker 镜像仅含二进制 + 运行时依赖。支持 `WEBSSH_CONTEXT_PATH` 子路径部署。

## 2. Goals（目标）

- 零配置即可首次运行（密钥自动生成），但密钥一旦生成必须稳定持久（更换 = 历史凭据不可解密）。
- 同一份二进制 + 同一份前端构建产物可在任意 context path 下部署。
- Docker 构建可缓存、镜像最小化、非 root 运行、数据持久化。

## 3. Non-Goals（非目标）

- 不做配置热加载 / 动态重载（配置仅启动时读取）。
- 不做多实例横向扩展（SQLite 文件型存储，单实例语义）。
- 不做密钥轮换工具链（手工更换 + 数据不可解密是既定语义）。

## 4. Constraints（约束）

### 环境变量契约（MUST）

| 变量 | 默认 | 校验/约束 |
|------|------|-----------|
| WEBSSH_HOST | 127.0.0.1 | 监听地址；Docker 内为 0.0.0.0 |
| WEBSSH_PORT | 3000 | u16 |
| WEBSSH_CONTEXT_PATH | （空） | 规范化：去首尾空白/尾斜杠、补前导 `/`、折叠连续 `/`；`""`/`"/"` → 根部署 |
| WEBSSH_DATABASE_URL | `sqlite://<data_dir>/webssh.db?mode=rwc` | 仅支持 `sqlite:` 前缀，解析出文件路径 |
| WEBSSH_DATA_DIR | `./data`（相对 CWD） | 数据目录（DB 与 .env 所在地），启动时 MUST 自动创建 |
| WEBSSH_MASTER_KEY | 自动生成 | base64 编码、解码后 MUST 为 32 字节，否则启动失败 |
| WEBSSH_SESSION_KEY | 自动生成 | base64 编码、解码后 MUST 为 64 字节，否则启动失败 |
| WEBSSH_SESSION_TTL_SECS | 604800 | i64，会话滑动过期时长 |
| WEBSSH_TERMINAL_IDLE_TIMEOUT_SECS | 0 | u64；0 = 禁用空闲断开 |

- **优先级**：进程环境变量 > `data/.env` 文件 > 默认值；`.env` 只在变量未设置时注入（不覆盖已存在的环境变量）。
- **密钥生成**：缺失/长度非法时生成随机字节写入 `data/.env`（替换同名旧行）；`WEBSSH_MASTER_KEY` 与 `WEBSSH_SESSION_KEY` 必须 base64 编码存储。
- **密钥不可变**：运行中不得重新生成或轮换密钥；换 key 后已存 SSH 凭据将无法解密（README 明示该语义，spec 0002 承袭）。
- **子路径语义**：`context_path` 非空时，`/api`、`/healthz`、SPA、会话 Cookie path 全部挂在该前缀下；裸 `/` 与 `{context_path}/` 302 重定向到 `{context_path}`（axum nest 精确匹配陷阱的规避）；`index.html` 注入 `<base href="{ctx}/">` 与 `window.__CONTEXT_PATH__`。
- **静态资源缓存**：带 hash 的 Vite 资产 `Cache-Control: public, max-age=31536000, immutable`；`index.html` 永不缓存（`no-cache, no-store, must-revalidate`，其注入内容随配置变化）。

### 构建（MUST）

- 前端 `npm ci`（锁文件驱动）→ `npm run build`（vue-tsc + vite）→ `dist/`。
- 后端 `cargo build --release`，`rust-embed` 指向 `../frontend/dist`；`[profile.release]` 保持 `lto="thin"`、`codegen-units=1`、`strip=true`。
- Docker 多阶段：`node:20-bookworm-slim`（前端）→ `rust:1-bookworm`（后端，先 COPY manifest + stub `cargo fetch` 缓存依赖层）→ `debian:bookworm-slim`（运行时）。
- 运行时镜像 MUST 安装 `ca-certificates`（TLS）、`openssh-client`（SFTP 依赖，sftp-server 客户端路径）、`tini`（信号转发 + 僵尸回收）；以非 root 用户（uid 1000）运行，数据目录 `/app/data` 属主为该用户。

### 部署（MUST）

- `docker-compose.yml` 固定端口映射 `3000:3000`、命名卷 `webssh-data` 挂载 `/app/data`（DB + .env 持久化）；环境变量覆盖默认值（如 `WEBSSH_CONTEXT_PATH: /webssh`）。
- 容器内 `WEBSSH_HOST=0.0.0.0`（端口暴露由 compose 负责）；`ENTRYPOINT ["/usr/bin/tini","--"]`。
- 数据卷删除警告：`docker compose down -v` 会同时清除已加密凭据与密钥（README 已标注 ⚠️）。

### MUST NOT

- MUST NOT 将密钥写入镜像层、compose 文件明文、或任何非 `data/.env` 的位置。
- MUST NOT 在 `main.rs` / handler 中读取环境变量（一律经 `config.rs`）。
- MUST NOT 修改已发布迁移文件（见 0000）或改变 `.env` 解析行为而不更新本 spec。

### SHOULD

- `.env.example` 与 README 配置表保持同步（新增变量必须三处同步：config.rs / .env.example / README）。
- 日志级别经 `RUST_LOG` 控制（默认 `info,web_ssh_backend=debug`），生产建议 `info`。

## 5. Decisions Made（已定决策）

- 密钥存储：`data/.env`（自制极简 dotenv 解析器，无第三方依赖；值 trim 并去可选引号）。
- Cookie：默认 `Secure=false`（局域网 HTTP 场景），`SameSite=Lax`、`HttpOnly`；TLS 反代后需手工开启 `with_secure(true)`（main.rs 注释明示）。
- SPA 路由：Hash 模式（见 0005），故静态服务只需正确回退到 index.html，无需 history fallback 规则。
- 数据目录解析：`WEBSSH_DATA_DIR` 优先，否则 CWD 下 `data/`；DB 默认路径基于 data_dir 相对解析（用户无需关心绝对路径）。

## 6. Task Breakdown（任务分解）

1. **配置层**：`config.rs`（新增/变更变量）+ `.env.example` + README 配置表（三处同步）。
2. **构建链**：`Dockerfile` / `docker-compose.yml`（构建缓存、依赖、镜像瘦身）。
3. **子路径**：`main.rs`（build_router / static_handler / index_response）+ 前端 `context.ts`。
4. **验证**：三种部署形态（裸机单二进制 / Docker / 子路径）各跑一遍验收。

## 7. Verification Criteria（验收标准）

- [ ] 首次运行：空目录启动自动生成 `data/.env`（含两个 key），重启后 key 不变；删除 `.env` 重新生成后旧凭据解密失败（500 级错误，非崩溃）。
- [ ] `WEBSSH_MASTER_KEY` 长度错误（如 16 字节 base64）时启动失败并给出明确错误。
- [ ] 根部署与 `/webssh` 部署：登录、终端 WS、文件上传下载、退出登录均正常；Cookie Path 为 `/` 与 `/webssh/`。
- [ ] `http://host/webssh/` 与 `http://host/webssh` 均 302 到 `http://host/webssh`（无 404 陷阱）；裸 `/` 302 到 `/webssh`。
- [ ] index.html 响应含 `<base href="/webssh/">` 与 `window.__CONTEXT_PATH__="/webssh"`，且 `Cache-Control` 为 no-store 系。
- [ ] Docker：`docker compose up -d --build` 一次成功；容器内非 root（`id -u` = 1000）；重启容器数据仍在；`docker compose down -v` 后数据清除。
- [ ] 前端 hash 资产响应头为 `immutable` 缓存；未构建前端时后端返回明确的 404 提示。
- [ ] 环境变量优先级：设置 `WEBSSH_PORT=4000` 时 `.env` 中的 3000 不生效。

## 8. Open Questions（待定项）

- 是否提供密钥轮换辅助脚本（重加密所有凭据）——当前明确不支持。
- 是否支持 systemd / 反向代理（nginx/caddy）示例配置——README 仅文字说明。
