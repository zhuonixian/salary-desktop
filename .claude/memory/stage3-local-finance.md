---
name: stage3-local-finance
description: 第三阶段本地轻量财务管理能力计划与开发接力摘要
---

# 第三阶段：本地轻量财务管理

完整计划：`docs/superpowers/plans/2026-08-10-stage3-local-finance.md`  
进度同步：`docs/superpowers/plans/2026-08-10-stage3-progress.md`

## 定位

项目继续保持本地单机 exe 轻量使用，不扩展成完整 ERP。第三阶段围绕出纳本地管理闭环：数据安全、正式月结、付款批次、银行流水匹配、预算与异常提醒。

## 优先级

1. P0：本地数据安全中心。
2. P0：正式月结与反月结。
3. P1：付款批次管理。
4. P1：银行流水导入与匹配。
5. P2：预算与异常提醒。
6. P2：发票类型扩展、工资规则版本化、凭证草稿导出。

## 开发协作

- 每轮开发前读取本文件、完整计划和进度同步文件。
- subagent 只处理互不重叠的文件范围。
- 主 agent 负责合并、测试、commit、push。
- 每轮结束必须更新进度同步文件。

## 测试门槛

基础回归：

```bash
npx tsc --noEmit
npm run build
cd src-tauri && cargo test --lib
```

涉及 Rust 新模块时增加：

```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo check
```

涉及 Tauri 打包或 exe 验收时增加：

```bash
npm run tauri build
```
