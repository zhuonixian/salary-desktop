# 第十七阶段进度：银行流水导入增强——列识别诊断与手工映射

spec：`docs/superpowers/specs/2026-10-25-stage17-bank-import-mapping-design.md`
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 3）
基线：第十六阶段已交付（后端 414 测试 → 本阶段 417）
## Task 状态

| # | 任务 | 状态 |
|---|---|---|
| 1 | spec：列识别诊断 / 手工映射全链路 / 单列金额 / 别名扩充 | ✅ |
| 2 | 后端：resolve_bank_columns + hints + `_with_mapping` 变体（旧签名委托）+ 命令参数 | ✅ |
| 3 | 后端测试 3 项（单列金额与 hints 三态 / 别名扩充 / 手工映射救回怪列名 + manual_missing 不炸） | ✅ |
| 4 | 前端：types/api + 导入 Modal 识别标签行 + 映射编辑器 + 重新解析 + 0 行禁确认 | ✅ |
| 5 | 回归 + 文档四件套 + push | 🔄 |

## 实现决策与偏差记录

- 旧签名全部保留为 None 委托（preview / read_content / import 前置解析），db.rs 既有 4 处测试调用与 excel 既有 CSV 测试零改动。
- `header_map`/`get_bank_value` 随重构删除（仅银行导入使用）；`value_at` 保持 trim+空串归 None 的原语义。
- 手工映射到不存在的列名 → manual_missing 提示而非报错：用户改选即可，不中断导入流。
- okButton 新增 total_rows===0 禁用：此前"解析出 0 行"仍可点确认导入（导入 0 条），现在引导先解决映射。

## 挂账

- Windows exe 手工验收：怪列名文件 → 映射 → 重析 → 导入一致；并入 v0.10 验收清单第十一节补项
- 多流水对多批次聚合匹配 / 明细级匹配 / 映射持久化与银行预设档案：spec 第 5 节不做范围
