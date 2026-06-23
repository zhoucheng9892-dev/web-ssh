# Web SSH

一个基于浏览器的多会话 SSH 终端客户端。后端 Rust（axum + russh），前端 Vue 3（xterm.js + Element Plus）。

## 功能

- **多会话终端**：浏览器中开多个 tab，每个 tab 一条独立 SSH shell，支持窗口尺寸自适应（resize）。
- **SFTP 文件管理**：浏览远程目录、上传、下载（流式）、删除、新建目录。
- **SSH 登录方式持久化**：密码 或 SSH 私钥，AES-256-GCM 加密后存入数据库。
- **多用户 + 数据隔离**：Web 端账号体系（用户名/密码 + Cookie 会话），每个用户的连接配置互不可见。
- **单二进制部署**：release 构建把前端静态资源嵌入后端二进制，一个文件即可运行。

## 技术栈

| 层 | 选型 |
|----|------|
| 后端框架 | axum 0.8（HTTP + WebSocket） |
| SSH 客户端 | russh 0.49 + russh-sftp |
| 数据库 | SQLite（rusqlite + deadpool-sqlite），可平滑切换 |
| 会话 | tower-sessions（Cookie）+ tower-sessions-rusqlite-store |
| 密码哈希 | Argon2id |
| 凭据加密 | AES-256-GCM |
| 前端 | Vue 3 + Vite + TypeScript |
| 终端组件 | @xterm/xterm（fit / web-links / webgl） |
| UI | Element Plus |
| 状态/路由 | Pinia + Vue Router |

## 目录结构

```
web-ssh/
├── backend/                # Rust 后端
│   ├── Cargo.toml
│   ├── migrations/         # 建表 SQL
│   └── src/
│       ├── main.rs         # 启动 + 路由 + 嵌入静态资源
│       ├── config.rs       # 环境变量 / 密钥自动生成
│       ├── crypto.rs       # argon2 + aes-gcm
│       ├── models.rs       # 数据访问层
│       ├── auth.rs         # setup/login/logout/me
│       ├── connections.rs  # SSH 连接配置 CRUD
│       ├── files.rs        # SFTP 文件操作
│       ├── terminal.rs     # WebSocket <-> SSH PTY 桥接
│       ├── ssh.rs          # russh 连接封装
│       └── extractors.rs   # AuthUser 等
└── frontend/               # Vue 3 前端
    └── src/
        ├── views/          # Login / Setup / Terminal / Connections / Files
        ├── components/     # AppLayout
        ├── composables/    # useTerminal（xterm + WebSocket）
        ├── api/            # axios 封装
        ├── stores/         # Pinia
        └── router/
```

## 快速开始（开发模式）

前置：Rust（rustup）、Node.js 18+。

```bash
# 1. 后端（监听 127.0.0.1:3000）
cd backend
cargo run

# 2. 前端（监听 127.0.0.1:5173，代理 /api 到后端）
cd ../frontend
npm install
npm run dev
```

打开 http://127.0.0.1:5173 。首次访问会进入「初始化」页面创建管理员账号，之后即可添加 SSH 连接、打开终端、管理文件。

## 生产构建（单二进制）

```bash
# 1. 编译前端到 frontend/dist
cd frontend && npm install && npm run build && cd ..

# 2. 编译后端（rust-embed 会把 frontend/dist 嵌入二进制）
cd backend && cargo build --release

# 3. 运行（无需前端文件）
./target/release/web-ssh-backend
```

访问 http://127.0.0.1:3000 。

## Docker 部署

项目自带 `Dockerfile`（多阶段构建）与 `docker-compose.yml`，一条命令即可构建并运行：

```bash
docker compose up -d --build
```

- 镜像为单二进制运行（前端已嵌入），基于 `debian:bookworm-slim`，非 root 用户。
- 数据库与生成的密钥（`.env`）持久化到命名卷 `webssh-data`（容器内 `/app/data`）。
- 端口映射到宿主机 `3000`。

构建完成后访问 **http://localhost:3000**。

常用命令：

```bash
docker compose logs -f          # 查看日志
docker compose down             # 停止并删除容器（保留数据卷）
docker compose down -v          # 同时删除数据卷（⚠️ 会丢失已加密的 SSH 凭据）
```

可在 `docker-compose.yml` 的 `environment` 段调整 `WEBSSH_SESSION_TTL_SECS`、`WEBSSH_TERMINAL_IDLE_TIMEOUT_SECS` 等（完整配置见下表）。

## 配置

所有配置通过环境变量或 `backend/.env` 文件设置。**首次运行会自动生成 `WEBSSH_MASTER_KEY` 和 `WEBSSH_SESSION_KEY` 并写入 `.env`**，请妥善保管这两个值（更换后已存的 SSH 凭据将无法解密）。

| 变量 | 默认 | 说明 |
|------|------|------|
| `WEBSSH_HOST` | `127.0.0.1` | 监听地址 |
| `WEBSSH_PORT` | `3000` | 监听端口 |
| `WEBSSH_CONTEXT_PATH` | `（空）` | 子路径部署，如 `/webssh`。空 = 根部署；非空时整个应用（含 `/api`、`/healthz`）都挂在该前缀下，根路径 `/` 会 302 跳转。环境变量优先级高于配置文件 |
| `WEBSSH_DATABASE_URL` | `sqlite://webssh.db?mode=rwc` | SQLite 文件 |
| `WEBSSH_MASTER_KEY` | （自动生成） | 32 字节，base64，用于加密 SSH 凭据 |
| `WEBSSH_SESSION_KEY` | （自动生成） | 64 字节，base64，签名会话 Cookie |
| `WEBSSH_SESSION_TTL_SECS` | `604800` | 会话有效期（秒） |
| `WEBSSH_TERMINAL_IDLE_TIMEOUT_SECS` | `0` | 终端空闲超时（秒）。`0` = 不因空闲断开（默认）；设为正数则终端无数据交互超过该时长后断开。无论取值如何，后端每 30 秒发送一次心跳保活 |

## 安全说明

- **Web 账号密码采用双层哈希**：前端用浏览器原生 Web Crypto API 对密码做 `SHA-256`（hex）后再传输，后端收到后用 **Argon2id**（带随机 salt 的慢哈希）再次哈希存储。明文密码从不离开浏览器、不进日志；数据库里只存 Argon2id 哈希，即便泄漏也无法反推。
- SSH 凭据（密码/私钥）在浏览器 → 后端传输后，**使用 AES-256-GCM 加密落库**，主密钥仅存在于服务端，从不返回给前端。`GET /api/connections/{id}` 只回显非敏感字段。
- 服务器公钥校验目前采用 **TOFU（首次信任）** 策略并记录指纹；后续可扩展为 `known_hosts` 强校验。
- 数据隔离：所有 `connections` 查询均带 `user_id` 过滤，SFTP 会话缓存同样以 `(user_id, connection_id)` 为键。
- 默认 Cookie 关闭 `Secure`（便于局域网 HTTP 使用）；若部署在 TLS 反向代理之后，请在 `main.rs` 中开启 `with_secure(true)`。
