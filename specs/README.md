# SDD 规范（Spec-Driven Development）

本目录是本项目**可执行的规格契约**（executable contracts），是 AI 代理与人类工程师在改动代码前必须阅读并遵守的单一事实来源（single source of truth）。规范随代码演化，任何偏离本目录的行为都必须先更新 spec 再改代码。

## 工作流

本项目采用 **Spec-First（棕地重建）** 模式：

1. **阅读**：动工前，先读取与本任务相关的 spec 文件（按编号前缀定位领域）。
2. **遵守**：实现必须满足 spec 中的 MUST / MUST NOT 约束；SHOULD 为推荐项，偏离需在代码注释中说明理由。
3. **验证**：完成改动后，对照每个 spec 的「验收标准」逐条自检；WebSocket 等浏览器不可读错误体的路径，必须额外检查服务端日志侧约束。
4. **更新**：行为变更（协议、路由、错误语义、数据流）必须先改 spec 再改代码；spec 是活的文档，不允许滞后。

推荐配合 **Coordinator / Implementor / Verifier** 模式使用：Coordinator 按 spec 拆分任务，Implementor 严格按子 spec 实现，Verifier 以「验收标准」为清单对抗性检查输出。

## 文件索引

| 编号 | 文件 | 领域 | 主要约束对象 |
|------|------|------|--------------|
| 0000 | [architecture.md](0000-architecture.md) | 系统架构 | 模块边界、数据流、技术栈、跨模块契约 |
| 0001 | [auth.md](0001-auth.md) | 认证与授权 | setup/login/logout/captcha/session/用户管理 |
| 0002 | [connections.md](0002-connections.md) | 连接配置 | SSH 连接 CRUD、凭据加密、数据隔离 |
| 0003 | [terminal.md](0003-terminal.md) | 终端桥接 | WebSocket 协议、心跳、空闲超时、SSH 拨号 |
| 0004 | [files.md](0004-files.md) | SFTP 文件管理 | 目录浏览、下载、上传、分块续传 |
| 0005 | [frontend.md](0005-frontend.md) | 前端架构 | Vue 3 路由/状态、终端会话生命周期、API 封装 |
| 0006 | [config-deploy.md](0006-config-deploy.md) | 配置与部署 | 环境变量、单二进制、Docker、子路径 |

## 模板约定

每个 spec 文件遵循统一的六要素结构（Outcomes / Scope / Constraints / Decisions / Tasks / Verification），并使用 YAML frontmatter：

```yaml
---
spec: NNNN
title: 领域名
status: active            # active | deprecated
owner: 负责人或模块路径
tags: [关键词]
last_reviewed: YYYY-MM-DD
---
```

正文结构：

```markdown
# 标题
## 1. Context（背景）        — 为什么存在、当前实现概要
## 2. Goals（目标）          — 完成时应当达成的可观察结果（Outcomes）
## 3. Non-Goals（非目标）    — 明确不做的事（Out-of-Scope）
## 4. Constraints（约束）    — MUST / MUST NOT / SHOULD，逐条可验证
## 5. Decisions Made（已定决策）— 已经敲定的技术选型，禁止再议
## 6. Task Breakdown（任务分解）— 改动该领域时的标准子任务切分
## 7. Verification Criteria（验收标准）— 可执行的检查清单
## 8. Open Questions（待定项）— 尚未决策、需要人工拍板的问题
```

## 约束措辞语义（RFC 2119）

- **MUST / MUST NOT**：硬性约束，违反即 spec 违规，实现不得合入。
- **SHOULD / SHOULD NOT**：推荐行为，偏离需在代码注释中记录理由。
- **MAY**：可选，无强制力。

## 维护规则

- 新增领域 → 新建 `NNNN-<slug>.md`，编号递增，并在本 README 索引中登记。
- 修改协议 / 路由 / 错误语义 / 安全边界 → **必须先更新对应 spec**，再改代码。
- spec 与代码冲突时，以代码行为为准回写 spec（记录已确认的行为），但安全类约束（MUST NOT 泄露凭据、数据隔离等）除外——此类约束冲突视为代码缺陷。
