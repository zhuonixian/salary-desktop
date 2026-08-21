# 第六阶段财务功能拓展 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 补全科目余额表与年末结转、社保公积金台账（含单位部分与凭证联动）、个税累计预扣与年度汇总、工资条打印、三大报表上年同期对比。

**Architecture:** 复用第五阶段凭证与报表底座——试算平衡与同期列基于现有引擎函数泛化区间实现；年末结转作为新 source_type='period_close' 凭证由月结命令层编排生成/作废；社保台账新表按员工×年度，工资引擎优先取台账；个税累计预扣基于已存工资记录聚合，无迁移。

**Tech Stack:** Rust + Tauri 2 + rusqlite（后端单文件模块）、React 19 + Ant Design 6 + TypeScript（前端）、rust_xlsxwriter（Excel 导出）。

**Spec:** `docs/superpowers/specs/2026-08-22-stage6-finance-extensions-design.md`

## Global Constraints

- 中文 UI 字符串、中文 commit message（可中英混合）
- Tauri 命令：`#[tauri::command]` + snake_case，前端 `invoke('snake_case_name')`
- 时间戳：`Utc::now().to_rfc3339()`
- 测试：单元测试在 `#[cfg(test)] mod tests`，用 `Connection::open_in_memory()`（db 初始化调用 `db::init_db(&conn)` 或现有测试 helper，参照 `accounting.rs` 测试）
- 不跳过 hooks，不 `--no-verify`
- 全量回归门槛：`npx tsc --noEmit`、`npm run lint`、`npm run build`、`cd src-tauri && cargo fmt --check`、`cargo check`、`cargo test --lib`
- 工资结果表名是 `salary_monthly_results`（不是 salary_results）
- 本年利润科目 3103、利润分配—未分配利润 3104（小企业会计准则编码）
- 已有约定：报表排除 `period_close` 分录；科目余额表包含全部凭证（真实账面）

---

### Task 1: 试算平衡（科目余额表）引擎

**Files:**
- Modify: `src-tauri/src/models.rs`（新增 2 结构体，放在报表结构附近）
- Modify: `src-tauri/src/accounting.rs`（新增 `build_trial_balance`，放在 `compute_balances` 附近；测试加到文件尾 `mod tests`）
- Test: `src-tauri/src/accounting.rs` `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `opening_month(conn) -> Option<String>`（accounting.rs:951 现有私有函数）
- Produces: `pub fn build_trial_balance(conn: &Connection, from_month: &str, to_month: &str) -> AppResult<TrialBalanceReport>`；`pub struct TrialBalanceReport { from_month: String, to_month: String, enabled: bool, rows: Vec<TrialBalanceRow>, balanced: bool }`；`pub struct TrialBalanceRow { code: String, name: String, category: String, direction: String, opening_debit: f64, opening_credit: f64, period_debit: f64, period_credit: f64, ending_debit: f64, ending_credit: f64 }`（Task 2 命令与前端依赖这些类型）

- [ ] **Step 1: models.rs 加结构体**

在 models.rs 报表结构区域（`BalanceSheet` 附近）加：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialBalanceRow {
    pub code: String,
    pub name: String,
    pub category: String,
    pub direction: String,
    pub opening_debit: f64,
    pub opening_credit: f64,
    pub period_debit: f64,
    pub period_credit: f64,
    pub ending_debit: f64,
    pub ending_credit: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialBalanceReport {
    pub from_month: String,
    pub to_month: String,
    pub enabled: bool,
    pub rows: Vec<TrialBalanceRow>,
    pub balanced: bool,
}
```

- [ ] **Step 2: 写失败测试**

在 accounting.rs `mod tests` 中加（参照现有测试的建库方式——现有测试用 `fn test_conn()` 或每个测试自建，先读 `mod tests` 开头确认 helper 名称后照抄；以下用 `let conn = test_conn();` 表达）：

```rust
#[test]
fn test_trial_balance_basic() {
    let conn = test_conn();
    // 期初：1001 借 1000、2211 贷 1000（沿用现有测试保存期初的方式）
    conn.execute("INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount) VALUES ('2025-01', '1001', 1000.0, 0.0)", []).unwrap();
    conn.execute("INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount) VALUES ('2025-01', '2211', 0.0, 1000.0)", []).unwrap();
    // 2025-01 发生：借 6602 / 贷 1001 各 100
    conn.execute("INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status) VALUES ('记-202501-001', '2025-01-10', '2025-01', 'bank_manual', 1, 100.0, 'active')", []).unwrap();
    let vid: i64 = conn.query_row("SELECT id FROM vouchers WHERE voucher_no='记-202501-001'", [], |r| r.get(0)).unwrap();
    conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '6602', 100.0, 0.0)", [vid]).unwrap();
    conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '1001', 0.0, 100.0)", [vid]).unwrap();

    let report = build_trial_balance(&conn, "2025-01", "2025-01").unwrap();
    assert!(report.enabled);
    let cash = report.rows.iter().find(|r| r.code == "1001").unwrap();
    assert_eq!(cash.opening_debit, 1000.0);
    assert_eq!(cash.period_credit, 100.0);
    assert_eq!(cash.ending_debit, 900.0);
    assert_eq!(cash.ending_credit, 0.0);
    assert!(report.balanced);
}

#[test]
fn test_trial_balance_cross_month_rolls_opening() {
    let conn = test_conn();
    conn.execute("INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount) VALUES ('2025-01', '1001', 500.0, 0.0)", []).unwrap();
    // 2025-01 发生贷 100 → 2025-02 查询时 1001 期初应为 400
    conn.execute("INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status) VALUES ('记-202501-001', '2025-01-10', '2025-01', 'bank_manual', 1, 100.0, 'active')", []).unwrap();
    let vid: i64 = conn.query_row("SELECT id FROM vouchers WHERE voucher_no='记-202501-001'", [], |r| r.get(0)).unwrap();
    conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '1001', 0.0, 100.0)", [vid]).unwrap();
    conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '2241', 100.0, 0.0)", [vid]).unwrap();

    let report = build_trial_balance(&conn, "2025-02", "2025-02").unwrap();
    let cash = report.rows.iter().find(|r| r.code == "1001").unwrap();
    assert_eq!(cash.opening_debit, 400.0);
    // 2241 期初为 0、区间前净额贷方 → 期初在贷侧 100
    let other = report.rows.iter().find(|r| r.code == "2241").unwrap();
    assert_eq!(other.opening_credit, 100.0);
    assert!(report.balanced);
}

#[test]
fn test_trial_balance_not_enabled_without_opening() {
    let conn = test_conn();
    let report = build_trial_balance(&conn, "2025-01", "2025-01").unwrap();
    assert!(!report.enabled);
    assert!(report.rows.is_empty());
}
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib trial_balance`
Expected: FAIL（`build_trial_balance` 未定义）

- [ ] **Step 4: 实现 build_trial_balance**

accounting.rs 中（`compute_balances` 函数后）：

```rust
/// 科目余额表（试算平衡）：区间 [from_month, to_month] 每科目期初/本期发生/期末（借贷双侧）。
/// 与三大报表不同：包含全部凭证（含 period_close），反映真实账面。
pub fn build_trial_balance(
    conn: &Connection,
    from_month: &str,
    to_month: &str,
) -> AppResult<TrialBalanceReport> {
    let mut report = TrialBalanceReport {
        from_month: from_month.to_string(),
        to_month: to_month.to_string(),
        enabled: false,
        rows: Vec::new(),
        balanced: false,
    };
    let Some(open_month) = opening_month(conn) else {
        return Ok(report);
    };
    if from_month < open_month.as_str() {
        return Ok(report);
    }
    report.enabled = true;

    let mut stmt =
        conn.prepare("SELECT code, name, category, direction FROM gl_accounts ORDER BY code")?;
    let mut rows: Vec<TrialBalanceRow> = stmt
        .query_map([], |r| {
            Ok(TrialBalanceRow {
                code: r.get(0)?,
                name: r.get(1)?,
                category: r.get(2)?,
                direction: r.get(3)?,
                opening_debit: 0.0,
                opening_credit: 0.0,
                period_debit: 0.0,
                period_credit: 0.0,
                ending_debit: 0.0,
                ending_credit: 0.0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    // 期初（带符号，借方为正）：启用月期初 + [启用月, from_month) 凭证净额（含 period_close）
    let mut opening_signed: HashMap<String, f64> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT account_code, debit_amount, credit_amount FROM opening_balances",
    )?;
    let obs = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    for row in &rows {
        let dir = if row.direction == "debit" { 1.0 } else { -1.0 };
        if let Some((_, debit, credit)) = obs.iter().find(|(c, _, _)| c == row.code) {
            *opening_signed.entry(row.code.clone()).or_insert(0.0) +=
                (debit - credit) * dir;
        }
    }
    let mut stmt = conn.prepare(
        "SELECT vl.account_code, SUM(vl.debit_amount - vl.credit_amount)
         FROM voucher_lines vl JOIN vouchers v ON v.id = vl.voucher_id
         WHERE v.belong_month >= ?1 AND v.belong_month < ?2 AND v.status = 'active'
         GROUP BY vl.account_code",
    )?;
    let nets = stmt
        .query_map(params![open_month, from_month], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    for row in &rows {
        let dir = if row.direction == "debit" { 1.0 } else { -1.0 };
        if let Some((_, net)) = nets.iter().find(|(c, _)| c == row.code) {
            *opening_signed.entry(row.code.clone()).or_insert(0.0) += net * dir;
        }
    }

    // 区间发生额（含 period_close）
    let mut stmt = conn.prepare(
        "SELECT vl.account_code, SUM(vl.debit_amount), SUM(vl.credit_amount)
         FROM voucher_lines vl JOIN vouchers v ON v.id = vl.voucher_id
         WHERE v.belong_month >= ?1 AND v.belong_month <= ?2 AND v.status = 'active'
         GROUP BY vl.account_code",
    )?;
    let periods = stmt
        .query_map(params![from_month, to_month], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut total_debit = 0.0;
    let mut total_credit = 0.0;
    for row in rows.iter_mut() {
        let opening = *opening_signed.get(&row.code).unwrap_or(&0.0);
        if let Some((_, debit, credit)) = periods.iter().find(|(c, _, _)| c == row.code) {
            row.period_debit = debit;
            row.period_credit = credit;
        }
        // 期末（带符号）= 期初 + (借-贷)（借贷记账法下净额方向即科目余额方向，无需 direction 系数）
        let ending = opening + row.period_debit - row.period_credit;
        // 分侧：正数记借方、负数记贷方（绝对值）
        if opening >= 0.0 {
            row.opening_debit = opening;
        } else {
            row.opening_credit = -opening;
        }
        if ending >= 0.0 {
            row.ending_debit = ending;
        } else {
            row.ending_credit = -ending;
        }
        total_debit += row.ending_debit;
        total_credit += row.ending_credit;
    }
    // 仅保留有数据（期初或发生非零）的科目
    report.rows = rows
        .into_iter()
        .filter(|r| {
            r.opening_debit.abs() > 0.005
                || r.opening_credit.abs() > 0.005
                || r.period_debit.abs() > 0.005
                || r.period_credit.abs() > 0.005
        })
        .collect();
    report.balanced = (total_debit - total_credit).abs() < 0.005;
    Ok(report)
}
```

注意：`opening_balances` 的 debit/credit 只有一侧有值（保存时校验借贷平衡是科目间，不是行内），`debit - credit` 即带符号期初。期末公式与方向无关：借贷记账法下 `期初(借正) + 借 - 贷` 的正负即余额侧。这与 `AccountBalance::ending`（带 direction 系数）数学等价。

- [ ] **Step 5: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib trial_balance`
Expected: PASS（3 个测试）

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/models.rs src-tauri/src/accounting.rs
git commit -m "feat(accounting): 科目余额表试算平衡引擎"
```

---

### Task 2: 试算平衡命令 + Excel 导出 + 前端 Tab

**Files:**
- Modify: `src-tauri/src/commands.rs`（新命令 `get_trial_balance`、`export_trial_balance`，放在财务报表命令附近）
- Modify: `src-tauri/src/lib.rs`（invoke_handler 注册 2 个命令）
- Modify: `src-tauri/src/excel.rs`（新 `export_trial_balance_excel`）
- Modify: `src/types/index.ts`（`TrialBalanceRow`、`TrialBalanceReport` 类型）
- Modify: `src/api/index.ts`（`getTrialBalance`、`exportTrialBalance` + mock case）
- Modify: `src/pages/FinancialReports.tsx`（第 4 个 Tab）
- Modify: `src/pages/OperationLogs.tsx`（导出日志中文映射）

**Interfaces:**
- Consumes: Task 1 的 `build_trial_balance`、`TrialBalanceReport`
- Produces: 前端 API `getTrialBalance(fromMonth, toMonth): Promise<TrialBalanceReport>`、`exportTrialBalance(fromMonth, toMonth, path): Promise<string>`；Tauri 命令名 `get_trial_balance`、`export_trial_balance`

- [ ] **Step 1: commands.rs 加命令**（参照现有 `export_financial_report` 命令的路径处理方式——先读该命令确认 dialog 路径参数模式，照抄）

```rust
#[tauri::command]
pub fn get_trial_balance(
    from_month: String,
    to_month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<TrialBalanceReport, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    accounting::build_trial_balance(&conn, &from_month, &to_month)
}

#[tauri::command]
pub fn export_trial_balance(
    from_month: String,
    to_month: String,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<String, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let report = accounting::build_trial_balance(&conn, &from_month, &to_month)?;
    excel::export_trial_balance_excel(&report, &path)?;
    db::log_operation(
        &conn,
        "export_trial_balance",
        &format!("导出科目余额表 {from_month}~{to_month}"),
        "system",
        None,
    )?;
    Ok(path)
}
```

- [ ] **Step 2: excel.rs 加导出**（参照 `export_financial_report` 同文件现有报表导出的样式风格——表头行 + 数据行 + 列宽）

```rust
pub fn export_trial_balance_excel(report: &TrialBalanceReport, path: &str) -> AppResult<()> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("科目余额表")?;
    let title = rust_xlsxwriter::Format::new().set_bold().set_font_size(14);
    let header = rust_xlsxwriter::Format::new().set_bold().set_border(1);
    let cell = rust_xlsxwriter::Format::new().set_border(1);
    let money = rust_xlsxwriter::Format::new()
        .set_border(1)
        .set_num_format("0.00");
    sheet.merge_range(0, 0, 0, 8, &format!("科目余额表（{} ~ {}）", report.from_month, report.to_month), &title)?;
    let headers = ["科目编码", "科目名称", "期初余额(借)", "期初余额(贷)", "本期发生(借)", "本期发生(贷)", "期末余额(借)", "期末余额(贷)", "类别"];
    for (i, h) in headers.iter().enumerate() {
        sheet.write_with_format(1, i as u16, *h, &header)?;
    }
    let mut r: u32 = 2;
    for row in &report.rows {
        sheet.write_with_format(r, 0, &row.code, &cell)?;
        sheet.write_with_format(r, 1, &row.name, &cell)?;
        sheet.write_number_with_format(r, 2, row.opening_debit, &money)?;
        sheet.write_number_with_format(r, 3, row.opening_credit, &money)?;
        sheet.write_number_with_format(r, 4, row.period_debit, &money)?;
        sheet.write_number_with_format(r, 5, row.period_credit, &money)?;
        sheet.write_number_with_format(r, 6, row.ending_debit, &money)?;
        sheet.write_number_with_format(r, 7, row.ending_credit, &money)?;
        sheet.write_with_format(r, 8, &row.category, &cell)?;
        r += 1;
    }
    sheet.write_with_format(r, 1, "合计", &header)?;
    let total_debit: f64 = report.rows.iter().map(|x| x.ending_debit).sum();
    let total_credit: f64 = report.rows.iter().map(|x| x.ending_credit).sum();
    sheet.write_number_with_format(r, 6, total_debit, &money)?;
    sheet.write_number_with_format(r, 7, total_credit, &money)?;
    for col in 0..9u16 {
        sheet.set_column_width(col, 14)?;
    }
    workbook.save(path)?;
    Ok(())
}
```

冒烟测试（excel.rs `mod tests`，如无测试模块则新建；临时文件用 `std::env::temp_dir().join("tb_smoke.xlsx")`）：

```rust
#[test]
fn test_export_trial_balance_smoke() {
    let report = TrialBalanceReport {
        from_month: "2026-01".into(),
        to_month: "2026-06".into(),
        enabled: true,
        balanced: true,
        rows: vec![TrialBalanceRow {
            code: "1001".into(),
            name: "库存现金".into(),
            category: "asset".into(),
            direction: "debit".into(),
            opening_debit: 1000.0,
            opening_credit: 0.0,
            period_debit: 0.0,
            period_credit: 100.0,
            ending_debit: 900.0,
            ending_credit: 0.0,
        }],
    };
    let path = std::env::temp_dir().join("trial_balance_smoke.xlsx");
    export_trial_balance_excel(&report, path.to_str().unwrap()).unwrap();
    assert!(path.exists());
    let _ = std::fs::remove_file(path);
}
```

- [ ] **Step 3: lib.rs 注册**

在 `generate_handler![...]` 列表（报表命令附近）追加：

```rust
commands::get_trial_balance,
commands::export_trial_balance,
```

- [ ] **Step 4: 后端验证**

Run: `cd src-tauri && cargo check`
Expected: 通过（无新 error）

- [ ] **Step 5: 前端类型与 API**

types/index.ts：

```typescript
export interface TrialBalanceRow {
  code: string;
  name: string;
  category: string;
  direction: string;
  opening_debit: number;
  opening_credit: number;
  period_debit: number;
  period_credit: number;
  ending_debit: number;
  ending_credit: number;
}

export interface TrialBalanceReport {
  from_month: string;
  to_month: string;
  enabled: boolean;
  rows: TrialBalanceRow[];
  balanced: boolean;
}
```

api/index.ts（与 `export_financial_report` 同区）：

```typescript
export async function getTrialBalance(fromMonth: string, toMonth: string) {
  return invoke<TrialBalanceReport>('get_trial_balance', { fromMonth, toMonth });
}

export async function exportTrialBalance(fromMonth: string, toMonth: string, path: string) {
  return invoke<string>('export_trial_balance', { fromMonth, toMonth, path });
}
```

（Tauri 参数名 snake_case→camelCase 自动转换：`from_month`/`to_month` 前端传 `fromMonth`/`toMonth`。）

mock case（api/index.ts mock 分支，`export_financial_report` case 旁）：

```typescript
case 'get_trial_balance':
  return {
    from_month: String(args?.fromMonth ?? ''),
    to_month: String(args?.toMonth ?? ''),
    enabled: true,
    rows: [],
    balanced: true,
  };
case 'export_trial_balance':
  return '';
```

OperationLogs.tsx 中文映射（`actionMap` 对象）加：`export_trial_balance: '导出科目余额表',`

- [ ] **Step 6: FinancialReports.tsx 加第 4 个 Tab**

页面顶部新增状态 `const [tb, setTb] = useState<TrialBalanceReport | null>(null);` 与区间选择（两个 DatePicker.MonthPicker，默认当年 1 月~当前月）。Tabs `items` 数组追加（key 类型 `FinancialReportType` 联合类型需加 `'trial_balance'`——读该类型定义处修改）：

```tsx
{
  key: 'trial_balance',
  label: '科目余额表',
  children: (
    <div>
      {!tb?.enabled && <Alert type="info" showIcon message="该区间早于启用月（或未录期初余额），报表为空" className="mb-16" />}
      {tb?.enabled && !tb.balanced && (
        <Alert type="error" showIcon message="试算不平衡：借贷合计存在差异，请检查凭证" className="mb-16" />
      )}
      <Button icon={<DownloadOutlined />} onClick={onExportTb} style={{ marginBottom: 12 }}>
        导出 Excel
      </Button>
      <Table
        rowKey="code"
        size="small"
        dataSource={tb?.rows ?? []}
        pagination={false}
        columns={[
          { title: '编码', dataIndex: 'code', width: 90 },
          { title: '科目名称', dataIndex: 'name' },
          { title: '期初(借)', dataIndex: 'opening_debit', render: (v: number) => renderAmount(v) },
          { title: '期初(贷)', dataIndex: 'opening_credit', render: (v: number) => renderAmount(v) },
          { title: '发生(借)', dataIndex: 'period_debit', render: (v: number) => renderAmount(v) },
          { title: '发生(贷)', dataIndex: 'period_credit', render: (v: number) => renderAmount(v) },
          { title: '期末(借)', dataIndex: 'ending_debit', render: (v: number) => renderAmount(v) },
          { title: '期末(贷)', dataIndex: 'ending_credit', render: (v: number) => renderAmount(v) },
        ]}
        summary={() => {
          const sum = (f: (r: TrialBalanceRow) => number) => (tb?.rows ?? []).reduce((a, r) => a + f(r), 0);
          return (
            <Table.Summary.Row>
              <Table.Summary.Cell index={0}>合计</Table.Summary.Cell>
              <Table.Summary.Cell index={1} colSpan={5} />
              <Table.Summary.Cell index={2}>{fmtMoney(sum((r) => r.ending_debit))}</Table.Summary.Cell>
              <Table.Summary.Cell index={3}>{fmtMoney(sum((r) => r.ending_credit))}</Table.Summary.Cell>
            </Table.Summary.Row>
          );
        }}
      />
    </div>
  ),
}
```

`renderAmount`/`fmtMoney` 沿用页面现有实现（脱敏渲染）。`onExportTb`：调 `save` 对话框选路径后 `exportTrialBalance`，成功 `message.success`（照抄现有报表导出按钮的实现模式）。区间变化或 Tab 切到 trial_balance 时调 `getTrialBalance`。

- [ ] **Step 7: 前端验证**

Run: `npx tsc --noEmit && npm run lint && npm run build`
Expected: 全部通过

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/src/excel.rs src/types/index.ts src/api/index.ts src/pages/FinancialReports.tsx src/pages/OperationLogs.tsx
git commit -m "feat(accounting): 科目余额表命令/Excel 导出与财务报表页 Tab"
```

---

### Task 3: 年末结转凭证生成/作废 + 报表口径排除

**Files:**
- Modify: `src-tauri/src/accounting.rs`（新 `generate_period_close_vouchers` / `void_period_close_vouchers`；`compute_balances`、`profit_loss_amounts`、`get_vouchers` 调用点加 source_type 排除）
- Test: `src-tauri/src/accounting.rs` `mod tests`

**Interfaces:**
- Consumes: `insert_voucher(&Connection, &VoucherDraft) -> AppResult<Voucher>`（accounting.rs:267）、`compute_balances`（accounting.rs:969）、`opening_month`（accounting.rs:951）
- Produces: `pub fn generate_period_close_vouchers(conn: &Connection, month: &str) -> AppResult<usize>`（返回生成凭证张数 0/1/2）、`pub fn void_period_close_vouchers(conn: &Connection, month: &str) -> AppResult<usize>`（Task 4 月结命令依赖）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn test_period_close_vouchers() {
    let conn = test_conn();
    // 启用月 + 收入 6001 贷 1000 / 费用 6602 借 400 的凭证
    conn.execute("INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount) VALUES ('2025-01', '1001', 0.0, 0.0)", []).unwrap();
    conn.execute("INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status) VALUES ('记-202512-001', '2025-12-05', '2025-12', 'bank_manual', 1, 1000.0, 'active')", []).unwrap();
    let vid: i64 = conn.query_row("SELECT id FROM vouchers WHERE voucher_no='记-202512-001'", [], |r| r.get(0)).unwrap();
    conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '6001', 0.0, 1000.0)", [vid]).unwrap();
    conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '1001', 1000.0, 0.0)", [vid]).unwrap();
    conn.execute("INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status) VALUES ('记-202512-002', '2025-12-06', '2025-12', 'bank_manual', 2, 400.0, 'active')", []).unwrap();
    let vid2: i64 = conn.query_row("SELECT id FROM vouchers WHERE voucher_no='记-202512-002'", [], |r| r.get(0)).unwrap();
    conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '6602', 400.0, 0.0)", [vid2]).unwrap();
    conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '1001', 0.0, 400.0)", [vid2]).unwrap();

    let n = generate_period_close_vouchers(&conn, "2025-12").unwrap();
    assert_eq!(n, 2); // 结转损益 + 结转本年利润
    // 结转后 3103 余额为 0、3104 余额 600（含结转凭证的账面）
    let tb = build_trial_balance(&conn, "2025-12", "2025-12").unwrap();
    let p3103 = tb.rows.iter().find(|r| r.code == "3103").unwrap();
    assert_eq!(p3103.ending_debit, 0.0);
    assert_eq!(p3103.ending_credit, 0.0);
    let p3104 = tb.rows.iter().find(|r| r.code == "3104").unwrap();
    assert_eq!(p3104.ending_credit, 600.0);
    // 报表口径排除 period_close：利润表累计收入仍 1000、费用 400
    let income = build_income_statement(&conn, "2025-12").unwrap();
    let rev = income.rows.iter().find(|r| r.key == "6001").unwrap();
    assert_eq!(rev.comparative, 1000.0);
    // 幂等：再次生成被唯一索引拒绝（返回 0，跳过）
    let n2 = generate_period_close_vouchers(&conn, "2025-12").unwrap();
    assert_eq!(n2, 0);

    // 反月结作废
    let voided = void_period_close_vouchers(&conn, "2025-12").unwrap();
    assert_eq!(voided, 2);
}

#[test]
fn test_period_close_skips_non_december_and_zero() {
    let conn = test_conn();
    conn.execute("INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount) VALUES ('2025-01', '1001', 0.0, 0.0)", []).unwrap();
    assert_eq!(generate_period_close_vouchers(&conn, "2025-11").unwrap(), 0);
    assert_eq!(generate_period_close_vouchers(&conn, "2025-12").unwrap(), 0); // 无损益凭证
}
```

（`build_income_statement` 若私有需改 `pub`——检查后统一。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib period_close`
Expected: FAIL（函数未定义）

- [ ] **Step 3: 实现生成与作废**

```rust
/// 年末结转：12 月月结时调用。凭证① 各损益科目余额 → 3103；凭证② 3103 余额 → 3104。
/// source_type='period_close'，凭证① source_id=YYYYMM*10+1、凭证② YYYYMM*10+2（避开部分唯一索引）。
/// 全年损益净额为零或非 12 月返回 0。
/// 报表口径统一排除 period_close（见 compute_balances / profit_loss_amounts）。
pub fn generate_period_close_vouchers(conn: &Connection, month: &str) -> AppResult<usize> {
    if !month.ends_with("-12") {
        return Ok(0);
    }
    // 全年（启用月~12 月）各 profit_loss 科目净额（排除已有 period_close）
    let open_month = opening_month(conn).unwrap_or_else(|| format!("{}-01", &month[..4]));
    let mut stmt = conn.prepare(
        "SELECT vl.account_code, SUM(vl.debit_amount - vl.credit_amount)
         FROM voucher_lines vl JOIN vouchers v ON v.id = vl.voucher_id
         WHERE v.belong_month >= ?1 AND v.belong_month <= ?2 AND v.status = 'active'
           AND v.source_type != 'period_close'
           AND vl.account_code IN (SELECT code FROM gl_accounts WHERE category = 'profit_loss')
         GROUP BY vl.account_code",
    )?;
    let nets = stmt
        .query_map(params![open_month, month], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    let month_id: i64 = month.replace('-', "").parse().unwrap_or(0);
    let mut created = 0;
    let net_total: f64 = nets.iter().map(|(_, v)| v).sum();
    if !nets.is_empty() {
        // 凭证①：收入类（净额>0 贷方余额）借记结平、费用类（净额<0）贷记结平，贷/借 3103
        let mut lines: Vec<VoucherLineDraft> = Vec::new();
        for (code, net) in &nets {
            if *net > 0.0 {
                lines.push(VoucherLineDraft {
                    account_code: code.clone(),
                    debit_amount: *net,
                    credit_amount: 0.0,
                    summary: Some(format!("{month} 年末结转损益（{code}）")),
                });
            } else if *net < 0.0 {
                lines.push(VoucherLineDraft {
                    account_code: code.clone(),
                    debit_amount: 0.0,
                    credit_amount: -*net,
                    summary: Some(format!("{month} 年末结转损益（{code}）")),
                });
            }
        }
        if net_total >= 0.0 {
            lines.push(VoucherLineDraft {
                account_code: "3103".into(),
                debit_amount: 0.0,
                credit_amount: net_total,
                summary: Some(format!("{month} 结转本年利润")),
            });
        } else {
            lines.push(VoucherLineDraft {
                account_code: "3103".into(),
                debit_amount: -net_total,
                credit_amount: 0.0,
                summary: Some(format!("{month} 结转本年亏损")),
            });
        }
        insert_voucher(
            conn,
            &VoucherDraft {
                belong_month: month.to_string(),
                voucher_date: format!("{month}-31"),
                source_type: "period_close".into(),
                source_id: month_id * 10 + 1,
                remark: Some(format!("{month} 年末损益结转")),
                lines,
            },
        )?;
        created += 1;
    }
    if net_total.abs() >= 0.005 {
        // 凭证②：3103 → 3104
        let (debit_code, credit_code) = if net_total >= 0.0 {
            ("3103", "3104")
        } else {
            ("3104", "3103")
        };
        let amount = net_total.abs();
        insert_voucher(
            conn,
            &VoucherDraft {
                belong_month: month.to_string(),
                voucher_date: format!("{month}-31"),
                source_type: "period_close".into(),
                source_id: month_id * 10 + 2,
                remark: Some(format!("{month} 本年利润结转未分配利润")),
                lines: vec![
                    VoucherLineDraft {
                        account_code: debit_code.into(),
                        debit_amount: amount,
                        credit_amount: 0.0,
                        summary: Some(format!("{month} 结转未分配利润")),
                    },
                    VoucherLineDraft {
                        account_code: credit_code.into(),
                        debit_amount: 0.0,
                        credit_amount: amount,
                        summary: Some(format!("{month} 结转未分配利润")),
                    },
                ],
            },
        )?;
        created += 1;
    }
    Ok(created)
}

/// 反月结联动：作废该月全部 period_close 凭证（按 belong_month + source_type，覆盖两张）。
pub fn void_period_close_vouchers(conn: &Connection, month: &str) -> AppResult<usize> {
    let n = conn.execute(
        "UPDATE vouchers SET status='void', updated_at=?2
         WHERE source_type='period_close' AND belong_month=?1 AND status='active'",
        params![month, Utc::now().to_rfc3339()],
    )?;
    Ok(n)
}
```

（测试中 `assert_eq!(voided, 2)` 对应两张。作废路径绕过 `void_vouchers_for_source` 的单 source_id 限制，直接按月批量作废。）

- [ ] **Step 4: 报表口径排除 period_close**

三处 SQL 的 `WHERE` 追加 `AND v.source_type != 'period_close'`：
1. `compute_balances`（accounting.rs:1014-1018 区间前滚入 SQL 与 1037-1041 当月发生 SQL，共两条）
2. `profit_loss_amounts`（1100-1105 当月 SQL 与 1117-1122 年累计 SQL，共两条）
3. `build_cash_flow_statement`（1363 行经 `get_vouchers` 取凭证——VoucherQuery 无 source_type 过滤时在函数内加过滤：`for v in &vouchers { if v.source_type == "period_close" { continue; } ... }`，注意 `Voucher` 结构的 source_type 字段是否存在——`vouchers` 表有该列，模型必有）

现金流量表兜底正确性：period_close 凭证不涉现金科目，跳过只为口径统一。

- [ ] **Step 5: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib`
Expected: 全部通过（含既有 119+ 个测试无回归——排除条件不改变现有行为，因为现在还没有 period_close 凭证）

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/accounting.rs
git commit -m "feat(accounting): 年末结转凭证生成与作废、报表口径排除 period_close"
```

---

### Task 4: 月结/反月结挂接年末结转 + 工作台检查项

**Files:**
- Modify: `src-tauri/src/commands.rs`（`close_month`、`reopen_month` 命令，约 630-665 行）
- Modify: `src-tauri/src/db.rs`（`get_month_close_workbench` 约 1917 行前加检查项）
- Test: `src-tauri/src/commands.rs` 不便单测（tauri::State），在 db.rs 或通过集成方式验证——测试写在 db.rs `mod tests`（若 close 逻辑在命令层，测试覆盖 db::close_month 不变 + workbench 检查项）

**Interfaces:**
- Consumes: Task 3 的 `generate_period_close_vouchers` / `void_period_close_vouchers`
- Produces: 12 月月结自动结转；workbench 检查项 key `period_close`（前端 MonthClose 页通用渲染检查项，无需改前端）

- [ ] **Step 1: 写失败测试（db.rs）**

```rust
#[test]
fn test_december_workbench_has_period_close_check() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let wb = get_month_close_workbench(&conn, "2025-12").unwrap();
    assert!(wb.checks.iter().any(|c| c.key == "period_close"));
    let wb_nov = get_month_close_workbench(&conn, "2025-11").unwrap();
    assert!(!wb_nov.checks.iter().any(|c| c.key == "period_close"));
}
```

（init_db 函数名以 db.rs 现有初始化函数为准——测试模块现有建库 helper 照抄。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib period_close_check`
Expected: FAIL

- [ ] **Step 3: 实现**

db.rs `get_month_close_workbench`（现有两个 checks.push 之后，1917 行 `let month_close = ...` 前）：

```rust
    if month.ends_with("-12") {
        checks.push(MonthCloseCheckItem {
            key: "period_close".to_string(),
            title: "年末结转".to_string(),
            status: "ok".to_string(),
            count: 0,
            description: "12 月正式月结时将自动生成年末损益结转凭证".to_string(),
            action_route: None,
        });
    }
```

commands.rs `close_month`（现有 `db::close_month(&tx, ...)?` 成功后、`log_operation` 前）：

```rust
    if data.month.ends_with("-12") {
        let n = accounting::generate_period_close_vouchers(&tx, &data.month)?;
        if n > 0 {
            db::log_operation(
                &tx,
                "period_close_vouchers",
                &format!("{month} 年末结转凭证 {n} 张", month = data.month),
                "system",
                None,
            )?;
        }
    }
```

commands.rs `reopen_month`（`db::reopen_month` 调用前）：

```rust
    if data.month.ends_with("-12") {
        accounting::void_period_close_vouchers(&tx, &data.month)?;
    }
```

OperationLogs.tsx 映射加：`period_close_vouchers: '年末结转凭证',`

- [ ] **Step 4: 跑测试 + 全量后端验证**

Run: `cd src-tauri && cargo fmt && cargo check && cargo test --lib`
Expected: 测试全过

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/db.rs src/pages/OperationLogs.tsx
git commit -m "feat(accounting): 12 月月结自动年末结转与反月结作废联动"
```

---

### Task 5: 社保台账 DDL + 模型 + CRUD

**Files:**
- Modify: `src-tauri/src/db.rs`（DDL 建表 + ensure_column + CRUD 函数）
- Modify: `src-tauri/src/models.rs`（`SocialInsuranceProfile`、`SocialInsuranceProfileInput`）

**Interfaces:**
- Produces: 表 `social_insurance_profiles`；列 `salary_monthly_results.social_security_employer / housing_fund_employer REAL DEFAULT 0`；db 函数 `get_social_profiles(conn, year: i64) -> AppResult<Vec<SocialInsuranceProfile>>`、`upsert_social_profile(conn, input: &SocialInsuranceProfileInput) -> AppResult<SocialInsuranceProfile>`、`delete_social_profile(conn, id: i64) -> AppResult<bool>`、`copy_social_profiles(conn, from_year: i64, to_year: i64, factor: f64, apply_clamp: bool) -> AppResult<usize>`、`get_social_base_limits(conn) -> AppResult<(f64, f64, f64, f64)>`、`set_social_base_limits(conn, ss_min, ss_max, hf_min, hf_max) -> AppResult<()>`（Task 6/7 依赖）

- [ ] **Step 1: DDL**

db.rs `create_tables` 的 execute_batch 中（`account_mappings` 表后）加：

```sql
CREATE TABLE IF NOT EXISTS social_insurance_profiles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    employee_no TEXT NOT NULL,
    profile_year INTEGER NOT NULL,
    ss_base REAL DEFAULT 0,
    hf_base REAL DEFAULT 0,
    ss_employer_rate REAL DEFAULT 0,
    ss_personal_rate REAL DEFAULT 0,
    hf_employer_rate REAL DEFAULT 0,
    hf_personal_rate REAL DEFAULT 0,
    remark TEXT,
    created_at TEXT,
    updated_at TEXT,
    UNIQUE(employee_no, profile_year)
);
```

迁移区（ensure_column 调用区，约 546 行后）加：

```rust
    ensure_column(
        conn,
        "salary_monthly_results",
        "social_security_employer",
        "REAL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "salary_monthly_results",
        "housing_fund_employer",
        "REAL DEFAULT 0",
    )?;
```

- [ ] **Step 2: models.rs**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialInsuranceProfile {
    pub id: i64,
    pub employee_no: String,
    pub profile_year: i64,
    pub ss_base: f64,
    pub hf_base: f64,
    pub ss_employer_rate: f64,
    pub ss_personal_rate: f64,
    pub hf_employer_rate: f64,
    pub hf_personal_rate: f64,
    pub remark: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialInsuranceProfileInput {
    pub id: Option<i64>,
    pub employee_no: String,
    pub profile_year: i64,
    pub ss_base: Option<f64>,
    pub hf_base: Option<f64>,
    pub ss_employer_rate: Option<f64>,
    pub ss_personal_rate: Option<f64>,
    pub hf_employer_rate: Option<f64>,
    pub hf_personal_rate: Option<f64>,
    pub remark: Option<String>,
}
```

- [ ] **Step 3: 写失败测试**

```rust
#[test]
fn test_social_profile_crud_and_copy() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let input = SocialInsuranceProfileInput {
        id: None,
        employee_no: "E001".into(),
        profile_year: 2026,
        ss_base: Some(8000.0),
        hf_base: Some(8000.0),
        ss_employer_rate: Some(0.24),
        ss_personal_rate: Some(0.105),
        hf_employer_rate: Some(0.12),
        hf_personal_rate: Some(0.12),
        remark: None,
    };
    let saved = upsert_social_profile(&conn, &input).unwrap();
    assert!(saved.id > 0);
    // 同员工同年度唯一
    assert!(upsert_social_profile(&conn, &input).is_err());
    // 上下限
    set_social_base_limits(&conn, 4590.0, 22950.0, 0.0, 0.0).unwrap();
    assert_eq!(get_social_base_limits(&conn).unwrap(), (4590.0, 22950.0, 0.0, 0.0));
    // 调基复制：2027 基数上浮 5% 并 clamp
    let n = copy_social_profiles(&conn, 2026, 2027, 1.05, true).unwrap();
    assert_eq!(n, 1);
    let rows = get_social_profiles(&conn, 2027).unwrap();
    assert_eq!(rows[0].ss_base, 8400.0);
    // 目标年度已存在时拒绝
    assert!(copy_social_profiles(&conn, 2026, 2027, 1.05, true).is_err());
    assert!(delete_social_profile(&conn, saved.id).unwrap());
}

#[test]
fn test_clamp_base() {
    assert_eq!(clamp_base(3000.0, 4590.0, 22950.0), 4590.0);
    assert_eq!(clamp_base(30000.0, 4590.0, 22950.0), 22950.0);
    assert_eq!(clamp_base(10000.0, 4590.0, 22950.0), 10000.0);
    assert_eq!(clamp_base(10000.0, 0.0, 0.0), 10000.0); // 0 = 不限制
}
```

- [ ] **Step 4: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib social_profile`
Expected: FAIL

- [ ] **Step 5: 实现 CRUD**

db.rs（Payment Batches 区域后新区域 `==== Social Insurance Profiles ====`）：

```rust
fn clamp_base(value: f64, min: f64, max: f64) -> f64 {
    let mut v = value;
    if min > 0.0 && v < min {
        v = min;
    }
    if max > 0.0 && v > max {
        v = max;
    }
    v
}

pub fn get_social_base_limits(
    conn: &Connection,
) -> AppResult<(f64, f64, f64, f64)> {
    let parse = |key: &str| -> f64 {
        get_setting(conn, key)
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0)
    };
    Ok((
        parse("ss_base_min"),
        parse("ss_base_max"),
        parse("hf_base_min"),
        parse("hf_base_max"),
    ))
}

pub fn set_social_base_limits(
    conn: &Connection,
    ss_min: f64,
    ss_max: f64,
    hf_min: f64,
    hf_max: f64,
) -> AppResult<()> {
    set_setting(conn, "ss_base_min", &ss_min.to_string())?;
    set_setting(conn, "ss_base_max", &ss_max.to_string())?;
    set_setting(conn, "hf_base_min", &hf_min.to_string())?;
    set_setting(conn, "hf_base_max", &hf_max.to_string())?;
    Ok(())
}

pub fn get_social_profiles(conn: &Connection, year: i64) -> AppResult<Vec<SocialInsuranceProfile>> {
    let mut stmt = conn.prepare(
        "SELECT id, employee_no, profile_year, ss_base, hf_base, ss_employer_rate,
                ss_personal_rate, hf_employer_rate, hf_personal_rate, remark, created_at, updated_at
         FROM social_insurance_profiles WHERE profile_year = ?1 ORDER BY employee_no",
    )?;
    let rows = stmt
        .query_map(params![year], |r| {
            Ok(SocialInsuranceProfile {
                id: r.get(0)?,
                employee_no: r.get(1)?,
                profile_year: r.get(2)?,
                ss_base: r.get(3)?,
                hf_base: r.get(4)?,
                ss_employer_rate: r.get(5)?,
                ss_personal_rate: r.get(6)?,
                hf_employer_rate: r.get(7)?,
                hf_personal_rate: r.get(8)?,
                remark: r.get(9)?,
                created_at: r.get(10)?,
                updated_at: r.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn upsert_social_profile(
    conn: &Connection,
    input: &SocialInsuranceProfileInput,
) -> AppResult<SocialInsuranceProfile> {
    if input.employee_no.trim().is_empty() {
        return Err(AppError::InvalidParam("员工工号必填".into()));
    }
    for rate in [
        input.ss_employer_rate,
        input.ss_personal_rate,
        input.hf_employer_rate,
        input.hf_personal_rate,
    ] {
        if let Some(r) = rate {
            if !(0.0..=1.0).contains(&r) {
                return Err(AppError::InvalidParam("费率必须在 0~1 之间".into()));
            }
        }
    }
    let exists: Option<i64> = if let Some(id) = input.id {
        conn.query_row(
            "SELECT id FROM social_insurance_profiles WHERE id = ?1 AND employee_no = ?2 AND profile_year = ?3",
            params![id, input.employee_no, input.profile_year],
            |r| r.get(0),
        )
        .ok()
    } else {
        None
    };
    let now = Utc::now().to_rfc3339();
    let ss_base = input.ss_base.unwrap_or(0.0);
    let hf_base = input.hf_base.unwrap_or(0.0);
    let (ss_e, ss_p, hf_e, hf_p) = (
        input.ss_employer_rate.unwrap_or(0.0),
        input.ss_personal_rate.unwrap_or(0.0),
        input.hf_employer_rate.unwrap_or(0.0),
        input.hf_personal_rate.unwrap_or(0.0),
    );
    let id = match exists {
        Some(id) => {
            conn.execute(
                "UPDATE social_insurance_profiles SET ss_base=?1, hf_base=?2, ss_employer_rate=?3,
                 ss_personal_rate=?4, hf_employer_rate=?5, hf_personal_rate=?6, remark=?7, updated_at=?8
                 WHERE id=?9",
                params![ss_base, hf_base, ss_e, ss_p, hf_e, hf_p, input.remark, now, id],
            )?;
            id
        }
        None => {
            // 同员工同年度已存在（不带 id 的重复保存）报错
            let dup: i64 = conn.query_row(
                "SELECT COUNT(*) FROM social_insurance_profiles WHERE employee_no=?1 AND profile_year=?2",
                params![input.employee_no, input.profile_year],
                |r| r.get(0),
            )?;
            if dup > 0 {
                return Err(AppError::InvalidParam(format!(
                    "{} 的 {} 年度台账已存在",
                    input.employee_no, input.profile_year
                )));
            }
            conn.execute(
                "INSERT INTO social_insurance_profiles
                 (employee_no, profile_year, ss_base, hf_base, ss_employer_rate, ss_personal_rate,
                  hf_employer_rate, hf_personal_rate, remark, created_at, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
                params![input.employee_no, input.profile_year, ss_base, hf_base, ss_e, ss_p, hf_e, hf_p, input.remark, now],
            )?;
            conn.last_insert_rowid()
        }
    };
    Ok(SocialInsuranceProfile {
        id,
        employee_no: input.employee_no.clone(),
        profile_year: input.profile_year,
        ss_base,
        hf_base,
        ss_employer_rate: ss_e,
        ss_personal_rate: ss_p,
        hf_employer_rate: hf_e,
        hf_personal_rate: hf_p,
        remark: input.remark.clone(),
        created_at: Some(now.clone()),
        updated_at: Some(now),
    })
}

pub fn delete_social_profile(conn: &Connection, id: i64) -> AppResult<bool> {
    Ok(conn.execute(
        "DELETE FROM social_insurance_profiles WHERE id = ?1",
        params![id],
    )? > 0)
}

/// 年度调基：复制 from_year 全部台账到 to_year，基数 ×factor 后按上下限 clamp。
/// to_year 已有任何台账时拒绝（避免覆盖）。
pub fn copy_social_profiles(
    conn: &Connection,
    from_year: i64,
    to_year: i64,
    factor: f64,
    apply_clamp: bool,
) -> AppResult<usize> {
    if from_year == to_year {
        return Err(AppError::InvalidParam("调基源年度与目标年度相同".into()));
    }
    let existing: i64 = conn.query_row(
        "SELECT COUNT(*) FROM social_insurance_profiles WHERE profile_year = ?1",
        params![to_year],
        |r| r.get(0),
    )?;
    if existing > 0 {
        return Err(AppError::InvalidParam(format!(
            "{to_year} 年度已存在台账，如需重新调基请先清空该年度"
        )));
    }
    let source = get_social_profiles(conn, from_year)?;
    if source.is_empty() {
        return Err(AppError::InvalidParam(format!("{from_year} 年度无台账可复制")));
    }
    let (ss_min, ss_max, hf_min, hf_max) = get_social_base_limits(conn)?;
    let now = Utc::now().to_rfc3339();
    let mut n = 0;
    for p in &source {
        let (ss, hf) = if apply_clamp {
            (
                clamp_base(p.ss_base * factor, ss_min, ss_max),
                clamp_base(p.hf_base * factor, hf_min, hf_max),
            )
        } else {
            (p.ss_base * factor, p.hf_base * factor)
        };
        conn.execute(
            "INSERT INTO social_insurance_profiles
             (employee_no, profile_year, ss_base, hf_base, ss_employer_rate, ss_personal_rate,
              hf_employer_rate, hf_personal_rate, remark, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
            params![p.employee_no, to_year, ss, hf, p.ss_employer_rate, p.ss_personal_rate,
                    p.hf_employer_rate, p.hf_personal_rate, p.remark, now],
        )?;
        n += 1;
    }
    Ok(n)
}
```

- [ ] **Step 6: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib social`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/db.rs src-tauri/src/models.rs
git commit -m "feat(insurance): 社保公积金台账表/模型/CRUD 与调基复制"
```

---

### Task 6: 社保台账命令 + 前端页面

**Files:**
- Modify: `src-tauri/src/commands.rs`（6 个命令）
- Modify: `src-tauri/src/lib.rs`（注册）
- Modify: `src/types/index.ts`、`src/api/index.ts`（类型 + API + mock）
- Create: `src/pages/SocialInsurance.tsx`
- Modify: `src/App.tsx`（路由 + 菜单"薪酬核算"组）
- Modify: `src/pages/OperationLogs.tsx`（映射）

**Interfaces:**
- Consumes: Task 5 的 db 函数
- Produces: Tauri 命令 `get_social_profiles(year)`、`save_social_profile(data)`、`delete_social_profile(id)`、`copy_social_profiles(fromYear, toYear, factor, applyClamp)`、`get_social_base_limits()`、`set_social_base_limits(ssMin, ssMax, hfMin, hfMax)`；路由 `/social-insurance`

- [ ] **Step 1: commands.rs 命令**（全部只读/写库，参照现有 `get_employees` 模式；写操作加 `log_operation`）

```rust
#[tauri::command]
pub fn get_social_profiles(
    year: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<SocialInsuranceProfile>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_social_profiles(&conn, year)
}

#[tauri::command]
pub fn save_social_profile(
    data: SocialInsuranceProfileInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<SocialInsuranceProfile, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::upsert_social_profile(&conn, &data)?;
    db::log_operation(
        &conn,
        "save_social_profile",
        &format!("保存社保台账 {}-{}", result.employee_no, result.profile_year),
        "system",
        None,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn delete_social_profile(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let ok = db::delete_social_profile(&conn, id)?;
    db::log_operation(&conn, "delete_social_profile", &format!("删除社保台账 #{id}"), "system", None)?;
    Ok(ok)
}

#[tauri::command]
pub fn copy_social_profiles(
    from_year: i64,
    to_year: i64,
    factor: f64,
    apply_clamp: bool,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<usize, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let n = db::copy_social_profiles(&conn, from_year, to_year, factor, apply_clamp)?;
    db::log_operation(
        &conn,
        "copy_social_profiles",
        &format!("{from_year} 调基复制到 {to_year} 共 {n} 条"),
        "system",
        None,
    )?;
    Ok(n)
}

#[tauri::command]
pub fn get_social_base_limits(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<f64>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let (a, b, c, d) = db::get_social_base_limits(&conn)?;
    Ok(vec![a, b, c, d])
}

#[tauri::command]
pub fn set_social_base_limits(
    ss_min: f64,
    ss_max: f64,
    hf_min: f64,
    hf_max: f64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<(), AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::set_social_base_limits(&conn, ss_min, ss_max, hf_min, hf_max)?;
    db::log_operation(&conn, "set_social_base_limits", "保存社保基数上下限", "system", None)?;
    Ok(())
}
```

lib.rs 注册 6 个命令。

- [ ] **Step 2: 前端类型 + API + mock**

types：

```typescript
export interface SocialInsuranceProfile {
  id: number;
  employee_no: string;
  profile_year: number;
  ss_base: number;
  hf_base: number;
  ss_employer_rate: number;
  ss_personal_rate: number;
  hf_employer_rate: number;
  hf_personal_rate: number;
  remark: string | null;
  created_at: string | null;
  updated_at: string | null;
}

export interface SocialInsuranceProfileInput {
  id?: number;
  employee_no: string;
  profile_year: number;
  ss_base?: number;
  hf_base?: number;
  ss_employer_rate?: number;
  ss_personal_rate?: number;
  hf_employer_rate?: number;
  hf_personal_rate?: number;
  remark?: string;
}
```

api（`get_social_profiles` 命令名与 db 函数同名——invoke 命令名 `get_social_profiles`）：

```typescript
export async function getSocialProfiles(year: number) {
  return invoke<SocialInsuranceProfile[]>('get_social_profiles', { year });
}
export async function saveSocialProfile(data: SocialInsuranceProfileInput) {
  return invoke<SocialInsuranceProfile>('save_social_profile', { data });
}
export async function deleteSocialProfile(id: number) {
  return invoke<boolean>('delete_social_profile', { id });
}
export async function copySocialProfiles(fromYear: number, toYear: number, factor: number, applyClamp: boolean) {
  return invoke<number>('copy_social_profiles', { fromYear, toYear, factor, applyClamp });
}
export async function getSocialBaseLimits() {
  return invoke<number[]>('get_social_base_limits');
}
export async function setSocialBaseLimits(ssMin: number, ssMax: number, hfMin: number, hfMax: number) {
  return invoke<void>('set_social_base_limits', { ssMin, ssMax, hfMin, hfMax });
}
```

mock case：`get_social_profiles` → `[]`；`get_social_base_limits` → `[0,0,0,0]`；`save_social_profile` → throw 预览不支持；`copy_social_profiles`/`set_social_base_limits`/`delete_social_profile` → throw 或 true（delete 走 default true 即可，不写 case）。

- [ ] **Step 3: SocialInsurance.tsx 页面**

结构（参照 ChartOfAccounts.tsx 的页面骨架：标题 Card + 工具栏 + Table + Modal 表单）：

- 年度 `Select`（当年~2028）+ "新增台账"按钮 + "年度调基"按钮 + "基数上下限"按钮
- Table 列：工号、社保基数、公积金基数、社保单位率、社保个人率、公积金单位率、公积金个人率、备注、操作（编辑/删除 Popconfirm）
- 新增/编辑 Modal：`Form` 工号（新增可输、编辑禁用）、年度、基数与 4 费率 `InputNumber`、备注；费率以百分数展示（输入 24 存 0.24——表单 `getValueProps={(v)=>({value: v==null?undefined:v*100})}` 转换，或直接小数输入，取简：直接小数输入 + placeholder "0.24"）
- 调基 Modal：from_year（默认当前年度-1）、to_year（默认当前年度）、factor（InputNumber 默认 1.0）、apply_clamp（Switch 默认开）→ 调 `copySocialProfiles` → message.success + 刷新
- 上下限 Modal：4 个 InputNumber（0 表示不限制）→ `setSocialBaseLimits`

- [ ] **Step 4: App.tsx 路由与菜单**

Routes 加：`<Route path="/social-insurance" element={<SocialInsurance />} />`
菜单"薪酬核算"组（90-92 行附近）加：`{ key: '/social-insurance', label: '社保台账', icon: <SafetyCertificateOutlined /> }`（icon 从 antd 图标按需 import）

OperationLogs 映射：`save_social_profile: '保存社保台账', delete_social_profile: '删除社保台账', copy_social_profiles: '年度调基', set_social_base_limits: '保存基数上下限',`

- [ ] **Step 5: 验证 + Commit**

Run: `cd src-tauri && cargo check && npx tsc --noEmit && npm run lint && npm run build`
Expected: 全过

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/types/index.ts src/api/index.ts src/pages/SocialInsurance.tsx src/App.tsx src/pages/OperationLogs.tsx
git commit -m "feat(insurance): 社保台账命令与页面（年度切换/调基/上下限）"
```

---

### Task 7: 工资计算挂接社保台账

**Files:**
- Modify: `src-tauri/src/salary.rs`（`calculate_single_employee`，约 132-240 行）
- Modify: `src-tauri/src/db.rs`（`save_salary_result`、`row_to_salary_result`（或同名映射函数）加 2 列）
- Modify: `src-tauri/src/models.rs`（`SalaryResult` 加 2 字段）
- Test: `src-tauri/src/salary.rs` 或 db.rs `mod tests`

**Interfaces:**
- Consumes: Task 5 的 `get_social_profiles`、`clamp_base`、`get_social_base_limits`
- Produces: `SalaryResult.social_security_employer: f64`、`SalaryResult.housing_fund_employer: f64`（Task 8 凭证依赖）；无台账年份回退现状且单位部分为 0

- [ ] **Step 1: models.rs 加字段**

`SalaryResult`（models.rs:127）`social_security_personal` 前加：

```rust
    pub social_security_employer: f64,
    pub housing_fund_employer: f64,
```

（同时找所有构造 `SalaryResult { ... }` 的位置——cargo check 会报缺字段，逐一补 0.0 或真实值。）

- [ ] **Step 2: db.rs 读写加列**

`save_salary_result` 的 INSERT 列清单加 `social_security_employer, housing_fund_employer` 与值；行映射函数（搜 `social_security_personal` 在 db.rs 的 `row.get` 位置）同步加 `row.get(n)?` 两列（索引顺延）。`update_salary_result` 若有 SET 列清单也补。

- [ ] **Step 3: 写失败测试**（salary.rs tests——现有测试模块在哪就放哪；无则建）

```rust
#[test]
fn test_salary_uses_profile_and_clamp() {
    let conn = Connection::open_in_memory().unwrap();
    db::init_db(&conn).unwrap();
    conn.execute("INSERT INTO employees (employee_no, name, status, base_salary, position_salary, performance_salary, social_security_base, housing_fund_base, special_deduction) VALUES ('E001','张三','active',10000.0,0.0,0.0,0.0,0.0,0.0)", []).unwrap();
    // 2026 台账：ss_base 8000（超上限 7000 → clamp），单位率 0.24/0.12
    conn.execute("INSERT INTO social_insurance_profiles (employee_no, profile_year, ss_base, hf_base, ss_employer_rate, ss_personal_rate, hf_employer_rate, hf_personal_rate) VALUES ('E001', 2026, 8000.0, 8000.0, 0.24, 0.105, 0.12, 0.12)", []).unwrap();
    db::set_social_base_limits(&conn, 4590.0, 7000.0, 0.0, 0.0).unwrap();
    // 调 calculate_single_employee（私有）——经公开入口触发：计算 2026-01 工资
    // 具体入口以 salary.rs 现有 pub fn calculate/save 流程为准（读文件确认函数名后调用）
    // 断言：
    // social_security_personal = 7000 * 0.105 = 735
    // social_security_employer = 7000 * 0.24 = 1680
    // housing_fund_employer = 8000 * 0.12 = 960
}

#[test]
fn test_salary_falls_back_without_profile() {
    // 无 2027 台账：基数回退 base_salary（10000）、费率回退 salary_rules 默认 0.105/0.12，
    // social_security_employer = 0、housing_fund_employer = 0
}
```

（执行者须先读 salary.rs 找到"计算整月工资"的公开函数名——`calculate_month_salary` 类似命名——把测试改为经公开入口触发，断言落到 `SalaryResult` 新字段。）

- [ ] **Step 4: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib salary_uses_profile`
Expected: FAIL（单位字段 0 或断言不符）

- [ ] **Step 5: 实现**

`calculate_single_employee`（salary.rs:168-183 替换）：

```rust
    // 社保公积金：优先取年度台账（含上下限 clamp），无台账回退员工基数/全局费率
    let profile_year: i64 = month.get(0..4).and_then(|y| y.parse().ok()).unwrap_or(0);
    let profile: Option<SocialInsuranceProfile> = conn
        .query_row(
            "SELECT id, employee_no, profile_year, ss_base, hf_base, ss_employer_rate,
                    ss_personal_rate, hf_employer_rate, hf_personal_rate, remark, created_at, updated_at
             FROM social_insurance_profiles WHERE employee_no = ?1 AND profile_year = ?2",
            params![emp.employee_no, profile_year],
            |r| {
                Ok(SocialInsuranceProfile {
                    id: r.get(0)?,
                    employee_no: r.get(1)?,
                    profile_year: r.get(2)?,
                    ss_base: r.get(3)?,
                    hf_base: r.get(4)?,
                    ss_employer_rate: r.get(5)?,
                    ss_personal_rate: r.get(6)?,
                    hf_employer_rate: r.get(7)?,
                    hf_personal_rate: r.get(8)?,
                    remark: r.get(9)?,
                    created_at: r.get(10)?,
                    updated_at: r.get(11)?,
                })
            },
        )
        .ok();
    let (ss_min, ss_max, hf_min, hf_max) = db::get_social_base_limits(conn)?;
    let (social_security_base, housing_fund_base, ss_personal_rate, hf_personal_rate, ss_employer_rate, hf_employer_rate) =
        match &profile {
            Some(p) => (
                db::clamp_base(p.ss_base, ss_min, ss_max),
                db::clamp_base(p.hf_base, hf_min, hf_max),
                p.ss_personal_rate,
                p.hf_personal_rate,
                p.ss_employer_rate,
                p.hf_employer_rate,
            ),
            None => (
                if emp.social_security_base > 0.0 { emp.social_security_base } else { base_salary },
                if emp.housing_fund_base > 0.0 { emp.housing_fund_base } else { base_salary },
                social_security_rate,
                housing_fund_rate,
                0.0,
                0.0,
            ),
        };

    let social_security_personal = social_security_base * ss_personal_rate;
    let housing_fund_personal = housing_fund_base * hf_personal_rate;
    let social_security_employer = social_security_base * ss_employer_rate;
    let housing_fund_employer = housing_fund_base * hf_employer_rate;
```

（`social_security_rate`/`housing_fund_rate` 原变量在 168-169 行，保留定义供回退分支使用；`params!` 需要 `use rusqlite::params`——确认 salary.rs 头部已引入。）

`SalaryResult` 构造（约 225 行区域）加：

```rust
        social_security_employer: (social_security_employer * 100.0).round() / 100.0,
        housing_fund_employer: (housing_fund_employer * 100.0).round() / 100.0,
```

- [ ] **Step 6: 跑测试与全量后端**

Run: `cd src-tauri && cargo fmt && cargo check && cargo test --lib`
Expected: 全过（注意：个税 taxable 计算不变——个人部分口径与现状一致，仅当台账费率与全局不同时数值变化，既有测试若断言社保数值需检查是否受影响——现有测试无台账走回退分支，数值不变）

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/salary.rs src-tauri/src/db.rs src-tauri/src/models.rs
git commit -m "feat(insurance): 工资计算挂接社保台账（clamp/回退/单位部分落库）"
```

---

### Task 8: 工资计提凭证升级 + 代扣腿

**Files:**
- Modify: `src-tauri/src/accounting.rs`（`generate_salary_accrual_vouchers`，393-467 行）
- Test: `src-tauri/src/accounting.rs` `mod tests`（改造现有 `test_salary_accrual_voucher` + 新增代扣断言）

**Interfaces:**
- Consumes: Task 7 落库的 `social_security_employer / housing_fund_employer` 与既有 `social_security_personal / housing_fund_personal / tax_amount`
- Produces: 计提凭证分录：借 dept(应发净额+单位部分)、贷 2211 同额、借 2211(个人部分+个税)、贷 2241(个人社保公积金)、贷 2221(个税)

- [ ] **Step 1: 改造测试**

现有 `test_salary_accrual_voucher`（accounting.rs:1821 附近）中工资结果 INSERT 补新列值（`social_security_employer`、`housing_fund_employer`）与个人部分/税额，断言扩展为：

```rust
    // 凭证分录：
    // 借 6602 = gross - attendance - other + employer_ss + employer_hf
    // 贷 2211 同额
    // 借 2211 = personal_ss + personal_hf + tax
    // 贷 2241 = personal_ss + personal_hf
    // 贷 2221 = tax
```

新增测试 `test_salary_accrual_zero_withholding`：个人部分与税全 0 时，只有两行分录（借 dept/贷 2211），无 2241/2221 行。

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib salary_accrual`
Expected: FAIL（分录缺少新行）

- [ ] **Step 3: 实现改造**

`generate_salary_accrual_vouchers` SELECT（393-400 行）加列：

```rust
    let mut stmt = conn.prepare(
        "SELECT id, name, department, gross_salary, attendance_deduction, other_deduction,
                social_security_personal, housing_fund_personal, tax_amount,
                social_security_employer, housing_fund_employer
         FROM salary_monthly_results
         WHERE salary_month = ?1 AND locked = 1 AND status != 'void'",
    )?;
```

query_map 元组扩展为 11 元组。循环体 amount 计算与 lines 改为：

```rust
        let employer = social_security_employer + housing_fund_employer;
        let amount = (gross - attendance - other).max(0.0);
        if amount <= 0.0 && employer <= 0.0 {
            continue;
        }
        let cost_amount = amount + employer;
        let withholding_ss = social_security_personal + housing_fund_personal;
        let mut lines = vec![
            VoucherLineDraft {
                account_code: dept_account.clone(),
                debit_amount: cost_amount,
                credit_amount: 0.0,
                summary: Some(format!("{month} 工资费用（{emp}）")),
            },
            VoucherLineDraft {
                account_code: "2211".into(),
                debit_amount: 0.0,
                credit_amount: cost_amount,
                summary: Some(format!("{month} 应付职工薪酬（{emp}）")),
            },
        ];
        if withholding_ss + tax_amount > 0.005 {
            lines.push(VoucherLineDraft {
                account_code: "2211".into(),
                debit_amount: withholding_ss + tax_amount,
                credit_amount: 0.0,
                summary: Some(format!("{month} 代扣款项（{emp}）")),
            });
            if withholding_ss > 0.005 {
                lines.push(VoucherLineDraft {
                    account_code: "2241".into(),
                    debit_amount: 0.0,
                    credit_amount: withholding_ss,
                    summary: Some(format!("{month} 代扣社保公积金（{emp}）")),
                });
            }
            if tax_amount > 0.005 {
                lines.push(VoucherLineDraft {
                    account_code: "2221".into(),
                    debit_amount: 0.0,
                    credit_amount: tax_amount,
                    summary: Some(format!("{month} 代扣个税（{emp}）")),
                });
            }
        }
```

`insert_voucher(conn, &VoucherDraft { ..., lines })`（替换原两行 lines）。

- [ ] **Step 4: 跑测试 + 全量**

Run: `cd src-tauri && cargo fmt && cargo check && cargo test --lib`
Expected: 全过

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/accounting.rs
git commit -m "feat(accounting): 工资计提凭证全额成本口径与代扣腿"
```

---

### Task 9: 个税累计预扣法

**Files:**
- Modify: `src-tauri/src/db.rs`（tax_rules 加 scope 列 + seed 累计 7 档 + `get_cumulative_tax_rules`）
- Modify: `src-tauri/src/salary.rs`（`calculate_cumulative_tax` 替换 `calculate_tax` 调用）
- Test: 两文件 `mod tests`

**Interfaces:**
- Consumes: `salary_monthly_results` 已存记录（gross/社保公积金个人/税额）
- Produces: `pub fn calculate_cumulative_tax(conn: &Connection, employee_no: &str, month: &str, gross: f64, ss_personal: f64, hf_personal: f64, special_deduction: f64, threshold: f64) -> AppResult<f64>`；`tax_rules.scope` 列（monthly/cumulative）

- [ ] **Step 1: DDL + seed**

迁移区加：`ensure_column(conn, "tax_rules", "scope", "TEXT NOT NULL DEFAULT 'monthly'")?;`

`insert_default_data`（tax seed 块后）加：

```rust
    let cumulative_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tax_rules WHERE scope = 'cumulative'",
        [],
        |row| row.get(0),
    )?;
    if cumulative_count == 0 {
        let cumulative_tax = vec![
            (0.0, 36000.0, 0.03, 0.0),
            (36000.0, 144000.0, 0.10, 2520.0),
            (144000.0, 300000.0, 0.20, 16920.0),
            (300000.0, 420000.0, 0.25, 31920.0),
            (420000.0, 660000.0, 0.30, 52920.0),
            (660000.0, 960000.0, 0.35, 85920.0),
            (960000.0, 999999999.0, 0.45, 181920.0),
        ];
        for (min, max, rate, deduction) in &cumulative_tax {
            conn.execute(
                "INSERT INTO tax_rules (min_amount, max_amount, tax_rate, quick_deduction, scope) VALUES (?1, ?2, ?3, ?4, 'cumulative')",
                params![min, max, rate, deduction],
            )?;
        }
    }
```

新函数（`get_tax_rules` 旁）：

```rust
pub fn get_cumulative_tax_rules(conn: &Connection) -> AppResult<Vec<TaxRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, min_amount, max_amount, tax_rate, quick_deduction FROM tax_rules
         WHERE scope = 'cumulative' ORDER BY min_amount",
    )?;
    let rules = stmt
        .query_map([], |row| {
            Ok(TaxRule {
                id: row.get(0)?,
                min_amount: row.get(1)?,
                max_amount: row.get(2)?,
                tax_rate: row.get(3)?,
                quick_deduction: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rules)
}
```

（`TaxRule` 模型不加 scope 字段——前端不展示该表，YAGNI。）

- [ ] **Step 2: 写失败测试**（salary.rs tests）

```rust
#[test]
fn test_cumulative_tax_january_equals_monthly() {
    let conn = Connection::open_in_memory().unwrap();
    db::init_db(&conn).unwrap();
    // 1 月无历史：累计=当月，累计预扣与旧月度算法在首月一致（10 万应税 → 100000*0.10-2520=7480）
    let tax = calculate_cumulative_tax(&conn, "E001", "2026-01", 100000.0, 0.0, 0.0, 0.0, 5000.0).unwrap();
    assert_eq!(tax, 7480.0);
}

#[test]
fn test_cumulative_tax_progresses_over_months() {
    let conn = Connection::open_in_memory().unwrap();
    db::init_db(&conn).unwrap();
    // 1-6 月已存：每月应税 10000、已预扣每月 10000*0.03=300 → 累计已缴 1800
    for m in 1..=6 {
        conn.execute(
            "INSERT INTO salary_monthly_results (salary_month, employee_no, gross_salary, social_security_personal, housing_fund_personal, tax_amount, status, locked)
             VALUES (?1, 'E002', 15000.0, 0.0, 0.0, 300.0, 'approved', 1)",
            [format!("2026-{m:02}")],
        ).unwrap();
    }
    // 7 月同收入：累计应税 70000 → 70000*0.10-2520=4480；已缴 1800 → 当月 2680
    let tax = calculate_cumulative_tax(&conn, "E002", "2026-07", 15000.0, 0.0, 0.0, 0.0, 5000.0).unwrap();
    assert_eq!(tax, 2680.0);
}
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib cumulative_tax`
Expected: FAIL（函数未定义）

- [ ] **Step 4: 实现**（salary.rs，`calculate_single_employee` 前）

```rust
/// 累计预扣法：累计应纳税所得额×预扣率-速算扣除-累计已预扣（max 0）。
/// 历史月份（含旧月度算法结果）自然作为"已预扣"基数，启用当月平滑。
pub fn calculate_cumulative_tax(
    conn: &Connection,
    employee_no: &str,
    month: &str,
    gross: f64,
    ss_personal: f64,
    hf_personal: f64,
    special_deduction: f64,
    threshold: f64,
) -> AppResult<f64> {
    let year_prefix = format!("{}-%", &month[..4]);
    let (prev_gross, prev_ss, prev_tax, prev_count): (f64, f64, f64, i64) = conn
        .query_row(
            "SELECT COALESCE(SUM(gross_salary),0), COALESCE(SUM(social_security_personal + housing_fund_personal),0),
                    COALESCE(SUM(tax_amount),0), COUNT(*)
             FROM salary_monthly_results
             WHERE employee_no = ?1 AND salary_month LIKE ?2 AND salary_month < ?3 AND status != 'void'",
            params![employee_no, year_prefix, month],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap_or((0.0, 0.0, 0.0, 0));
    let months = (prev_count + 1) as f64;
    let cumulative_taxable = (prev_gross + gross) - (prev_ss + ss_personal + hf_personal)
        - threshold * months
        - special_deduction * months;
    if cumulative_taxable <= 0.0 {
        return Ok(0.0);
    }
    let rules = db::get_cumulative_tax_rules(conn)?;
    let mut annual_tax = 0.0;
    for rule in &rules {
        let max = rule.max_amount.unwrap_or(f64::MAX);
        if cumulative_taxable > rule.min_amount && cumulative_taxable <= max {
            annual_tax = (cumulative_taxable * rule.tax_rate - rule.quick_deduction).max(0.0);
            break;
        }
    }
    Ok((annual_tax - prev_tax).max(0.0))
}
```

（注意：`special_deduction` 传 `emp.special_deduction`，历史月的专项附加未落库、按当月值×月数近似——与员工专项附加年度内不变的实际场景一致，在函数 doc 注释说明。）

`calculate_single_employee` 中替换：

```rust
    let tax_amount = calculate_cumulative_tax(
        conn,
        &emp.employee_no,
        month,
        gross_salary,
        social_security_personal,
        housing_fund_personal,
        special_deduction,
        tax_threshold,
    )?;
```

（`calculate_tax` 月度函数保留不删——其他调用方如有。）

- [ ] **Step 5: 跑测试 + 全量后端**

Run: `cd src-tauri && cargo fmt && cargo check && cargo test --lib`
Expected: 全过。若既有工资测试因累计口径改变断言失败，修正断言（首月一致；跨月测试按公式重算期望值）。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/db.rs src-tauri/src/salary.rs
git commit -m "feat(tax): 个税改累计预扣法（历史平滑、无迁移）"
```

---

### Task 10: 个税年度汇总 + Excel + 前端弹窗

**Files:**
- Modify: `src-tauri/src/db.rs`（`get_annual_tax_summary`）
- Modify: `src-tauri/src/models.rs`（`AnnualTaxSummaryRow`）
- Modify: `src-tauri/src/commands.rs`（`get_annual_tax_summary`、`export_annual_tax_summary`）
- Modify: `src-tauri/src/lib.rs`（注册）
- Modify: `src-tauri/src/excel.rs`（`export_annual_tax_summary_excel`）
- Modify: `src/types/index.ts`、`src/api/index.ts`、`src/pages/SalaryCalculate.tsx`、`src/pages/OperationLogs.tsx`

**Interfaces:**
- Consumes: Task 9 的 `get_cumulative_tax_rules`
- Produces: `AnnualTaxSummaryRow { employee_no, name, month_count: i32, total_gross, total_ss_personal, total_hf_personal, total_special_deduction, total_tax_withheld, annual_tax_due, difference }`（difference = annual_tax_due - total_tax_withheld，负数=多缴）；命令 `get_annual_tax_summary(year)`、`export_annual_tax_summary(year, path)`

- [ ] **Step 1: 模型 + db 函数**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnualTaxSummaryRow {
    pub employee_no: String,
    pub name: Option<String>,
    pub month_count: i32,
    pub total_gross: f64,
    pub total_ss_personal: f64,
    pub total_hf_personal: f64,
    pub total_special_deduction: f64,
    pub total_tax_withheld: f64,
    pub annual_tax_due: f64,
    pub difference: f64,
}
```

db.rs：

```rust
pub fn get_annual_tax_summary(conn: &Connection, year: i64) -> AppResult<Vec<AnnualTaxSummaryRow>> {
    let prefix = format!("{year}-%");
    let mut stmt = conn.prepare(
        "SELECT r.employee_no, MAX(r.name), COUNT(*),
                COALESCE(SUM(r.gross_salary),0),
                COALESCE(SUM(r.social_security_personal),0),
                COALESCE(SUM(r.housing_fund_personal),0),
                COALESCE(SUM(r.tax_amount),0),
                COALESCE(MAX(COALESCE(e.special_deduction,0)),0)
         FROM salary_monthly_results r LEFT JOIN employees e ON e.employee_no = r.employee_no
         WHERE r.salary_month LIKE ?1 AND r.status != 'void'
         GROUP BY r.employee_no ORDER BY r.employee_no",
    )?;
    let rows = stmt
        .query_map(params![prefix], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, f64>(5)?,
                r.get::<_, f64>(6)?,
                r.get::<_, f64>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    let rules = get_cumulative_tax_rules(conn)?;
    let rate_of = |taxable: f64| -> f64 {
        for rule in &rules {
            let max = rule.max_amount.unwrap_or(f64::MAX);
            if taxable > rule.min_amount && taxable <= max {
                return (taxable * rule.tax_rate - rule.quick_deduction).max(0.0);
            }
        }
        0.0
    };
    let mut result = Vec::new();
    for (no, name, count, gross, ss, hf, withheld, special) in rows {
        let months = count as f64;
        let taxable = gross - ss - hf - 5000.0 * months - special * months;
        let due = if taxable > 0.0 { rate_of(taxable) } else { 0.0 };
        result.push(AnnualTaxSummaryRow {
            employee_no: no,
            name,
            month_count: count as i32,
            total_gross: (gross * 100.0).round() / 100.0,
            total_ss_personal: (ss * 100.0).round() / 100.0,
            total_hf_personal: (hf * 100.0).round() / 100.0,
            total_special_deduction: (special * months * 100.0).round() / 100.0,
            total_tax_withheld: (withheld * 100.0).round() / 100.0,
            annual_tax_due: (due * 100.0).round() / 100.0,
            difference: ((due - withheld) * 100.0).round() / 100.0,
        });
    }
    Ok(result)
}
```

- [ ] **Step 2: 测试**（db.rs tests：插 3 个月记录断言聚合与差额；沿用 Task 9 测试数据模式）

- [ ] **Step 3: 命令 + Excel + 前端**

命令（参照 Task 2 `export_trial_balance` 模式，写路径由前端对话框传入）：

```rust
#[tauri::command]
pub fn get_annual_tax_summary(
    year: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<AnnualTaxSummaryRow>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_annual_tax_summary(&conn, year)
}
```

`export_annual_tax_summary(year, path)`：调 db → `excel::export_annual_tax_summary_excel(&rows, year, &path)` → log_operation（映射 `export_annual_tax_summary: '导出个税年度汇总'`）。

excel：表头行 `["工号","姓名","月数","累计收入","累计社保个人","累计公积金个人","累计专项附加","累计已预扣","年度应预扣","差额"]`，参照 Task 2 的写法逐行 write_number_with_format。

前端 SalaryCalculate.tsx：工具栏加按钮"个税年度汇总"→ Modal（宽 1000）内 year DatePicker.YearPicker + Table（10 列，金额走 `SensitiveText type="amount"` 或明文——工资页现有金额列的脱敏方式照抄）+ "导出 Excel"按钮（`save` 对话框选路径）。api 函数 `getAnnualTaxSummary(year)`、`exportAnnualTaxSummary(year, path)` + mock case（`get_annual_tax_summary` → `[]`、`export_annual_tax_summary` → `''`）。lib.rs 注册 2 命令。

- [ ] **Step 4: 验证 + Commit**

Run: `cd src-tauri && cargo check && cargo test --lib && npx tsc --noEmit && npm run lint && npm run build`

```bash
git add src-tauri/src/ src/types/index.ts src/api/index.ts src/pages/SalaryCalculate.tsx src/pages/OperationLogs.tsx
git commit -m "feat(tax): 个税年度汇总表与 Excel 导出"
```

---

### Task 11: 工资条预览打印

**Files:**
- Modify: `src/pages/SalaryCalculate.tsx`（工资条按钮 + Modal 预览）
- Modify: `src/index.css`（@media print 规则）
- Modify: `src/App.tsx`（仅当布局容器需要打印类名时——检查 Modal 是否在 Layout 内渲染，antd Modal 默认挂 body，不需要改）

**Interfaces:**
- Consumes: `useSecurity()` 的 `isSensitiveRevealed`（contexts/SecurityContext）；当前月份已加载的工资数据（SalaryCalculate 页 state）
- Produces: 无新命令（纯前端）

- [ ] **Step 1: index.css 追加打印样式**

```css
/* 工资条打印：仅显示工资条区域，每张卡片分页 */
@media print {
  body * {
    visibility: hidden;
  }
  .payslip-print-area,
  .payslip-print-area * {
    visibility: visible;
  }
  .payslip-print-area {
    position: absolute;
    left: 0;
    top: 0;
    width: 100%;
  }
  .payslip-card {
    page-break-after: always;
    border: 1px solid #333;
    margin-bottom: 12px;
  }
}
.payslip-card {
  border: 1px solid #d9d9d9;
  border-radius: 4px;
  padding: 12px 16px;
  margin-bottom: 12px;
}
```

- [ ] **Step 2: SalaryCalculate.tsx 加工资条入口与 Modal**

工具栏加按钮（与"计算工资"同区）：

```tsx
<Button icon={<PrinterOutlined />} onClick={() => setPayslipOpen(true)}>工资条</Button>
```

Modal（页面 state：`payslipOpen`）：

```tsx
const { isSensitiveRevealed } = useSecurity();

<Modal
  open={payslipOpen}
  onCancel={() => setPayslipOpen(false)}
  title={`${currentMonth} 工资条预览`}
  width={720}
  footer={[
    <Button key="print" type="primary" disabled={!isSensitiveRevealed} onClick={() => window.print()}>
      打印 / 另存 PDF
    </Button>,
  ]}
>
  {!isSensitiveRevealed && (
    <Alert type="warning" showIcon message="工资条含明文金额，请先解锁敏感数据（点击任意金额的眼睛图标解锁）" className="mb-16" />
  )}
  <div className="payslip-print-area">
    {results.filter((r) => r.status !== 'void').map((r) => (
      <div key={r.id} className="payslip-card">
        <div style={{ display: 'flex', justifyContent: 'space-between', fontWeight: 600, marginBottom: 8 }}>
          <span>{r.name}（{r.employee_no}）</span>
          <span>{currentMonth}</span>
        </div>
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
          <tbody>
            {[
              ['基本工资', r.base_salary], ['岗位工资', r.position_salary], ['绩效工资', r.performance_salary],
              ['加班费', r.overtime_salary], ['餐补', r.meal_allowance], ['交通补贴', r.transport_allowance],
              ['应发合计', r.gross_salary], ['社保(个人)', -r.social_security_personal],
              ['公积金(个人)', -r.housing_fund_personal], ['考勤扣款', -r.attendance_deduction],
              ['个税', -r.tax_amount], ['其他扣款', -r.other_deduction],
            ].map(([label, value]) => (
              <tr key={label as string}>
                <td style={{ border: '1px solid #d9d9d9', padding: '2px 8px', width: '50%' }}>{label}</td>
                <td style={{ border: '1px solid #d9d9d9', padding: '2px 8px', textAlign: 'right' }}>
                  {fmtMoney(value as number)}
                </td>
              </tr>
            ))}
            <tr>
              <td style={{ border: '1px solid #333', padding: '4px 8px', fontWeight: 700 }}>实发工资</td>
              <td style={{ border: '1px solid #333', padding: '4px 8px', textAlign: 'right', fontWeight: 700 }}>
                {fmtMoney(r.net_salary)}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    ))}
  </div>
</Modal>
```

（`results`、`currentMonth`、`fmtMoney` 用页面现有 state/helper 名——执行时对齐实际命名；金额明文 `fmtMoney`，不走 SensitiveText，Modal 内已有解锁提示与打印按钮禁用逻辑。）

- [ ] **Step 3: 验证 + Commit**

Run: `npx tsc --noEmit && npm run lint && npm run build`

```bash
git add src/pages/SalaryCalculate.tsx src/index.css
git commit -m "feat(salary): 工资条预览与打印（敏感解锁门槛）"
```

---

### Task 12: 三大报表上年同期对比列

**Files:**
- Modify: `src-tauri/src/models.rs`（`ReportRow` 加 `prior_year: f64`；`BalanceSheet`/`IncomeStatement`/`CashFlowStatement` 加 `has_prior_year: bool`）
- Modify: `src-tauri/src/accounting.rs`（三个 build 函数计算同期值；所有 `ReportRow` 构造点补 `prior_year` 字段——`cargo check` 报错清单即修改清单）
- Modify: `src-tauri/src/excel.rs`（三报表导出加列）
- Modify: `src/pages/FinancialReports.tsx`（三 Tab 表格加列）
- Test: `src-tauri/src/accounting.rs` `mod tests`

**Interfaces:**
- Consumes: `compute_balances`、`profit_loss_amounts`、现金流量分摊逻辑
- Produces: `ReportRow.prior_year`（前端列 `prior_year`）；`has_prior_year`（false 时前端显示 `-`）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn test_reports_prior_year_columns() {
    let conn = test_conn();
    // 2024-12 凭证：收入 6001 贷 1200
    conn.execute("INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount) VALUES ('2024-01', '1001', 0.0, 0.0)", []).unwrap();
    conn.execute("INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status) VALUES ('记-202412-001', '2024-12-05', '2024-12', 'bank_manual', 1, 1200.0, 'active')", []).unwrap();
    let v24: i64 = conn.query_row("SELECT id FROM vouchers WHERE voucher_no='记-202412-001'", [], |r| r.get(0)).unwrap();
    conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '6001', 0.0, 1200.0)", [v24]).unwrap();
    conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '1001', 1200.0, 0.0)", [v24]).unwrap();
    // 2025-12 凭证：收入 6001 贷 800
    conn.execute("INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status) VALUES ('记-202512-001', '2025-12-05', '2025-12', 'bank_manual', 2, 800.0, 'active')", []).unwrap();
    let v25: i64 = conn.query_row("SELECT id FROM vouchers WHERE voucher_no='记-202512-001'", [], |r| r.get(0)).unwrap();
    conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '6001', 0.0, 800.0)", [v25]).unwrap();
    conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '1001', 800.0, 0.0)", [v25]).unwrap();

    let income = build_income_statement(&conn, "2025-12").unwrap();
    assert!(income.has_prior_year);
    let rev = income.rows.iter().find(|r| r.key == "6001").unwrap();
    assert_eq!(rev.prior_year, 1200.0); // 上年同期累计

    let bs = build_balance_sheet(&conn, "2025-12").unwrap();
    assert!(bs.has_prior_year);
    let cash_row = bs.asset_rows.iter().find(|r| r.key == "monetary").unwrap();
    assert_eq!(cash_row.prior_year, 1200.0); // 上年年末时点

    // 无上年数据：2026-12
    let income26 = build_income_statement(&conn, "2026-12").unwrap();
    assert!(income26.has_prior_year);
    let rev26 = income26.rows.iter().find(|r| r.key == "6001").unwrap();
    assert_eq!(rev26.prior_year, 800.0); // 2025 年 800 滚为上年同期
}
```

（启用月 2024-01 时 2025-12 的上年=2024；`has_prior_year` 判定：`上年1月 >= 启用月`，否则 false 全列 0。最后一断言依赖 2025 数据存在——若认为应视为"有"则保留，执行时按此口径。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib prior_year`
Expected: FAIL（字段不存在）

- [ ] **Step 3: 实现引擎**

models.rs：`ReportRow` 加 `pub prior_year: f64`；三个报表结构体加 `pub has_prior_year: bool`。

`build_balance_sheet`（1187 起）：函数开头计算：

```rust
    let prior_dec = format!("{}-12", &month[..4].parse::<i64>().map(|y| y - 1).map(|y| y.to_string()).unwrap_or_else(|_| month[..4].to_string()));
    let prior_enabled = month_enabled(conn, &prior_dec);
    let prior_balances = if prior_enabled {
        compute_balances(conn, &prior_dec)?
    } else {
        Vec::new()
    };
    let prior_ending = |code: &str| -> f64 {
        prior_balances
            .iter()
            .find(|b| b.code == code)
            .map(|b| b.ending())
            .unwrap_or(0.0)
    };
```

资产行构造时 `prior_year`：货币资金行累加 `prior_ending(code)`（CASH_ACCOUNTS 内），普通行 `prior_ending(b.code)`；3104 行 prior = `prior_ending("3104") + 上年净利润`——上年净利润 = `net_profit(conn, &prior_dec)?.1`；`has_prior_year = prior_enabled`。

`build_income_statement`：`let prior_amounts = profit_loss_amounts(conn, &prior_month)?;`（prior_month = 上年同月）。行构造 `prior_year: prior_amounts.as_ref().map(|m| m.get(*code).map(|(m, y)| *y).unwrap_or(0.0)).unwrap_or(0.0)`（取 y 累计分量；`other_pl` 同理）；`has_prior_year = prior_amounts.is_some()`。营业利润/利润总额/净利润的 prior 分量用 get 闭包按同样公式对 prior 值重算（照 current 的计算式复制一遍换成 prior 值）。

`build_cash_flow_statement`：把 1363-1446 的分摊循环抽为内部函数：

```rust
fn sum_cash_flow(vouchers: &[Voucher], cfc_map: &HashMap<String, String>) -> (CashFlowSums, Vec<UnclassifiedCashItem>) { /* 原 1375-1446 逻辑 */ }
```

当月：现有 get_vouchers 单月结果。同期：区间凭证查询（排除 period_close）：

```rust
fn prior_year_vouchers(conn: &Connection, month: &str) -> AppResult<Vec<Voucher>> {
    let year: i64 = month[..4].parse().map_err(|_| AppError::General("月份格式错误".into()))?;
    let from = format!("{}-01", year - 1);
    let to = format!("{}-12", year - 1);
    let vs = get_vouchers_range(conn, &from, &to)?; // 实现：SQL between belong_month 且 status='active' 且 source_type != 'period_close'，行映射照 get_vouchers
    Ok(vs)
}
```

（`get_vouchers_range` 新增——参照 `get_vouchers` 的 SQL 与行映射实现；`has_prior_year = !prior_vouchers.is_empty() || 上年 >= 启用月`，取 `month_enabled(conn, 上年同月)`。）rows 构造补 `prior_year: prior_sums.operating_in` 等；`net_increase` 的 prior 分量若有展示需求同式计算（前端只展示行级 prior，net_increase 不加——保持现状）。

所有其它 `ReportRow { ... }` 构造点（`cargo check` 报错清单）补 `prior_year: 0.0`。

- [ ] **Step 4: 跑测试**

Run: `cd src-tauri && cargo test --lib prior_year && cargo test --lib`
Expected: 全过

- [ ] **Step 5: Excel 加列 + 前端加列**

excel.rs 三报表导出：每张表数据列后加"上年同期"列（值 `row.prior_year`；balance_sheet 标"上年年末"、income/cash 标"上年同期"）——在现有列循环后加一列 write_number_with_format。

FinancialReports.tsx：三个 Tab 的 columns 数组各加 `{ title: '上年同期', dataIndex: 'prior_year', render: (v: number) => (report?.has_prior_year ? fmtMoney(v) : '-') }`（balance Tab 标题"上年年末"；report 变量为对应 Tab 已加载的报表对象；合计行若手工渲染则同步加）。

- [ ] **Step 6: 验证 + Commit**

Run: `cd src-tauri && cargo fmt --check && cargo check && cargo test --lib && npx tsc --noEmit && npm run lint && npm run build`

```bash
git add src-tauri/src/ src/pages/FinancialReports.tsx
git commit -m "feat(accounting): 三大报表上年同期对比列与导出"
```

---

### Task 13: 全量回归 + 文档回写

**Files:**
- Modify: `CLAUDE.md`（第六阶段段落 + Memory References）
- Create: `.claude/memory/stage6-finance-extensions.md`
- Create: `docs/superpowers/plans/2026-08-22-stage6-progress.md`
- Modify: `docs/user-manual*`（使用手册补第六章功能说明——文件名以 docs/ 下实际手册为准）

- [ ] **Step 1: 全量回归**

```bash
npx tsc --noEmit
npm run lint
npm run build
cd src-tauri && cargo fmt --check && cargo check && cargo test --lib
```

Expected: 全部通过（cargo 既有 7 个 warning 可保留）

- [ ] **Step 2: 文档**

CLAUDE.md 加"第六阶段开发"段（仿第五阶段段落：定位、批次、先读文件）；Memory References 加 stage6 链接。

`.claude/memory/stage6-finance-extensions.md`：仿 stage5-accounting.md（frontmatter name/description、定位、已交付能力、批次、测试门槛、已知边界——含"启用月之前存量数据报表为 0 属预期""个税历史月按月度算法视为已预扣"）。

progress 文件：基线 + 协作规则 + 记录模板（照 stage5-progress.md 结构）。

使用手册：补科目余额表、年末结转、社保台账与调基、个税年度汇总、工资条打印、上年同期各节（每节 3-6 行操作说明）。

- [ ] **Step 3: graphify 更新 + Commit**

```bash
graphify update . 2>/dev/null || true
git add CLAUDE.md .claude/memory/stage6-finance-extensions.md .claude/memory/MEMORY.md docs/
git commit -m "docs: 第六阶段文档四件套与 memory 摘要"
```

---

## 批次与交付顺序

| 批次 | Task | 可交付物 |
|------|------|------|
| 一 | 1-4 | 科目余额表 + 年末结转闭环 |
| 2a | 5-8 | 社保台账 + 凭证联动 |
| 2b | 9-10 | 个税累计预扣 + 年度汇总 |
| 2c | 11 | 工资条打印 |
| 三 | 12 | 报表同期列 |
| 收尾 | 13 | 回归 + 文档 |

每个 Task 独立可测试可 commit；Task 7 依赖 Task 5、Task 8 依赖 Task 7、Task 10 依赖 Task 9、Task 12 依赖 Task 3（排除口径），其余可并行。

## 执行注意

- Task 1/3/5/9 的测试 helper：先读 `accounting.rs` / `db.rs` 现有 `mod tests` 的建库方式（函数名可能不是 `test_conn`/`init_db`），照现有模式写，本文中 `test_conn()`/`init_db()` 为占位指代。
- Task 7 测试须经 salary.rs 公开入口触发计算（先读文件确认入口函数名再写测试）。
- Task 11 的 `results`/`currentMonth`/`fmtMoney` 以 SalaryCalculate.tsx 实际 state 命名为准。
- 涉及既有测试断言因算法升级（个税累计）失败时：按新公式重算期望值，不改生产代码迁就旧断言。
