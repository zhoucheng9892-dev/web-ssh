---
spec: 0005
title: 前端架构与交互约束
status: active
owner: frontend/src
tags: [vue3, router, pinia, xterm, terminal-lifecycle, axios]
last_reviewed: 2026-08-05
---

# 前端架构与交互约束

## 1. Context（背景）

Vue 3 + Vite + TypeScript 单页应用，Element Plus 组件库。路由使用 **Hash 模式**（子路径部署时后端无需 SPA fallback 深链支持）。终端会话由 `useTerminal.ts` 管理：xterm 实例与 WebSocket 在会话生命周期内常驻，跨页面切换仅做 DOM reparenting，不销毁渲染器。生产环境前端构建产物被后端 rust-embed 嵌入，前端代码不得假设任何外部静态服务器。

## 2. Goals（目标）

- 终端会话跨页面切换（KeepAlive）不丢滚动历史、不闪断 WebSocket。
- 所有 API/WS 路径在任意 context path 下可用（`/`、`/webssh` 等）。
- 未授权（401）时全应用统一弹回登录页；未初始化时全应用引导至 Setup 页。

## 3. Non-Goals（非目标）

- 不做 PWA / Service Worker / 离线缓存。
- 不做 i18n（界面文案为中文，代码注释英文为主）。
- 不引入组件库之外的 UI 框架（保持 Element Plus 唯一来源）。
- 不做路由级代码拆分优化之外的前端性能工程（当前按路由懒加载）。

## 4. Constraints（约束）

### 路由与状态（MUST）

- **Hash 路由**：`createWebHashHistory()`；路由表固定：公开 `/login`、`/setup`；受保护 `/`（terminals）、`/connections`、`/files`、`/password`、`/users`（admin-only，由 AppLayout 隐藏入口 + 守卫双重控制）。
- **守卫顺序**：`beforeEach` 中先 `init()`（若 loading），然后按序判定：needsSetup → 未登录访问受保护页 → `/users` 非管理员 → 已登录访问公开页（弹回 terminals）。保持该判定顺序，不得跳过 loading 等待。
- **全局 401**：axios 响应拦截器在 401 时 MUST `dispatchEvent(new CustomEvent('auth:unauthorized'))`，App 层监听该事件跳转登录；错误消息统一取 `error.response.data.error`。
- **context path**：所有 URL（API baseURL、WS URL、下载 URL、上传 URL）MUST 经 `appPath()` 构造（基于 `window.__CONTEXT_PATH__`，后端注入 index.html）；禁止硬编码 `/api` 前缀。
- **密码哈希**：`auth.ts` / `users.ts` 中所有密码提交 MUST 先 `sha256Hex()`（`utils/hash.ts`，js-sha256）再发送。
- **KeepAlive**：AppLayout 内子视图保持 `<KeepAlive>`；`TerminalView` / `FilesView` 依赖 `onActivated/onDeactivated` 完成重挂载与数据刷新，不得移除。

### 终端会话生命周期（MUST）

- **常驻模型**：`openSession` 一次性创建 `Terminal + FitAddon + host div + WebSocket`；`attachSession` 将 host div reparent 进 pane（保留滚动历史与渲染器）；`detachSession` 摘除但**不销毁**；`closeSession` 才销毁（ws.close + terminal.dispose）。
- **输出缓冲**：host 未挂载（detached）期间收到的输出 MUST 缓冲进 `session.pending`，`attachSession` 时 flush——任何路径不得丢字节。
- **发送串行**：`terminal.onData` 输出 MUST 经单一 promise 链（`sendChain`）串行发送，防止键入与粘贴交错。
- **首帧 resize 竞态**：WS `onopen` 后 MUST 立即补发一次 resize（携带最新 cols/rows）——PTY 初始 80×24 与真实尺寸不一致会导致 readline 换行错乱（历史缺陷，禁止回退）。
- **probe 预检**：`openTerminal` 流程 MUST 先 `terminalApi.probe()`，失败以 ElMessage 展示真实原因后中止；成功后 WS 握手失败（`!everOpened && code===1006`）显示明确的拒绝文案。
- **心跳应答**：收到服务端 `{"type":"ping"}` MUST 回 `{"type":"pong"}`（仅 OPEN 状态）。
- **refit 防抖**：`refit()` MUST 100ms 防抖（WeakMap 每会话一个 timer）；窗口 resize / 侧栏 pin 切换（`sidebar:toggle` 自定义事件）/ tab 切换触发。
- **关闭确认**：非 closed 会话关闭前 MUST `ElMessageBox.confirm` 二次确认。

### API 层（MUST）

- `client.ts`：`baseURL = appPath('api')`、`withCredentials: true`、`timeout: 30000`。
- **大文件上传不走 axios**：进度上报与取消依赖 `uploadWithProgress`（XHR）与 `uploadChunked`（fetch + AbortController）；两者返回 `{promise, abort}` 统一形状；禁止用 axios 上传大文件（30s 超时 + 无可靠进度）。
- **分块续传**：`uploadChunked` 流程固定为 probe(upload-status) → 从远端 size 续传 → 逐块 POST → sessionStorage 记录 `webssh.upload.<name>`（可选，尽力而为）；禁止对已完整文件重复上传。
- **下载**：走 `downloadUrl` 临时 `<a>` 触发（cookie 自动携带），不 fetch 到内存。

### 样式与组件（SHOULD）

- 视图共享 CSS 变量（`--bg / --panel / --text / --muted / --accent / --border`，见 styles.css），禁止视图内硬编码色值（xterm 主题色除外）。
- Element Plus 组件全局注册按需导入；消息统一 `ElMessage` / `ElMessageBox`，避免混用原生 alert/confirm。

## 5. Decisions Made（已定决策）

- 路由 Hash 模式（后端仅需服务 `/` 与静态资源，深链由 `#` 承载）。
- 终端主题：GitHub Dark 风格色板（`defaultTheme()`），WebGL 渲染器优先、失败回退 canvas。
- 会话编号：`s${++counter}` 作为 session id；同名连接多开时标题加 `(N)` 后缀。
- 上传策略：默认 `uploadChunked`（8 MiB 块），`uploadWithProgress` 保留为备用路径。
- 认证事件：`auth:unauthorized` 自定义事件作为全局登出信号（store + router 协同）。
- KeepAlive 缓存由 AppLayout 统一承载，视图内不自行缓存组件。

## 6. Task Breakdown（任务分解）

1. **基础设施**：`context.ts` + `api/client.ts` + `utils/hash.ts`（所有上层依赖，先改）。
2. **认证流**：`stores/auth.ts` + `router/index.ts` + Login/Setup/Password 视图。
3. **连接管理**：`api/connections.ts` + `ConnectionsView.vue`。
4. **终端**：`composables/useTerminal.ts` + `TerminalView.vue`（高风险，独立任务）。
5. **文件管理**：`api/files.ts` + `FilesView.vue`。
6. **用户管理**：`api/users.ts` + `UsersView.vue` + AppLayout 导航入口。

## 7. Verification Criteria（验收标准）

- [ ] `npm run build`（vue-tsc 严格类型检查）零错误；`npm run type-check` 通过。
- [ ] 子路径部署：`WEBSSH_CONTEXT_PATH=/webssh` 下，登录、API 请求、WS 连接、文件上传下载全部可用（URL 前缀正确）。
- [ ] 401 流：删除 Cookie 后任意操作 → 弹回 `/login`；未初始化 → 强制 `/setup`。
- [ ] 终端会话切换：终端页 → 连接页 → 返回终端页，滚动历史保留、无重连、无空白；期间远端输出不丢（pending 缓冲验证：在 Files 页停留 5s 后返回，期间终端输出完整显示）。
- [ ] 快速 resize 后远端 `stty size` 与本地一致；首次打开（WS 尚未 open 时 pane 已 attach）尺寸正确（回归 80×24 缺陷）。
- [ ] 发送串行：长粘贴 + 连续键入不出现乱序（远端 `cat` 校验顺序）。
- [ ] 上传：100MB 文件进度条平滑、可暂停/继续、完成后 md5 一致；刷新页面后重传跳过已传部分。
- [ ] 下载：文件名含空格/中文的 `Content-Disposition` 正确。
- [ ] 越权导航：非管理员 URL 直达 `/users` 被重定向至终端页。

## 8. Open Questions（待定项）

- 是否引入 `vite-plugin-checker` 等构建期 lint 门禁（当前仅 vue-tsc 类型检查）。
- 终端搜索 / 复制快捷键自定义是否需要设置面板。
