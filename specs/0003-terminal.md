---
spec: 0003
title: 终端桥接（WebSocket ↔ SSH PTY）
status: active
owner: backend/src/terminal + backend/src/ssh
tags: [terminal, websocket, ssh, pty, protocol]
last_reviewed: 2026-08-05
---

# 终端桥接（WebSocket ↔ SSH PTY）

## 1. Context（背景）

浏览器通过 WebSocket 连接到 `/api/terminal/connect`，后端完成 SSH 拨号 + PTY 后把 WebSocket 与 SSH channel 双向桥接。WS 帧协议：**binary = 原始终端字节流**，**text = JSON 控制消息**（resize / ping / exit / closed）。另有 `probe` 端点做连接预检（因为浏览器无法读取 WebSocket 握手失败时的 HTTP 错误体）。

## 2. Goals（目标）

- 任意时刻、任意网络条件下（NAT / 代理 / sshd ClientAlive）会话不被静默掐断：30 秒心跳保活。
- 空闲超时（可配置）能可靠关闭死会话；`0` 表示永不因空闲断开。
- 拨号失败（DNS / 算法协商 / 认证 / 超时）时用户能看到真实原因。
- resize 与输入竞态不丢帧：输入串行有序、尺寸最终一致。

## 3. Non-Goals（非目标）

- 不做终端回放 / 录制 / 日志审计。
- 不做多路复用（一个 WS 只承载一个会话；多 Tab = 多个 WS）。
- 不做 SSH 端口转发 / X11 / scp（仅 shell + SFTP）。
- 不实现 known_hosts 强校验（TOFU 为当前策略，见第 5 节）。

## 4. Constraints（约束）

### 路由与协议

| 项 | 规格 |
|----|------|
| GET /api/terminal/connect?connection_id=&cols=&rows= | WebSocket 升级；**SSH 拨号必须先于升级完成**，失败返回 400 JSON（浏览器侧不可读，故配 probe） |
| GET /api/terminal/probe?connection_id=&cols=&rows= | HTTP 预检：拨号 + 认证后立即断开（不开 shell），成功返回 `{"ok":true,host,port,username}` |
| WS binary 帧 | 原始终端字节（双向） |
| WS text 帧（客户端→服务端） | `{"type":"resize","cols":N,"rows":N}`；`{"type":"ping"}` / `{"type":"pong"}`（心跳应答） |
| WS text 帧（服务端→客户端） | `{"type":"ping"}`（每 30s）；`{"type":"exit","code":N}`；`{"type":"closed","reason":"idle timeout"}` |

### MUST

- **拨号前置**：`connect` handler 必须在 `ws.on_upgrade` 之前完成 `resolve_secret`（解密凭据）→ `get_connection_dial` → `ssh::connect` → `ssh::open_shell`；任一步失败返回 400（错误文案形如「SSH 连接失败: ...」）。
- **认证方式**：密码认证 MUST 依次尝试 `keyboard-interactive` 与 `password` 两种方法（各自独立新连接），任一成功即返回；两种均拒绝/超时才判定失败；私钥走 `authenticate_publickey` 单次尝试。
- **超时预算**：握手 15s、单次认证操作 10s、kbd-interactive 至多 3 轮；超时文案使用人类可读时长（如「SSH 握手超时（15s）」）。
- **PTY 规格**：`xterm-256color`，完整标准 termios 模式表（VINTR..ONLCR 全套，含 OPOST/ONLCR=1 保证 `\n`→`\r\n` 输出处理）；禁止回退到仅 ECHO 的精简模式（曾导致 readline 换行错乱）。
- **心跳**：服务端每 30s 发送 text `{"type":"ping"}`；客户端收到后应答（前端实现见 0005）。心跳不重置空闲计时（只有真实数据交互才视为活动）。
- **空闲超时**：`terminal_idle_timeout_secs > 0` 时，任何方向超过该时长无数据交互 → 发送 `{"type":"closed","reason":"idle timeout"}` 并关闭 WS；`0` = 禁用（用 `pending()` future 永不触发）。空闲计时基于 `last_activity`，每次收/发数据、resize、ping/pong 均刷新。
- **通道关闭**：收到 SSH `ExitStatus` → 发送 `{"type":"exit","code":N}` 后关闭 WS；`Eof`/channel 消失 → 直接关闭 WS；WS 关闭 → `channel.eof()` + `channel.close()`，并 abort reader 任务。
- **帧发送顺序**：SSH 输出（Data / ExtendedData）与心跳、控制帧共用同一 sink，MUST 保证不交错（驱动循环内 select 顺序写入即可）。
- **TOFU 主机密钥**：`check_server_key` 当前实现为接受任意主机密钥并记录 SHA-256 指纹（info 日志）；不得静默吞掉指纹日志。
- **性能参数**：`maximum_packet_size` 必须 ≥ 1 MiB、`window_size` ≥ 4 MiB（大文件/高频输出吞吐依赖，恢复默认 32KiB 会退化到 ~150KB/s 的 stop-and-wait）。

### MUST NOT

- MUST NOT 在 `connect` 的 HTTP 阶段之后（升级后）再进行任何可能失败的异步拨号——升级成功后失败无法以 HTTP 状态返回。
- MUST NOT 将 SSH 会话复用/共享给其他用户或请求（每 WS 独立 channel）。
- MUST NOT 在日志中记录凭据或会话密钥。

### SHOULD

- `touch_last_used` 在拨号成功后更新（尽力而为，失败不阻塞）。
- 忽略的 channel 消息以 debug 级别记录（如 `ChannelMsg::WindowAdjusted` 等）。
- probe 成功时记录 info 日志（host/port/user）便于排查。

## 5. Decisions Made（已定决策）

- SSH 客户端封装集中在 `ssh/mod.rs`：`connect` / `open_shell` / `open_sftp` / `ClientHandler`；terminal 与 files 共用。
- 认证策略「双方法 + 独立连接」：因部分服务端对不支持的方法静默不响应（表现为超时），先拒绝/超时不可直接判负。
- 空闲计时语义：**仅真实数据交互**（Data/Resize/Activity）刷新；心跳不算活动（否则空闲超时形同虚设）。
- PTY 模式表为 OpenSSH 客户端标准值，改动需回归验证 readline 换行与粘贴行为。
- 前端依赖 `probe` 预检 + WS 的 `everOpened` 标志区分「握手被拒」与「中途断开」（见 0005）。

## 6. Task Breakdown（任务分解）

1. **SSH 层**：`ssh/mod.rs`（connect 双方法认证 / open_shell / 超时工具）——改动风险最高，独立任务。
2. **桥接层**：`terminal/mod.rs`（connect / probe / run_bridge / idle_future）。
3. **前端**：`composables/useTerminal.ts`（openSession / attach / detach / refit / close）+ `api/terminal.ts`。
4. **验证**：真实 SSH 目标（password 与 key 各一）端到端验证 + 断网/超时场景。

## 7. Verification Criteria（验收标准）

- [ ] 对可达目标：probe 返回 ok；WS 连接后 shell 提示符出现，输入回显正常。
- [ ] 密码目标同时验证：仅支持 password 方法、仅支持 keyboard-interactive（PAM）方法、两者都拒绝（返回 400 且文案含「authentication rejected」）。
- [ ] resize：浏览器窗口缩放后远端 `stty size` 与客户端一致；快速连续 resize 不出现 PTY 尺寸回退。
- [ ] 心跳：30s 静默期后 WS 仍存活（服务端日志可见 ping 发送）；抓包确认双向 ping/pong。
- [ ] 空闲超时：设 `WEBSSH_TERMINAL_IDLE_TIMEOUT_SECS=5`，静默 5s 后收到 `{"type":"closed","reason":"idle timeout"}` 且 WS 关闭；设 0 时静默 60s 不断。
- [ ] 会话结束：远端执行 `exit` → 收到 `{"type":"exit","code":0}` 后 WS 关闭；杀掉远端 sshd → WS 关闭且前端显示连接关闭提示。
- [ ] 拨号失败：错误 host（DNS）/ 错误密码 / 连接拒绝，probe 分别返回可读的 400 文案（前端 ElMessage 可见）。
- [ ] 粘贴长行 + 方向键：readline 重绘无乱码（回归 OPOST/ONLCR 约束）。
- [ ] 并发：同一连接开 3 个 Tab，各自独立、互不影响；关一个不影响另两个。
- [ ] 日志检查：全程日志无明文密码/私钥内容。

## 8. Open Questions（待定项）

- known_hosts 强校验：TOFU 升级方案（指纹持久化 → 首次信任 + 变更告警）。
- 空闲超时是否需要「客户端可见倒计时」提示（当前仅关闭前一次性提示）。
