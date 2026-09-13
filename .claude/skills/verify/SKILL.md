---
name: verify
description: "对照任务验收标准与项目规范验证刚写的代码：Acceptance 逐条核对、scope creep 检查、lint/typecheck/tests 确定性检查、只报可行动失败并直接修复。Use when code was just written or changed, before claiming done, before committing, or when the user asks to check/verify/review changes. 高风险改动会升级为独立 fresh-eyes 复核。"
---

# Verify —— 结果检查，不是阶段仪式

一次跑完，产出结论；fail → 修 → 重跑，直到 green。不做 report-and-wait。

## 输入

- `git diff` + `git status`（本次改动全集）
- `.mini/tasks/current.md`（若存在——Acceptance 段是验收基准）
- AGENTS.md / CONTEXT.md / 相关 ADR（规范基准）

## 检查顺序

1. **验收逐条核对**：TASK.md 的 Acceptance 每一条标注 pass / fail / 未验证。Direct 任务没有 TASK.md，用用户原始请求作为验收基准。
2. **范围检查**：diff 里有没有任务之外的东西？scope creep 的文件要么回退，要么明确说明理由。
3. **正确性 / 安全 / 回归扫查**：是否破坏现有行为；debug 残留、被绕过的类型检查、压掉的警告、删掉的测试。
4. **确定性检查**：跑项目的 lint / typecheck / tests。命令从 package.json / Makefile / CI 配置里发现，不要猜。
5. **只报告可行动的失败，并直接修**；修完重跑第 4 步。全部 green 才算过。

## fresh-eyes 升级（唯一值得保留的 sub-agent 场景）

满足任一条 → 派一个**独立的** reviewer sub-agent（无 Task 工具时建议用户开新会话）只读 diff + TASK.md + 规范做复核：

- 改了 API 契约 / 数据模型 / 安全相关代码
- 数据迁移或不可逆操作
- diff 跨 3 层以上，或超过约 300 行

理由：刚写完代码的同一个上下文会继承自己的假设。fresh review 不是因为模型不会 review，是因为自我审查有盲区。复核意见按第 5 步同样处理。

## 边界

- 不重新设计、不顺手重构任务外的东西、不引入新依赖
- 发现"规范本身该补" → 记到 TASK.md 备注，留给 finish 门控判断；不在 verify 里写文档
