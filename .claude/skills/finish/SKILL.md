---
name: finish
description: "任务收尾：确认 verify 已通过，按『再发现成本 > 维护成本』门控决定是否有知识要写回 CONTEXT.md/AGENTS.md/docs/adr/（默认不写），归档 TASK.md。Use when a task is verified and done, the user says finish/wrap up/archive, or at a natural stopping point. 刻意保持轻，不生成报告不搞提交仪式。"
---

# Finish —— 门控收尾，默认什么都不写

前置：verify 已通过。未通过 → 先回去跑 verify；不要在 finish 里修代码。

## 1. 知识写回门控

只有**同时满足**以下三条才落盘：

1. 未来的 agent 无法廉价地再发现（代码、测试、git history 里看不出来）
2. 大概率再次重要
3. 不属于"从代码和测试一眼 obvious"

满足 → 按路由表写入；**三个都不满足 → 什么都不写，直接进第 2 步**。默认是不写，这是特性不是偷懒。

| 知识类型 | 去处 |
|---|---|
| 领域词汇 / 敲定的术语 | CONTEXT.md（保持纯词汇表） |
| 永久不变量 / "在这必须怎么做" | AGENTS.md |
| 难逆转 + 未来读者会困惑 + 真权衡的决定 | docs/adr/（三条件缺一即弃） |
| API 的实际行为 | 代码 + 测试，不写文档 |
| 实现细节 / 一次性调试发现 | 不写 |

ADR 三个条件在写入前重新过一遍——防知识垃圾场靠的是这道门，不是靠自觉。

## 2. 归档

- TASK.md：在顶部加一行状态（`done: YYYY-MM-DD`），然后
  `mv .mini/tasks/current.md .mini/tasks/archive/MM-DD-<slug>.md`
- 任务目录里若有升级产物（design.md 等）一并 mv
- 归档后 `.mini/tasks/` 下没有 current.md —— 这是正常状态，表示当前无活动任务

## 3. 收尾输出

两行以内：本次完成了什么 + 归档到了哪里。

不要生成总结报告、不要写 journal、不要额外的 bookkeeping commit——正常 git commit 走当前会话的常规流程。跨会话的记忆交给 git history + `trellis mem`（若已装）+ 留下的 CONTEXT/ADR。
