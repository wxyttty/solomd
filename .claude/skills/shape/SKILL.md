---
name: shape
description: "Shaped 任务的需求澄清与塑造：先探索仓库，再按打断预算只问 consequential 的决定（用户可见行为/API 契约/数据模型/安全/不可逆架构/范围），顺带维护 CONTEXT.md 词汇表与 ADR，最后产出一个 TASK.md。Use when a task has consequential ambiguity, requirements are unclear, there are multiple valid approaches, or the user asks to plan/clarify/brainstorm before building. 不用于明确、局部、低风险的 Direct 任务（那种直接做）。"
---

# Shape —— 有界的澄清与塑造

先探索，后提问；一次一个问题；每个问题都带推荐答案。

本 skill 的目标不是"把设计树问完"，而是：把**猜错会返工的分歧**在动手前解决掉，其余全部采用合理默认。

## 第一步：探索代替提问（Evidence Rule）

问题能从代码、测试、配置、文档、git history、既有 ADR、`trellis mem` 会话历史得到答案的，自己查，不问用户。

仓库证据确立"现状是什么"。用户的意图、偏好、范围边界只能问人；但仓库里已有的惯例只是推荐选项的证据，不是决定本身。

## 决定分诊（每个未知点先分诊，再决定去向）

- **事实问题** → repo / research 查证。不问人。
- **跑起来才知道的问题**（交互手感、状态机实际行为、某库 API 是否承载设计）→ 建议 prototype：一次性丢弃代码验证完就扔。不要继续问。
- **consequential 的用户决策** → 进入提问预算。

## 提问准入（四条全满足才允许问）

1. 无法从 codebase / docs / conventions 推断；
2. 不属于 agent 可合理自主决定的实现细节；
3. 猜错的代价是明显返工，或改变：**用户可见行为 / API 契约 / 数据模型 / 安全边界 / 不可逆架构 / 范围**；
4. 对当前任务是 blocking 的。

## 打断预算（要限制的是打断用户的次数，不是问题数量）

- tiny 任务：0 次用户决策
- normal：3 次
- high-risk / 产品塑形：5 次

- 超预算：采用你的推荐答案，写入 TASK.md 的 **Assumptions** 段（含推荐依据），继续开工。
- **唯一例外**：destructive / security / data-loss 类决策必须问，不受预算限制。
- 动态早停：目标明确 + 验收标准明确 + 修改边界明确 + 无关键未知 → 立即停止提问，不烧完预算。

提问方式：一次一个；每个问题包含「要决定什么 / 为什么重要 / 推荐答案 / 选别的的代价」；多个低风险小决策合并成一次带选项的提问。

## 顺带维护项目语言（保留自 grill-with-docs）

- 用户用词与 CONTEXT.md 已有定义冲突 → 当场指出："你的词汇表把『取消』定义为 X，你现在说的是 Y——以哪个为准？"
- 模糊/多义词 → 提出精确的规范术语："你说的 account 是 Customer 还是 User？"
- 术语敲定 → **立即**写入 CONTEXT.md，不攒批。CONTEXT.md 是纯词汇表，不放实现细节、不当草稿纸。
- ADR 三条件（全中才提议，缺一即弃）：**难逆转 + 未来读者会困惑"为什么这么做" + 真实权衡的结果** → `docs/adr/`。文件懒创建：有第一个词条才建 CONTEXT.md，有第一个 ADR 才建目录。

## 产出：一个 TASK.md

只有当"这次对话结束后意图可能丢失"时才创建 `.mini/tasks/current.md`。Direct 任务不建任何文件。

```markdown
# Goal
用户最终要得到什么。

# Scope
做什么。

# Non-goals
明确不做什么。

# Acceptance
- 逐条可判 pass/fail 的验收标准

# Decisions
本次真正影响实现的决定（含用户决策记录）。

# Assumptions
agent 自主决定但值得暴露的关键假设，各带推荐依据；被推翻时的代价。

# Verify
完成后如何证明它工作：具体命令 + 预期结果。
```

复杂度升级：涉及多契约/数据迁移/多层联动 → TASK.md 内加 Design 段（或独立 design.md）；跨会话/多个独立可交付物 → 建议升级 Structured 流程（to-spec → to-tickets 类技能），不要在 shape 里硬扛。
