---
spec: 0004
title: SFTP 文件管理
status: active
owner: backend/src/files + frontend/src/api/files.ts
tags: [sftp, files, upload, download, resume]
last_reviewed: 2026-08-05
---

# SFTP 文件管理

## 1. Context（背景）

通过 SFTP 子系统管理远端文件：目录浏览、下载（流式）、上传（multipart 流式 + 分块续传）、删除、建目录。SFTP 会话按 `(user_id, connection_id)` 缓存于 `AppState.sftp`，同一连接复用一条子系统通道；`Mutex` 串行化同一通道上的操作（SFTP 为请求/响应模型，串行安全）。

## 2. Goals（目标）

- 大文件上传/下载不经过内存缓冲，全程流式；分块上传支持中断后续传。
- 并发访问同一连接（多 Tab / 多请求）不破坏 SFTP 通道协议状态。
- 错误（权限、路径不存在、磁盘满）以可读文案返回，不出现 500。

## 3. Non-Goals（非目标）

- 不做重命名 / 移动 / 复制 / 权限修改（chmod）/ 软链操作。
- 不做目录树递归删除（仅空目录）。
- 不做跨连接文件复制、本地磁盘缓存。
- 不做 WebDAV / FTP 等替代协议。

## 4. Constraints（约束）

### 路由清单

| 方法 | 路径 | 鉴权 | 说明 |
|------|------|------|------|
| GET    | /api/files/list?connection_id=&path=   | AuthUser | 列目录；`path` 缺省 `"."`；返回 `{path, entries:[{name,is_dir,size,modified}]}`，目录优先、名称升序 |
| GET    | /api/files/download?connection_id=&path= | AuthUser | 流式下载；`Content-Disposition: attachment` |
| POST   | /api/files/upload?connection_id=&path=  | AuthUser | multipart 上传（一个或多个文件），返回 `{"uploaded":[target,...]}` |
| GET    | /api/files/upload-status?connection_id=&path=&filename= | AuthUser | 返回 `{exists, size}`，用于断点续传探测 |
| POST   | /api/files/upload-chunk?connection_id=&path=&filename=&offset=N | AuthUser | 单块写入（raw body），返回 `{"offset": 新文件大小}` |
| POST   | /api/files/mkdir?connection_id=&path=   | AuthUser | 建目录，返回 `{"ok": true}` |
| DELETE | /api/files?connection_id=&path=         | AuthUser | 删文件或空目录，返回 `{"ok": true}` |

### MUST

- **会话缓存**：SFTP 会话 MUST 以 `(user_id, connection_id)` 为键缓存；缓存未命中时现场拨号（`ssh::connect` + `open_sftp`）并 `touch_last_used`；任一用户的请求 MUST NOT 触碰其他用户缓存的会话。
- **锁语义**：短临界区（open/create/stat 等单请求操作）在缓存锁内执行；长操作（流式下载/上传 body 写入）MUST 尽快释放锁——`File` 自身持有 channel 的 Arc，可在锁外继续读写。
- **路径拼接**：远端路径统一用 `/` 拼接（`join_remote`）：空/`.` 目录 → 直接文件名；否则 `dir/name`（注意尾斜杠）。禁止使用平台分隔符。
- **下载**：`path` 必填（空 → 400）；64 KiB 块读入 mpsc channel 流式下发，headers 先发、锁先释放；`filename` 取路径 basename，`Content-Disposition` 中需剔除 `"` 防注入。
- **multipart 上传**：每个 field 的 `file_name()`（回退 `name()`）作为远端文件名；`sftp.create()` 在锁内完成，随后锁外 `write_all + flush` 流式写入；响应列出全部实际写入的目标路径。
- **分块上传**：`open_with_flags(CREATE | WRITE)`（**无 TRUNCATE**，保留已写内容）→ `seek(offset)` → `write_all` → `flush` → 返回 `offset + len`；前端按序调用，中断后可通过 upload-status 从远端实际大小续传。
- **删除**：先 `remove_file`，失败则回退 `remove_dir`（仅空目录）；两者都失败 → 400，错误文案同时包含两者原因。
- **错误映射**：所有 SFTP 操作错误 MUST 映射为 `AppError::BadRequest`（400）且文案含具体操作（如 `read_dir: ...`），不得出现 500 或裸错误。
- **会话失效处理**：缓存中的 `Option<SftpSession>` 若为 `None`（会话已关）必须视为缓存未命中重新建立（当前实现持 `Mutex<Option<...>>` 结构，保持该语义）。

### MUST NOT

- MUST NOT 在锁持有时执行可能长时间阻塞的 I/O（大文件读写），避免串行化整个连接的并发操作。
- MUST NOT 使用 `std::path` 的本地路径语义拼接远端路径（`PathBuf::join` 等），必须走 `join_remote`。
- MUST NOT 允许 `path` 为空串（除 list/upload 的缺省 `"."` 外均显式校验）。
- MUST NOT 缓存跨用户/跨连接的会话，或把 SFTP 会话序列化到响应中。

### SHOULD

- 目录列表 `modified` 使用 RFC3339（UTC）输出，前端本地化显示。
- 上传队列由前端串行执行（同一时间一个文件），避免 SFTP 通道被并发写打乱。
- 分块大小由前端常量 `CHUNK_SIZE = 8 MiB` 控制；调整需同时考虑反代 body 上限。

## 5. Decisions Made（已定决策）

- 缓存结构：`AppState.sftp: Arc<RwLock<HashMap<(i64,i64), CachedSftp>>>`，`CachedSftp = Arc<Mutex<Option<SftpSession>>>`。
- 上传双通道：小文件/普通场景走 `upload`（multipart，XHR 带进度）；大文件/可中断场景走 `upload-chunk`（前端默认使用分块上传，见 0005）。
- 下载不设大小上限，浏览器端通过临时 `<a download>` 触发。
- 删除语义：文件优先、目录回退，不递归。
- SFTP 性能参数（channel 包大小/窗口）继承 0003 的 `ssh::dial` 配置（1 MiB / 4 MiB）。

## 6. Task Breakdown（任务分解）

1. **后端**：`files/mod.rs`（list / download / upload / upload_status / upload_chunk / mkdir / remove / get_sftp / join_remote）。
2. **前端**：`api/files.ts`（filesApi + uploadWithProgress + uploadChunked + resume 状态）+ `FilesView.vue`（目录、队列、进度、断点续传 UI）。
3. **验证**：真实 SFTP 目标（本机 sshd 即可）覆盖全操作 + 中断恢复。

## 7. Verification Criteria（验收标准）

- [ ] 列表：目录在前且按名称升序；`modified` 为 RFC3339 或 null；`path` 回显请求值。
- [ ] 下载：文件内容与远端一致（md5 对比）；1GB+ 文件内存占用平稳（无整文件缓冲）；断点续传下载场景行为正确（HTTP 层面无 Range 依赖，纯流式）。
- [ ] 上传（multipart）：多文件一次上传全部落盘；空文件正常。
- [ ] 分块上传：8 MiB 分块 + 偏移写正确；**中断后重传**：先传 1/3 中断 → 再调用同文件 → upload-status 返回已有大小 → 仅续传剩余块，最终文件 md5 一致。
- [ ] 续传跨会话：分块上传中关闭连接（或重启后端），再次上传同一文件仍能续传（远端 size 探测）。
- [ ] 删除：文件删除成功；空目录删除成功；非空目录返回 400 且文案含原因。
- [ ] 路径处理：目录名为 `a/`、`a`、`.`、空串等边界不产生双斜杠或错误路径。
- [ ] 并发：同一连接上边下载边列表不互相阻塞（锁释放验证：下载期间列表请求响应时间不显著增加）。
- [ ] 越权：用户 B 携带用户 A 的 connection_id 请求文件接口 → 404/400（凭据不可解析路径 404）。
- [ ] 上传取消：前端 abort 后远端存在部分文件（可续传），不产生悬挂句柄。

## 8. Open Questions（待定项）

- 上传取消后是否提供「清理远端残片」动作（当前保留残片以支持续传）。
- 是否支持目录整体上传（递归）——当前仅单文件。
