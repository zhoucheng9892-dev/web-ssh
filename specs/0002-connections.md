---
spec: 0002
title: SSH 连接配置与凭据安全
status: active
owner: backend/src/connections + backend/src/crypto + db::models(connections)
tags: [connections, crud, encryption, credential, data-isolation]
last_reviewed: 2026-08-05
---

# SSH 连接配置与凭据安全

## 1. Context（背景）

用户持久化 SSH 连接档案（host / port / username / 认证方式 / 密码或私钥）。凭据在浏览器明文到达后端后立即用 AES-256-GCM 加密落库；所有读写严格按用户隔离。终端与文件模块通过 `resolve_secret` 解密凭据用于拨号。

## 2. Goals（目标）

- 连接档案 CRUD 完整可用，且凭据在任何环节都不以明文形态回显给客户端。
- 加密失败、密钥更换等场景给出可理解但不过度暴露的错误。
- 删除连接时同步清理该连接缓存的 SFTP 会话，避免悬挂句柄。

## 3. Non-Goals（非目标）

- 不做连接「分享 / 收藏 / 分组 / 标签」。
- 不做凭据的 known_hosts 关联校验（见 0003）。
- 不做 master key 轮换 / 重加密迁移（更换密钥 = 历史凭据不可解密，README 已声明）。

## 4. Constraints（约束）

### 路由清单

| 方法 | 路径 | 鉴权 | 说明 |
|------|------|------|------|
| GET    | /api/connections     | AuthUser | 当前用户全部连接（列表字段无 secret） |
| POST   | /api/connections     | AuthUser | 创建连接，`secret` 必填 |
| GET    | /api/connections/{id} | AuthUser | 详情（`{id,name,host,port,username,auth_type}` 白名单字段） |
| PUT    | /api/connections/{id} | AuthUser | 更新；`secret` 空/缺省 = 保留原凭据 |
| DELETE | /api/connections/{id} | AuthUser | 删除连接 + 清理 SFTP 缓存 |

### 输入校验（MUST）

- `name`、`host`、`username` trim 后非空，否则 400。
- `auth_type` 仅允许 `"password"` | `"key"`，其余 400（并 WARN 日志）。
- `port` 缺省 22；`secret` 在创建时 MUST 非空，否则 400「secret is required」。
- 更新时 `secret` 为空字符串或缺失 = 保留原密文，不重加密。

### MUST

- **加密存储**：`secret` 使用 `crypto::encrypt_secret(master_key, bytes)`（AES-256-GCM，32 字节主密钥，12 字节随机 nonce），返回 (ciphertext, nonce) 一并落库；读取时用 `decrypt_secret` 还原。
- **永不回显**：任何响应体（列表 / 详情 / 创建返回值）MUST NOT 包含 `encrypted_secret`、`iv`、`secret` 字段；详情响应字段白名单固定为 `{id, name, host, port, username, auth_type}`。
- **按用户隔离**：所有 SQL 均带 `WHERE ... user_id=?`；`GET/PUT/DELETE /{id}` 对不属于当前用户的 id MUST 返回 404（而非 403，避免泄露资源存在性）。
- **删除清理**：`DELETE` 成功后 MUST 从 `AppState.sftp` 缓存移除 `(user_id, id)` 键。
- **主密钥约束**：`master_key` 必须为 32 字节，否则启动失败或加解密报错；nonce 必须 12 字节。
- **更新语义**：更新返回 `{"ok": true}`，目标不存在或不属于该用户时返回 404。

### MUST NOT

- MUST NOT 将明文凭据写入日志（含 `tracing` debug 级别）、错误信息或任何响应。
- MUST NOT 在列表接口中返回凭据（哪怕是密文形式）——密文只允许在服务端内部流转。
- MUST NOT 允许跨用户访问连接档案（唯一例外：系统管理员并无查看用户连接的专门接口，保持现状）。

### SHOULD

- 创建成功后仅返回 `{"id": <id>}`，避免冗余回显。
- 加密失败（如密钥长度非法）映射为 500「internal error」而非暴露算法细节。

## 5. Decisions Made（已定决策）

- 加密算法：aes-gcm 0.10 `Aes256Gcm`，`OsRng` 生成 12 字节 nonce；密文 + nonce 分别存 `encrypted_secret`、`iv` 列。
- 表结构（迁移 0001）：`connections(id, user_id FK CASCADE, name, host, port DEFAULT 22, username, auth_type, encrypted_secret BLOB, iv BLOB, last_used_at, created_at)`，`idx_connections_user` 索引。
- `last_used_at` 由 terminal/files 模块在拨号成功后更新（`touch_last_used`），connections 模块自身不维护。
- 详情接口实现方式：先按 user_id 拉全量列表再过滤（当前实现），数据量小可接受；后续可优化为单条带 user_id 查询。

## 6. Task Breakdown（任务分解）

1. **数据层**：`db/models.rs` connections 查询族（list / insert / update / delete / get_secret / get_dial / touch_last_used）。
2. **加密层**：`crypto/mod.rs`（encrypt_secret / decrypt_secret 及其 roundtrip 单测）——已存在，改动时保持 API 不变。
3. **业务层**：`connections/mod.rs` 五个 handler + `validate`。
4. **前端**：`api/connections.ts` + `ConnectionsView.vue`。
5. **验证**：见第 7 节；重点做跨用户与脱敏断言。

## 7. Verification Criteria（验收标准）

- [ ] 创建连接（password 与 key 两种 auth_type）成功；DB 中 `encrypted_secret` 非空且不等于明文。
- [ ] `GET /api/connections` 与 `GET /api/connections/{id}` 响应体不含 `secret` / `encrypted_secret` / `iv` 字段（字符串断言）。
- [ ] 用户 B 访问用户 A 的连接 id：GET/PUT/DELETE 均返回 404。
- [ ] 创建时缺 `secret` → 400；`auth_type="token"` → 400；port 缺省为 22。
- [ ] 更新时省略 secret → 凭据不变（用旧凭据仍可 probe 成功）；提供新 secret → 凭据更新。
- [ ] 删除连接后：再次 GET 返回 404；`state.sftp` 中对应 `(user_id, id)` 缓存键不存在（可通过对同一连接再开 SFTP 验证重新拨号而非复用）。
- [ ] `crypto` 单测通过：roundtrip 一致、错误密钥解密失败。
- [ ] 换 master_key 后访问旧凭据返回 500 级错误且不泄露解密失败细节。

## 8. Open Questions（待定项）

- 详情接口是否改为「单条 SQL + user_id」实现（当前全量拉取再过滤，数据量大时有性能风险）。
- 是否需要管理员级别的「查看某用户连接」能力（当前刻意不做）。
