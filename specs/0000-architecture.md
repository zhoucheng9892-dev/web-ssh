---
spec: 0000
title: 系统架构约束
status: active
owner: backend/src + frontend/src 全局
tags: [architecture, module-boundary, tech-stack, data-flow]
last_reviewed: 2026-08-05
---

# 系统架构约束

## 1. Context（背景）

Web SSH 是一个**基于浏览器的多会话 SSH 终端客户端**：后端 Rust（axum + russh）提供 HTTP API 与 WebSocket 桥接，前端 Vue 3（xterm.js + Element Plus）提供多 Tab 终端、SFTP 文件管理与多用户管理。生产构建为**单二进制**：前端静态资源经 rust-embed 嵌入后端，一个文件即可运行。

## 2. Goals（目标）

- 任何新增/重构必须保持「前端零外部依赖 + 单二进制部署」的产品形态。
- 所有跨模块数据流（HTTP / WebSocket / DB）必须维持本 spec 定义的边界，防止模块职责漂移。
- 安全边界（凭据不落明文、用户数据隔离）必须保持可审计、可验证。

## 3. Non-Goals（非目标）

- 不引入独立前端服务器（Nginx 等）作为必选组件；开发模式 Vite 代理仅限开发环境。
- 不引入 Redis / PostgreSQL 等外部存储；数据库层必须可仅凭 SQLite 平滑运行。
- 不实现 OAuth / LDAP / SSO 等第三方登录。
- 不实现 known_hosts 强校验（当前为 TOFU，见 0003 spec）。
- 不实现 SFTP 之外的文件协议（FTP / SMB 等）。

## 4. Constraints（约束）

### MUST

- **模块边界**：后端模块划分固定为 `auth`（认证与用户）、`connections`（连接 CRUD）、`terminal`（WS↔PTY 桥接）、`files`（SFTP）、`ssh`（russh 封装）、`db`（池 + 迁移 + 查询）、`crypto`（哈希/加密）、`config`（配置）、`captcha`、`error`、`state`。新功能必须归入现有模块，职责不明时先开 Open Question，禁止在 `main.rs` 中堆积业务逻辑。
- **数据库访问**：所有 SQL 必须经由 `db::models` 中的函数完成，禁止在 handler 内直接写 SQL；统一走 `db::with_conn(pool, ...)`。
- **错误处理**：所有 handler 必须返回 `AppResult<T>`（即 `Result<T, AppError>`），由 `error.rs` 统一映射为 `{"error": "<msg>"}` JSON；数据库/内部错误不得向客户端泄露内部细节（统一为 "database error" / "internal error"），客户端可见错误（4xx）需以 WARN 级别记日志。
- **认证中间层**：除 `GET /api/auth/status` 与 `GET /api/auth/captcha` 外，所有 `/api` 路由 MUST 通过 `AuthUser` / `RequireUser` / `AdminUser` 提取器鉴权；新路由默认加鉴权，显式豁免才可公开。
- **用户数据隔离**：所有 `connections` 查询、SFTP 会话缓存键 MUST 携带 `user_id` 过滤；(user_id, connection_id) 作为访问边界，禁止出现仅按 connection_id 的跨用户访问路径。
- **会话 Cookie**：Cookie 名固定 `webssh_session`，HttpOnly + SameSite=Lax；`Secure` 默认关闭（允许局域网 HTTP），仅在 TLS 反代后开启；Cookie path 与 context_path 一致。
- **路由前缀**：业务路由 MUST 挂在 `/api` 下；`/healthz` 保持公开返回 `"ok"`；SPA 回退到 index.html（未构建时返回 404 文案提示）。
- **请求体限制**：应用层 MUST 保持 `DefaultBodyLimit::disable()`（大文件上传依赖此约束），禁止恢复 axum 默认 2MB 上限。
- **SQLite 并发**：连接池 MUST 设置 `journal_mode=WAL`、`foreign_keys=ON`、`busy_timeout=5000`；会话存储与业务池共享同一 DB 文件。
- **迁移机制**：schema 变更 MUST 以 `migrations/NNNN_<name>.sql` 形式新增，并在 `db::run_migrations` 的迁移表中登记（`_migrations` 表保证只执行一次）；禁止修改已发布迁移文件的内容。
- **前端嵌入**：`FrontendAsset`（rust-embed `../frontend/dist`）MUST 保持为唯一静态资源来源；release 构建必须依赖前端构建产物。

### MUST NOT

- MUST NOT 将 SSH 凭据（密码/私钥）明文写入数据库、日志、响应体或前端可访问的任何位置（见 0002）。
- MUST NOT 在前端引入后端独立部署的假设（如同源 API 之外的 CORS 跨域调用）；开发代理仅存在于 Vite 配置。
- MUST NOT 绕过 `AppError` 直接返回裸 `StatusCode`（静态资源与重定向辅助除外）。
- MUST NOT 在业务代码中直接读写 `data/.env`（密钥管理只属于 `config.rs`）。

### SHOULD

- handler 保持薄层：解析 → 校验 → 调用 models/ssh/files 层 → 组装响应；复杂逻辑下沉到对应模块，便于 Verifier 按 spec 检查。
- 新增公开路由时同步更新对应领域 spec 的「路由清单」。
- 使用 `tracing` 结构化日志（`info!` / `warn!` / `debug!`），中文用户可见错误与英文内部错误风格与现有代码一致。

## 5. Decisions Made（已定决策）

- 技术栈锁定：axum 0.8 / russh 0.49 + russh-sftp 2.0 / rusqlite 0.37 + deadpool-sqlite 0.12 / tower-sessions 0.14 + rusqlite store / argon2 0.5 / aes-gcm 0.10；前端 Vue 3.5 + Vite 5 + TS 5.6 + Element Plus 2.8 + Pinia 2.2 + Vue Router 4.4 + xterm 5.5（fit / web-links / webgl）。
- 会话：`Expiry::OnInactivity`（TTL 默认 7 天），用户 id 存于 session `user_id` 键。
- 密码安全：前端 SHA-256（hex）→ 后端 Argon2id（随机 salt）双层哈希，明文不离开浏览器。
- 凭据加密：AES-256-GCM，32 字节主密钥，12 字节随机 nonce，密文+nonce 落库。
- 部署形态：Docker 多阶段（node:20 → rust:1 → debian:bookworm-slim），非 root 用户 + tini，数据卷持久化。
- 子路径部署：`WEBSSH_CONTEXT_PATH` 支持（`/webssh` 等），cookie path、API/WS/SPA 均随前缀。

## 6. Task Breakdown（任务分解）

改动跨多个领域时的标准切分（并行安全，文件不冲突为前提）：

1. **配置层**：改 `config.rs` 与 `.env.example`、README 配置表 → 独立任务。
2. **数据层**：改 `migrations/*.sql` + `db/models.rs` → 独立任务，先于业务层。
3. **业务层**：按领域（auth / connections / terminal / files）各拆一个任务，互不共享文件。
4. **前端**：`api/*.ts` 契约与后端同步改；视图层任务独立。
5. **验证**：Verifier 按各领域 spec 的验收标准逐条检查，重点核对路由、错误语义与安全边界。

## 7. Verification Criteria（验收标准）

- [ ] 后端 `cargo build --release` 通过，且产物为单二进制（无外部静态文件依赖）。
- [ ] 前端 `npm run build`（vue-tsc + vite）通过，产物被后端正确嵌入并可从 `/` 访问。
- [ ] 访问 `/healthz` 返回 `ok`；未登录访问任意业务 API 返回 401 `{"error":"unauthorized"}`。
- [ ] `db` 层迁移在空库上全量执行一次，重启后不重复执行（`_migrations` 表）。
- [ ] 未构建前端时访问 SPA 路由返回明确的 404 提示文案（非 500）。
- [ ] 跨用户访问验证：用户 A 无法读取/修改/删除用户 B 的连接（构造 B 的 id 请求 A 的接口 → 404）。
- [ ] 任意 5xx 响应体不含内部细节（无 SQL 语句、无堆栈、无路径）。

## 8. Open Questions（待定项）

- 是否引入 `known_hosts` 强校验（当前 TOFU 记录指纹，见 0003）。
- 会话 `Secure` 标志是否默认开启（依赖 TLS 反代场景的普及度）。
