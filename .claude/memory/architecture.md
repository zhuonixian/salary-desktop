---
name: architecture
description: 后端模块职责 + 数据流 + 前端页面映射
---

# 架构模块

## 后端 Rust（src-tauri/src/）

| 文件 | 职责 |
|------|------|
| `main.rs` | 入口（仅调 `app_lib::run()`），含 `diag()` 启动日志 |
| `lib.rs` | `run()` 配置 Tauri Builder：plugins / setup（DB init）/ `generate_handler!` 注册所有命令 |
| `commands.rs` | 所有 `#[tauri::command]` 入口（员工/考勤/工资/OCR/发票/导出/打卡表），薄薄一层只做参数转发 |
| `db.rs` | SQLite CRUD + 默认数据 + 操作日志 + dashboard 汇总。所有 SQL 在这里 |
| `models.rs` | 所有 serde 结构体（Employee / AttendanceRecord / SalaryResult / Invoice / OcrBatch 等） |
| `errors.rs` | `AppError` enum（Database/Excel/Json/Io/NotFound/Ocr/InvalidParam/General/Network）+ `AppResult<T>` |
| `excel.rs` | calamine 读员工/考勤 Excel + rust_xlsxwriter 写工资明细/银行代发/工资条/考勤汇总/发票清单 |
| `ocr.rs` | 考勤 OCR（百度在线 + Python PaddleOCR 本地）+ 打卡表 OCR + `get_baidu_access_token`（pub(crate)，发票模块复用） |
| `salary.rs` | 工资计算引擎：读取员工 + 考勤 + 规则 + 税率，按公式算 gross/social/tax/net |
| `invoice.rs` | 发票业务：OCR 调用 + 字段映射 + dedup + 图片归档 + save/update/delete。含 `InvoiceOcrDbOps` trait |

## 前端 React（src/）

| 文件 | 职责 |
|------|------|
| `main.tsx` | React 入口 |
| `bootstrap.tsx` | 应用 bootstrap（如错误边界） |
| `App.tsx` | HashRouter + Layout（Sider 菜单 + Header 全局月份 + Content Routes） |
| `api/index.ts` | 所有 Tauri `invoke<>` 封装 + 后端字段标准化（snake_case 透传） |
| `types/index.ts` | 所有 TS 类型定义 |
| `pages/Dashboard.tsx` | 首页仪表盘（员工数、计算数、应发合计等汇总） |
| `pages/Employees.tsx` | 员工管理（CRUD + Excel 导入导出） |
| `pages/Attendance.tsx` | 考勤管理（Excel 导入 + 编辑） |
| `pages/PunchCard.tsx` | 打卡表管理（模板生成 + OCR 识别） |
| `pages/OcrCenter.tsx` | OCR 识别中心（考勤 OCR，百度在线 / 本地 PaddleOCR 切换 + 设置） |
| `pages/Invoices.tsx` | 发票管理（上传 OCR + 手工录入 + 编辑 + 详情 + 费用类型 + Excel 导出） |
| `pages/SalaryRules.tsx` | 工资规则配置（扣款/比例/起征点） + 个税税率表 |
| `pages/SalaryCalculate.tsx` | 月度工资计算（一键算 + 单人重算 + 调整 + 锁定） |
| `pages/ExportCenter.tsx` | 导出中心（多格式 Excel） |

## 数据库表（database/schema.sql）

| 表 | 用途 |
|----|------|
| `employees` | 员工基础信息 + 工资参数 |
| `attendance_records` | 月度考勤（每员工每月一行） |
| `salary_rules` | 工资规则键值对（迟到扣款/社保比例等） |
| `tax_rules` | 个税 7 级超额累进税率 |
| `salary_monthly_results` | 月度工资计算结果（每员工每月一行） |
| `ocr_batches` | 考勤 OCR 批次记录 |
| `punch_card_batches` | 打卡表 OCR 批次 |
| `operation_logs` | 操作审计日志 |
| `app_settings` | KV 配置（百度 key、token 缓存等） |
| `invoice_expense_types` | 发票费用类型字典 |
| `invoices` | 发票主表 + dedup 索引 |

## 数据流示例

**工资核算**：
```
用户选月份 → 点"一键计算"
  → commands::calculate_salary(month, state)
  → salary::calculate_monthly_salary(month, conn)
    ├─ 读取 employees / attendance_records / salary_rules / tax_rules
    ├─ 遍历员工计算 gross / social / tax / net
    └─ save_salary_result() 写 salary_monthly_results
  → 返回结果数组 → 前端展示
用户可调整其他补助/扣款 → update_salary_result
锁定 → lock_salary_results（status='locked', locked=1）
```

**发票上传**：
```
用户选 PDF/PNG
  → commands::ocr_invoice(image_path, state: &Mutex)
  → invoice::ocr_invoice(image_path, db_ops: &impl InvoiceOcrDbOps)
    ├─ token = db_ops.get_baidu_access_token()  // 锁内
    ├─ HTTP POST vat_invoice  // 锁外
    ├─ map_baidu_response → InvoiceOcrPreview
    └─ db_ops.find_invoice_by_dedup_key()  // 锁内
  → 返回 preview
用户编辑表单 → 保存
  → commands::save_invoice(data, app, state)
  → invoice::save_invoice(data, db_ops, app_data_dir)
    ├─ guard: number 非空 + trim 规范化
    ├─ 二次查重 find_invoice_by_dedup_key
    ├─ copy_image_to_app_dir(src, belong_month, app_data_dir)
    ├─ db::insert_invoice(conn, input, target_path)
    └─ db::log_operation("save_invoice", ...)
```

## 关键 trait / 抽象

- `InvoiceOcrDbOps`（invoice.rs）：抽象 DB 操作，有两个 impl：
  - `Connection`：直接传 conn（测试用）
  - `Mutex<Connection>`：生产用，trait 方法内部短暂 lock
- `InvoiceOcrInnerError`（invoice.rs）：`TokenInvalid(AppError) | Other(AppError)`，让 110 错误类型化重试
- `AppError`（errors.rs）：所有错误统一类型，impl `From<rusqlite::Error>` / `From<io::Error>` 等让 `?` 工作

## 测试组织

- `#[cfg(test)] mod tests`：单元测试（pure function 如 `map_baidu_response`、`parse_amount`、`translate_baidu_error`）
- `#[cfg(test)] mod business_tests`：业务层集成测试（in-memory SQLite + save_invoice 流程）
- `db::tests`：CRUD 测试，setup_db() 建 in-memory schema
- 测试 idempotent，无外部依赖（不调真实百度 API）
