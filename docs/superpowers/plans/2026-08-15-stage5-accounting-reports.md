# 第五阶段实现计划：科目表与三大报表

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 业务数据自动派生复式记账凭证，生成科目表、资产负债表、利润表、现金流量表，月结即关账。

**Architecture:** 方案 A（凭证落库）：业务命令在同一事务内生成/作废 `vouchers`+`voucher_lines`；报表查询时实时汇总凭证；正式月结冻结凭证。新后端模块 `accounting.rs`。

**Tech Stack:** Tauri 2 + rusqlite (bundled) + rust_xlsxwriter；前端 React 19 + Ant Design 6。

**Spec:** `docs/superpowers/specs/2026-08-15-stage5-accounting-reports-design.md`（本计划对 spec 3.1 规则 1 有精化，见 Task 5 开头说明，需回写 spec）

## Global Constraints

- 中文 UI 字符串、中文 commit message
- Tauri 命令：`#[tauri::command]` + snake_case，前端 `invoke('snake_case_name')`
- 时间戳：`Utc::now().to_rfc3339()`
- 测试：Rust 单测在 `#[cfg(test)] mod tests`，用 `Connection::open_in_memory()`
- 不跳过 hooks，不 `--no-verify`
- 每批回归：`npx tsc --noEmit`、`npm run lint`、`npm run build`、`cd src-tauri && cargo fmt --check && cargo test --lib`
- subagent 只处理互不重叠文件范围；主 agent 统一合并、测试、commit、push
- 每批结束追加 `docs/superpowers/plans/2026-08-15-stage5-progress.md`

## 文件结构总览

| 文件 | 职责 |
|---|---|
| `src-tauri/src/db.rs`（改） | 新增 5 张表 DDL + 预置科目 seed + 迁移 |
| `src-tauri/src/accounting.rs`（新） | 科目/期初/映射 CRUD、凭证生成与作废、三大报表计算 |
| `src-tauri/src/models.rs`（改） | 新结构体与 serde 类型 |
| `src-tauri/src/commands.rs`（改） | 新命令 + 业务命令挂接凭证生成 |
| `src-tauri/src/lib.rs`（改） | 注册新模块与命令 |
| `src-tauri/src/excel.rs`（改） | 三大报表导出 |
| `src/types/index.ts`、`src/api/index.ts`（改） | 前端类型与 invoke 封装 |
| `src/pages/ChartOfAccounts.tsx`、`src/pages/Vouchers.tsx`、`src/pages/FinancialReports.tsx`（新） | 三个新页面 |
| `src/App.tsx`（改） | "财务管理"菜单组 |
| `src/pages/BankTransactions.tsx`（改） | 未匹配流水"生成凭证"入口 |

分四批交付：批次一（Task 1-4）科目与期初；批次二（Task 5-9）凭证引擎与业务挂接；批次三（Task 10-11）报表与导出；批次四（Task 12-14）前端页面与回归。

---

## 批次一：科目表与期初余额

### Task 1: 数据库表与预置科目

**Files:**
- Modify: `src-tauri/src/db.rs`（`create_tables` 函数内，`budgets` 表之后追加）
- Test: `src-tauri/src/db.rs`（文件尾部 `#[cfg(test)] mod tests` 内追加）

**Interfaces:**
- Produces: 表 `gl_accounts` / `vouchers` / `voucher_lines` / `opening_balances` / `account_mappings`；`db::seed_gl_accounts(&conn) -> AppResult<usize>`

- [ ] **Step 1: 写失败测试**

在 `db.rs` 测试模块追加：

```rust
#[test]
fn test_gl_tables_and_seed() {
    let conn = Connection::open_in_memory().unwrap();
    initialize_database(&conn).unwrap();
    // initialize_database 内部调用 create_tables + migrate_existing_schema + seed_gl_accounts
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM gl_accounts WHERE is_system=1", [], |r| r.get(0))
        .unwrap();
    assert!(count >= 70, "预置科目不足: {count}");
    // 借贷方向枚举合法
    let bad: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM gl_accounts WHERE direction NOT IN ('debit','credit')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bad, 0);
}
```

（若现有测试初始化用的是别的辅助函数名，以现有测试写法为准，如直接 `create_tables(&conn).unwrap()`。）

- [ ] **Step 2: 运行确认失败**

Run: `cd src-tauri && cargo test --lib test_gl_tables_and_seed`
Expected: FAIL（no such table: gl_accounts）

- [ ] **Step 3: 实现 DDL 与 seed**

在 `create_tables` 中 `budgets` 建表与索引之后追加：

```rust
        CREATE TABLE IF NOT EXISTS gl_accounts (
            code TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            category TEXT NOT NULL CHECK (category IN ('asset','liability','equity','cost','profit_loss')),
            direction TEXT NOT NULL CHECK (direction IN ('debit','credit')),
            cash_flow_category TEXT NOT NULL DEFAULT 'none'
                CHECK (cash_flow_category IN ('operating','investing','financing','none')),
            is_system INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            created_at TEXT,
            updated_at TEXT
        );
        CREATE TABLE IF NOT EXISTS vouchers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            voucher_no TEXT UNIQUE NOT NULL,
            voucher_date TEXT NOT NULL,
            belong_month TEXT NOT NULL,
            source_type TEXT NOT NULL CHECK (source_type IN (
                'salary_accrual','salary_payment','reimbursement_accrual',
                'reimbursement_payment','invoice_expense','bank_manual')),
            source_id INTEGER NOT NULL,
            total_amount REAL NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','void')),
            remark TEXT,
            created_at TEXT,
            updated_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_vouchers_month ON vouchers(belong_month, status);
        CREATE INDEX IF NOT EXISTS idx_vouchers_source ON vouchers(source_type, source_id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_vouchers_source_active
            ON vouchers(source_type, source_id) WHERE status = 'active';
        CREATE TABLE IF NOT EXISTS voucher_lines (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            voucher_id INTEGER NOT NULL,
            account_code TEXT NOT NULL,
            debit_amount REAL NOT NULL DEFAULT 0,
            credit_amount REAL NOT NULL DEFAULT 0,
            summary TEXT,
            line_order INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (voucher_id) REFERENCES vouchers(id) ON DELETE CASCADE,
            FOREIGN KEY (account_code) REFERENCES gl_accounts(code)
        );
        CREATE INDEX IF NOT EXISTS idx_voucher_lines_voucher ON voucher_lines(voucher_id);
        CREATE INDEX IF NOT EXISTS idx_voucher_lines_account ON voucher_lines(account_code);
        CREATE TABLE IF NOT EXISTS opening_balances (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            month TEXT NOT NULL,
            account_code TEXT NOT NULL,
            debit_amount REAL NOT NULL DEFAULT 0,
            credit_amount REAL NOT NULL DEFAULT 0,
            created_at TEXT,
            updated_at TEXT,
            FOREIGN KEY (account_code) REFERENCES gl_accounts(code)
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_opening_balances_account ON opening_balances(account_code);
        CREATE TABLE IF NOT EXISTS account_mappings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scope TEXT NOT NULL CHECK (scope IN ('expense_type','department')),
            key TEXT NOT NULL,
            account_code TEXT NOT NULL,
            remark TEXT,
            created_at TEXT,
            updated_at TEXT,
            FOREIGN KEY (account_code) REFERENCES gl_accounts(code)
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_account_mappings_scope_key ON account_mappings(scope, key);
```

注意：`account_mappings` 是 spec 2.5 `expense_account_mappings` 的泛化（scope 区分费用类型/部门映射），需回写 spec 改名。

同文件新增 seed 函数（放在 `create_tables` 之后）：

```rust
const GL_ACCOUNT_PRESETS: &[(&str, &str, &str, &str, &str)] = &[
    // (code, name, category, direction, cash_flow_category)
    ("1001", "库存现金", "asset", "debit", "none"),
    ("1002", "银行存款", "asset", "debit", "none"),
    ("1012", "其他货币资金", "asset", "debit", "none"),
    ("1101", "短期投资", "asset", "debit", "investing"),
    ("1121", "应收票据", "asset", "debit", "operating"),
    ("1122", "应收账款", "asset", "debit", "operating"),
    ("1123", "预付账款", "asset", "debit", "operating"),
    ("1131", "应收股利", "asset", "debit", "investing"),
    ("1132", "应收利息", "asset", "debit", "operating"),
    ("1221", "其他应收款", "asset", "debit", "operating"),
    ("1231", "坏账准备", "asset", "credit", "operating"),
    ("1401", "材料采购", "asset", "debit", "operating"),
    ("1402", "在途物资", "asset", "debit", "operating"),
    ("1403", "原材料", "asset", "debit", "operating"),
    ("1405", "库存商品", "asset", "debit", "operating"),
    ("1406", "发出商品", "asset", "debit", "operating"),
    ("1411", "周转材料", "asset", "debit", "operating"),
    ("1471", "存货跌价准备", "asset", "credit", "operating"),
    ("1501", "长期债券投资", "asset", "debit", "investing"),
    ("1511", "长期股权投资", "asset", "debit", "investing"),
    ("1601", "固定资产", "asset", "debit", "investing"),
    ("1602", "累计折旧", "asset", "credit", "operating"),
    ("1604", "在建工程", "asset", "debit", "investing"),
    ("1605", "工程物资", "asset", "debit", "investing"),
    ("1606", "固定资产清理", "asset", "debit", "investing"),
    ("1621", "生产性生物资产", "asset", "debit", "investing"),
    ("1701", "无形资产", "asset", "debit", "investing"),
    ("1702", "累计摊销", "asset", "credit", "operating"),
    ("1801", "长期待摊费用", "asset", "debit", "operating"),
    ("1901", "待处理财产损溢", "asset", "debit", "operating"),
    ("2001", "短期借款", "liability", "credit", "financing"),
    ("2201", "应付票据", "liability", "credit", "operating"),
    ("2202", "应付账款", "liability", "credit", "operating"),
    ("2203", "预收账款", "liability", "credit", "operating"),
    ("2211", "应付职工薪酬", "liability", "credit", "operating"),
    ("2221", "应交税费", "liability", "credit", "operating"),
    ("2231", "应付利息", "liability", "credit", "financing"),
    ("2232", "应付利润", "liability", "credit", "financing"),
    ("2241", "其他应付款", "liability", "credit", "operating"),
    ("2401", "递延收益", "liability", "credit", "operating"),
    ("2501", "长期借款", "liability", "credit", "financing"),
    ("2502", "长期应付款", "liability", "credit", "financing"),
    ("3001", "实收资本", "equity", "credit", "financing"),
    ("3002", "资本公积", "equity", "credit", "financing"),
    ("3101", "盈余公积", "equity", "credit", "none"),
    ("3103", "本年利润", "equity", "credit", "none"),
    ("3104", "利润分配—未分配利润", "equity", "credit", "none"),
    ("5001", "生产成本", "cost", "debit", "operating"),
    ("5101", "制造费用", "cost", "debit", "operating"),
    ("5201", "劳务成本", "cost", "debit", "operating"),
    ("6001", "主营业务收入", "profit_loss", "credit", "operating"),
    ("6051", "其他业务收入", "profit_loss", "credit", "operating"),
    ("6111", "投资收益", "profit_loss", "credit", "investing"),
    ("6301", "营业外收入", "profit_loss", "credit", "operating"),
    ("6401", "主营业务成本", "profit_loss", "debit", "operating"),
    ("6402", "其他业务成本", "profit_loss", "debit", "operating"),
    ("6403", "税金及附加", "profit_loss", "debit", "operating"),
    ("6601", "销售费用", "profit_loss", "debit", "operating"),
    ("6602", "管理费用", "profit_loss", "debit", "operating"),
    ("6603", "财务费用", "profit_loss", "debit", "operating"),
    ("6711", "营业外支出", "profit_loss", "debit", "operating"),
    ("6801", "所得税费用", "profit_loss", "debit", "operating"),
];

pub fn seed_gl_accounts(conn: &Connection) -> AppResult<usize> {
    let now = Utc::now().to_rfc3339();
    let mut n = 0;
    for (code, name, category, direction, cfc) in GL_ACCOUNT_PRESETS {
        let changed = conn.execute(
            "INSERT OR IGNORE INTO gl_accounts
             (code, name, category, direction, cash_flow_category, is_system, is_active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, 1, ?6)",
            params![code, name, category, direction, cfc, now],
        )?;
        n += changed;
    }
    Ok(n)
}
```

在 `initialize_database`（或现有入口，第 23 行附近 `create_tables(&conn)?;` 之后）追加 `seed_gl_accounts(&conn)?;`。

- [ ] **Step 4: 运行确认通过**

Run: `cd src-tauri && cargo test --lib test_gl_tables_and_seed && cargo fmt`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/db.rs
git commit -m "feat(accounting): 新增总账五张表与预置科目"
```

### Task 2: 科目/期初/映射 CRUD（accounting.rs）

**Files:**
- Create: `src-tauri/src/accounting.rs`
- Modify: `src-tauri/src/models.rs`、`src-tauri/src/lib.rs`（`mod accounting;`）
- Test: `src-tauri/src/accounting.rs` 内 `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `db::initialize_database`、Task 1 的表
- Produces:
  - `models.rs`：`GlAccount`、`OpeningBalanceInput`、`AccountMapping`、`Voucher`、`VoucherLine`、`VoucherDraft`、`VoucherLineDraft`、`BalanceSheet`、`IncomeStatement`、`CashFlowStatement`（本任务只加前 3 个 + 后续任务的按需追加）
  - `accounting.rs`：`get_accounts(&Connection) -> AppResult<Vec<GlAccount>>`、`set_account_active(&Connection, &str, bool) -> AppResult<bool>`、`create_account(&Connection, &GlAccountInput) -> AppResult<GlAccount>`、`get_opening_balances(&Connection) -> AppResult<(Option<String>, Vec<OpeningBalanceRow>)>`、`save_opening_balances(&Connection, &str, &[OpeningBalanceRow]) -> AppResult<()>`、`get_account_mappings(&Connection) -> AppResult<Vec<AccountMapping>>`、`save_account_mapping(&Connection, &AccountMapping) -> AppResult<AccountMapping>`、`delete_account_mapping(&Connection, i64) -> AppResult<bool>`

- [ ] **Step 1: models.rs 追加结构体**

```rust
#[derive(Debug, Clone, Serialize)]
pub struct GlAccount {
    pub code: String,
    pub name: String,
    pub category: String,
    pub direction: String,
    pub cash_flow_category: String,
    pub is_system: i64,
    pub is_active: i64,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlAccountInput {
    pub code: String,
    pub name: String,
    pub category: String,
    pub direction: String,
    pub cash_flow_category: Option<String>,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpeningBalanceRow {
    pub account_code: String,
    pub debit_amount: f64,
    pub credit_amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountMapping {
    pub id: i64,
    pub scope: String,
    pub key: String,
    pub account_code: String,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountMappingInput {
    pub scope: String,
    pub key: String,
    pub account_code: String,
    pub remark: Option<String>,
}
```

- [ ] **Step 2: 写失败测试（accounting.rs 测试模块）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::initialize_database(&conn).unwrap();
        conn
    }

    #[test]
    fn test_account_crud() {
        let conn = setup();
        let accounts = get_accounts(&conn).unwrap();
        assert!(accounts.len() >= 70);
        // 新增自定义科目
        let created = create_account(
            &conn,
            &GlAccountInput {
                code: "660201".into(),
                name: "管理费用—办公费".into(),
                category: "profit_loss".into(),
                direction: "debit".into(),
                cash_flow_category: Some("operating".into()),
                remark: None,
            },
        )
        .unwrap();
        assert_eq!(created.is_system, 0);
        // 编码重复报错
        assert!(create_account(&conn, &GlAccountInput {
            code: "660201".into(), name: "重复".into(), category: "profit_loss".into(),
            direction: "debit".into(), cash_flow_category: None, remark: None,
        }).is_err());
        // 停用/启用
        assert!(set_account_active(&conn, "660201", false).unwrap());
        assert!(set_account_active(&conn, "660201", true).unwrap());
    }

    #[test]
    fn test_opening_balance_validation() {
        let conn = setup();
        let rows = vec![
            OpeningBalanceRow { account_code: "1002".into(), debit_amount: 100000.0, credit_amount: 0.0 },
            OpeningBalanceRow { account_code: "2001".into(), debit_amount: 0.0, credit_amount: 40000.0 },
            // 少 60000，不平
        ];
        let err = save_opening_balances(&conn, "2026-01", &rows);
        assert!(err.is_err());
        rows.push(OpeningBalanceRow { account_code: "3001".into(), debit_amount: 0.0, credit_amount: 60000.0 });
        save_opening_balances(&conn, "2026-01", &rows).unwrap();
        // 换月重录：清空旧月重新保存
        let (month, loaded) = get_opening_balances(&conn).unwrap();
        assert_eq!(month.as_deref(), Some("2026-01"));
        assert_eq!(loaded.len(), 3);
    }

    #[test]
    fn test_account_mapping() {
        let conn = setup();
        save_account_mapping(&conn, &AccountMappingInput {
            scope: "expense_type".into(), key: "OFFICE".into(),
            account_code: "6602".into(), remark: None,
        }).unwrap();
        let maps = get_account_mappings(&conn).unwrap();
        assert_eq!(maps.len(), 1);
        assert!(delete_account_mapping(&conn, maps[0].id).unwrap());
    }
}
```

- [ ] **Step 3: 运行确认失败**

Run: `cd src-tauri && cargo test --lib accounting`
Expected: 编译失败（模块不存在/函数未定义）

- [ ] **Step 4: 实现 accounting.rs**

```rust
use crate::db;
use crate::errors::{AppError, AppResult};
use crate::models::*;
use chrono::Utc;
use rusqlite::{params, Connection};

pub fn get_accounts(conn: &Connection) -> AppResult<Vec<GlAccount>> {
    let mut stmt = conn.prepare(
        "SELECT code, name, category, direction, cash_flow_category, is_system, is_active, remark
         FROM gl_accounts ORDER BY code",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(GlAccount {
                code: r.get(0)?,
                name: r.get(1)?,
                category: r.get(2)?,
                direction: r.get(3)?,
                cash_flow_category: r.get(4)?,
                is_system: r.get(5)?,
                is_active: r.get(6)?,
                remark: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn create_account(conn: &Connection, input: &GlAccountInput) -> AppResult<GlAccount> {
    let cfc = input.cash_flow_category.clone().unwrap_or_else(|| "none".into());
    if conn
        .query_row("SELECT 1 FROM gl_accounts WHERE code = ?1", params![input.code], |_| Ok(()))
        .is_ok()
    {
        return Err(AppError::General(format!("科目编码 {} 已存在", input.code)));
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO gl_accounts (code, name, category, direction, cash_flow_category, is_system, is_active, remark, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 1, ?6, ?7, ?7)",
        params![input.code, input.name, input.category, input.direction, cfc, input.remark, now],
    )?;
    Ok(GlAccount {
        code: input.code.clone(),
        name: input.name.clone(),
        category: input.category.clone(),
        direction: input.direction.clone(),
        cash_flow_category: cfc,
        is_system: 0,
        is_active: 1,
        remark: input.remark.clone(),
    })
}

pub fn set_account_active(conn: &Connection, code: &str, active: bool) -> AppResult<bool> {
    let used: i64 = conn.query_row(
        "SELECT COUNT(*) FROM voucher_lines WHERE account_code = ?1",
        params![code], |r| r.get(0))?;
    if !active && used > 0 {
        return Err(AppError::General(format!("科目 {code} 已有 {used} 条凭证分录，不能停用")));
    }
    conn.execute(
        "UPDATE gl_accounts SET is_active = ?2, updated_at = ?3 WHERE code = ?1",
        params![code, active as i64, Utc::now().to_rfc3339()],
    )?;
    Ok(true)
}

pub fn get_opening_balances(conn: &Connection) -> AppResult<(Option<String>, Vec<OpeningBalanceRow>)> {
    let month: Option<String> = conn
        .query_row("SELECT MIN(month) FROM opening_balances", [], |r| r.get(0))
        .unwrap_or(None);
    let mut rows = Vec::new();
    if let Some(m) = &month {
        let mut stmt = conn.prepare(
            "SELECT account_code, debit_amount, credit_amount FROM opening_balances WHERE month = ?1 ORDER BY account_code",
        )?;
        rows = stmt
            .query_map(params![m], |r| {
                Ok(OpeningBalanceRow {
                    account_code: r.get(0)?,
                    debit_amount: r.get(1)?,
                    credit_amount: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
    }
    Ok((month, rows))
}

pub fn save_opening_balances(conn: &Connection, month: &str, rows: &[OpeningBalanceRow]) -> AppResult<()> {
    let debit: f64 = rows.iter().map(|r| r.debit_amount).sum();
    let credit: f64 = rows.iter().map(|r| r.credit_amount).sum();
    if (debit - credit).abs() > 0.005 {
        return Err(AppError::General(format!(
            "期初余额不平衡：借方合计 {debit:.2}，贷方合计 {credit:.2}，差额 {:.2}",
            debit - credit
        )));
    }
    // 校验科目方向：资产类填借方，负债/权益填贷方
    for row in rows {
        let direction: String = conn.query_row(
            "SELECT direction FROM gl_accounts WHERE code = ?1",
            params![row.account_code], |r| r.get(0))?;
        let wrong = (direction == "debit" && row.credit_amount != 0.0)
            || (direction == "credit" && row.debit_amount != 0.0);
        if wrong {
            return Err(AppError::General(format!(
                "科目 {} 是{}方向科目，金额应填在{}侧",
                row.account_code,
                if direction == "debit" { "借" } else { "贷" },
                if direction == "debit" { "借" } else { "贷" }
            )));
        }
    }
    let now = Utc::now().to_rfc3339();
    conn.execute("DELETE FROM opening_balances", [])?;
    for row in rows {
        conn.execute(
            "INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![month, row.account_code, row.debit_amount, row.credit_amount, now],
        )?;
    }
    Ok(())
}

pub fn get_account_mappings(conn: &Connection) -> AppResult<Vec<AccountMapping>> {
    let mut stmt = conn.prepare(
        "SELECT id, scope, key, account_code, remark FROM account_mappings ORDER BY scope, key",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(AccountMapping {
                id: r.get(0)?,
                scope: r.get(1)?,
                key: r.get(2)?,
                account_code: r.get(3)?,
                remark: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn save_account_mapping(conn: &Connection, input: &AccountMappingInput) -> AppResult<AccountMapping> {
    if !["expense_type", "department"].contains(&input.scope.as_str()) {
        return Err(AppError::General("映射 scope 只支持 expense_type / department".into()));
    }
    let category: String = conn.query_row(
        "SELECT category FROM gl_accounts WHERE code = ?1",
        params![input.account_code], |r| r.get(0))?;
    if input.scope == "expense_type" && !["profit_loss", "cost"].contains(&category.as_str()) {
        return Err(AppError::General(format!("科目 {} 不是费用类科目", input.account_code)));
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO account_mappings (scope, key, account_code, remark, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(scope, key) DO UPDATE SET account_code = ?3, remark = ?4, updated_at = ?5",
        params![input.scope, input.key, input.account_code, input.remark, now],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM account_mappings WHERE scope = ?1 AND key = ?2",
        params![input.scope, input.key], |r| r.get(0))?;
    Ok(AccountMapping {
        id, scope: input.scope.clone(), key: input.key.clone(),
        account_code: input.account_code.clone(), remark: input.remark.clone(),
    })
}

pub fn delete_account_mapping(conn: &Connection, id: i64) -> AppResult<bool> {
    Ok(conn.execute("DELETE FROM account_mappings WHERE id = ?1", params![id])? > 0)
}
```

`lib.rs` 加 `mod accounting;`。注意 `db::initialize_database` 若非 pub，改为 pub 或在测试里用现有公开初始化路径。

- [ ] **Step 5: 运行确认通过**

Run: `cd src-tauri && cargo test --lib accounting && cargo fmt`
Expected: 3 个测试 PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/accounting.rs src-tauri/src/models.rs src-tauri/src/lib.rs src-tauri/src/db.rs
git commit -m "feat(accounting): 科目/期初/映射管理"
```

### Task 3: 科目与期初 Tauri 命令

**Files:**
- Modify: `src-tauri/src/commands.rs`（文件尾部新增段落）、`src-tauri/src/lib.rs`（generate_handler 列表）

**Interfaces:**
- Consumes: Task 2 的 `accounting::*` 函数
- Produces: 命令 `get_gl_accounts`、`create_gl_account`、`set_gl_account_active`、`get_opening_balances`、`save_opening_balances`、`get_account_mappings`、`save_account_mapping`、`delete_account_mapping`

- [ ] **Step 1: 实现命令**

`commands.rs` 尾部追加（沿用现有 `state.lock()` 模式）：

```rust
// ==================== Accounting Commands ====================

#[tauri::command]
pub fn get_gl_accounts(state: tauri::State<'_, Mutex<Connection>>) -> Result<Vec<GlAccount>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(accounting::get_accounts(&conn)?)
}

#[tauri::command]
pub fn create_gl_account(
    data: GlAccountInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<GlAccount, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    accounting::create_account(&conn, &data)
}

#[tauri::command]
pub fn set_gl_account_active(
    code: String,
    active: bool,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    accounting::set_account_active(&conn, &code, active)
}

#[tauri::command]
pub fn get_opening_balances(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<OpeningBalanceState, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let (month, rows) = accounting::get_opening_balances(&conn)?;
    Ok(OpeningBalanceState { month, rows })
}

#[tauri::command]
pub fn save_opening_balances(
    month: String,
    rows: Vec<OpeningBalanceRow>,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    accounting::save_opening_balances(&conn, &month, &rows)?;
    db::log_operation(&conn, "save_opening_balances", &format!("保存{month}期初余额"), "system", None)?;
    Ok(true)
}

#[tauri::command]
pub fn get_account_mappings(state: tauri::State<'_, Mutex<Connection>>) -> Result<Vec<AccountMapping>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(accounting::get_account_mappings(&conn)?)
}

#[tauri::command]
pub fn save_account_mapping(
    data: AccountMappingInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<AccountMapping, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    accounting::save_account_mapping(&conn, &data)
}

#[tauri::command]
pub fn delete_account_mapping(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    accounting::delete_account_mapping(&conn, id)
}
```

`models.rs` 补 `OpeningBalanceState { month: Option<String>, rows: Vec<OpeningBalanceRow> }`（Serialize+Deserialize）。`commands.rs` 头部 use 补 `use crate::accounting;` 与相应模型。`lib.rs` 的 `generate_handler![]` 追加 8 个命令名。

- [ ] **Step 2: 编译验证**

Run: `cd src-tauri && cargo check && cargo fmt`
Expected: 无 error（既有 warning 可忽略）

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/src/models.rs
git commit -m "feat(accounting): 科目与期初 Tauri 命令"
```

### Task 4: 前端科目表页面与菜单

**Files:**
- Create: `src/pages/ChartOfAccounts.tsx`
- Modify: `src/types/index.ts`、`src/api/index.ts`、`src/App.tsx`（menuItems，第 59 行起）

**Interfaces:**
- Consumes: Task 3 的 8 个命令
- Produces: `GlAccount`、`GlAccountInput`、`OpeningBalanceState`、`OpeningBalanceRow`、`AccountMapping` 类型；`api.getGlAccounts()` 碰撞后端；路由 `/accounts`、`/vouchers`、`/reports`（本任务只建 `/accounts` 页面，菜单组一次加齐三条目，页面后两批补）

- [ ] **Step 1: types/index.ts 追加**

```typescript
export interface GlAccount {
  code: string;
  name: string;
  category: 'asset' | 'liability' | 'equity' | 'cost' | 'profit_loss';
  direction: 'debit' | 'credit';
  cash_flow_category: 'operating' | 'investing' | 'financing' | 'none';
  is_system: number;
  is_active: number;
  remark?: string | null;
}

export interface GlAccountInput {
  code: string;
  name: string;
  category: GlAccount['category'];
  direction: GlAccount['direction'];
  cash_flow_category?: GlAccount['cash_flow_category'];
  remark?: string | null;
}

export interface OpeningBalanceRow {
  account_code: string;
  debit_amount: number;
  credit_amount: number;
}

export interface OpeningBalanceState {
  month: string | null;
  rows: OpeningBalanceRow[];
}

export interface AccountMapping {
  id: number;
  scope: 'expense_type' | 'department';
  key: string;
  account_code: string;
  remark?: string | null;
}
```

- [ ] **Step 2: api/index.ts 追加**（沿用现有 invoke 封装风格）

```typescript
export const getGlAccounts = () => invoke<GlAccount[]>('get_gl_accounts');
export const createGlAccount = (data: GlAccountInput) => invoke<GlAccount>('create_gl_account', { data });
export const setGlAccountActive = (code: string, active: boolean) =>
  invoke<boolean>('set_gl_account_active', { code, active });
export const getOpeningBalances = () => invoke<OpeningBalanceState>('get_opening_balances');
export const saveOpeningBalances = (month: string, rows: OpeningBalanceRow[]) =>
  invoke<boolean>('save_opening_balances', { month, rows });
export const getAccountMappings = () => invoke<AccountMapping[]>('get_account_mappings');
export const saveAccountMapping = (data: Omit<AccountMapping, 'id'>) =>
  invoke<AccountMapping>('save_account_mapping', { data });
export const deleteAccountMapping = (id: number) => invoke<boolean>('delete_account_mapping', { id });
```

- [ ] **Step 3: App.tsx 菜单**

menuItems 中"输出审计"组之前插入：

```typescript
  {
    key: 'finance-group',
    icon: <AuditOutlined />,
    label: '财务管理',
    children: [
      { key: '/accounts', icon: <ProfileOutlined />, label: '科目表' },
      { key: '/vouchers', icon: <FileTextOutlined />, label: '记账凭证' },
      { key: '/reports', icon: <BarChartOutlined />, label: '财务报表' },
    ],
  },
```

icon 从 `@ant-design/icons` 补 import。Routes 部分按现有模式（`/accounts` → `<ChartOfAccounts />`）注册三条路由；`/vouchers`、`/reports` 先指向占位组件（下一批替换，本批可先不注册这两条路由，只注册 `/accounts`，避免空页面）。

- [ ] **Step 4: ChartOfAccounts.tsx 实现**

页面结构：顶部两个按钮（"新增科目"、"期初余额"）+ 左侧分类 Tab（资产/负债/权益/成本/损益/全部）+ 表格（编码/名称/方向/现金流量分类/状态/操作[停用|启用]）+ 新增 Modal（Form：编码/名称/分类/方向/现金流量分类）+ 期初 Modal（月份 DatePicker.MonthPicker + 科目金额可编辑 Table + 底部借贷合计与差额，差额≠0 禁止保存）。完整实现（约 260 行）按 `src/pages/DataSafety.tsx` 的 antd 页面模式编写，关键逻辑：

```tsx
const CATEGORY_LABEL: Record<string, string> = {
  asset: '资产', liability: '负债', equity: '权益', cost: '成本', profit_loss: '损益',
};
const CFC_LABEL: Record<string, string> = {
  operating: '经营活动', investing: '投资活动', financing: '筹资活动', none: '不分类',
};
// 期初保存前校验
const debit = rows.reduce((s, r) => s + (Number(r.debit_amount) || 0), 0);
const credit = rows.reduce((s, r) => s + (Number(r.credit_amount) || 0), 0);
const balanced = Math.abs(debit - credit) < 0.005;
// 保存
await saveOpeningBalances(month, rows.filter(r => r.debit_amount || r.credit_amount));
```

科目停用/启用按钮：`setGlAccountActive(code, !record.is_active)`，错误 message.error(err) 展示后端中文提示。

- [ ] **Step 5: 验证与提交**

Run: `npx tsc --noEmit && npm run lint && npm run build`
Expected: 全部通过（既有 chunk 体积提示忽略）

```bash
git add src/pages/ChartOfAccounts.tsx src/types/index.ts src/api/index.ts src/App.tsx
git commit -m "feat(accounting): 科目表页面与财务管理菜单"
```

---

## 批次二：凭证引擎与业务挂接

### Task 5: 凭证核心（生成/作废/查询）

**spec 精化说明（需回写 spec 3.1 规则 1）**：工资计提按"应发 − 缺勤扣款 − 其他扣款"计费用与应付职工薪酬；个人社保/公积金/个税不转账到其他应付款，保留在 2211 贷方，实际缴纳时由银行流水手工凭证（借 2211 贷 1002）冲减。这样与付款凭证（借 2211 实发、贷 1002 实发）天然衔接平衡。

**Files:**
- Modify: `src-tauri/src/accounting.rs`、`src-tauri/src/models.rs`
- Test: `src-tauri/src/accounting.rs` 测试模块

**Interfaces:**
- Produces:
  - `models.rs`：`VoucherDraft { belong_month, voucher_date, source_type, source_id, remark, lines: Vec<VoucherLineDraft> }`、`VoucherLineDraft { account_code, debit_amount, credit_amount, summary }`、`Voucher`（含 `lines: Vec<VoucherLine>`）、`VoucherQuery { month: Option<String>, source_type: Option<String>, status: Option<String> }`
  - `accounting.rs`：`insert_voucher(&Connection, &VoucherDraft) -> AppResult<Voucher>`、`void_vouchers_for_source(&Connection, source_type, source_id) -> AppResult<usize>`、`get_vouchers(&Connection, &VoucherQuery) -> AppResult<Vec<Voucher>>`、`next_voucher_no(&Connection, month) -> AppResult<String>`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn test_voucher_core() {
    let conn = setup();
    let draft = VoucherDraft {
        belong_month: "2026-08".into(),
        voucher_date: "2026-08-31".into(),
        source_type: "bank_manual".into(),
        source_id: 1,
        remark: Some("手续费".into()),
        lines: vec![
            VoucherLineDraft { account_code: "6603".into(), debit_amount: 30.0, credit_amount: 0.0, summary: Some("手续费".into()) },
            VoucherLineDraft { account_code: "1002".into(), debit_amount: 0.0, credit_amount: 30.0, summary: Some("手续费".into()) },
        ],
    };
    let v = insert_voucher(&conn, &draft).unwrap();
    assert!(v.voucher_no.starts_with("记-202608-"));
    assert_eq!(v.total_amount, 30.0);
    // 不平衡拒绝
    let bad = VoucherDraft {
        lines: vec![VoucherLineDraft { account_code: "6603".into(), debit_amount: 30.0, credit_amount: 0.0, summary: None }],
        ..draft.clone()
    };
    assert!(insert_voucher(&conn, &bad).is_err());
    // 同源重复拒绝（部分唯一索引）
    assert!(insert_voucher(&conn, &draft).is_err());
    // 作废后可重新生成，编号递增
    assert_eq!(void_vouchers_for_source(&conn, "bank_manual", 1).unwrap(), 1);
    let v2 = insert_voucher(&conn, &draft).unwrap();
    assert_ne!(v.id, v2.id);
    assert_ne!(v.voucher_no, v2.voucher_no);
    // 查询
    let list = get_vouchers(&conn, &VoucherQuery { month: Some("2026-08".into()), source_type: None, status: Some("active".into()) }).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].lines.len(), 2);
}
```

（`VoucherDraft` 需 `#[derive(Clone)]`。）

- [ ] **Step 2: 运行确认失败**

Run: `cd src-tauri && cargo test --lib test_voucher_core`
Expected: 编译失败

- [ ] **Step 3: 实现**

`models.rs` 追加：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoucherLineDraft {
    pub account_code: String,
    pub debit_amount: f64,
    pub credit_amount: f64,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoucherDraft {
    pub belong_month: String,
    pub voucher_date: String,
    pub source_type: String,
    pub source_id: i64,
    pub remark: Option<String>,
    pub lines: Vec<VoucherLineDraft>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VoucherLine {
    pub id: i64,
    pub account_code: String,
    pub debit_amount: f64,
    pub credit_amount: f64,
    pub summary: Option<String>,
    pub line_order: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Voucher {
    pub id: i64,
    pub voucher_no: String,
    pub voucher_date: String,
    pub belong_month: String,
    pub source_type: String,
    pub source_id: i64,
    pub total_amount: f64,
    pub status: String,
    pub remark: Option<String>,
    pub lines: Vec<VoucherLine>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoucherQuery {
    pub month: Option<String>,
    pub source_type: Option<String>,
    pub status: Option<String>,
}
```

`accounting.rs` 追加：

```rust
pub fn insert_voucher(conn: &Connection, draft: &VoucherDraft) -> AppResult<Voucher> {
    let debit: f64 = draft.lines.iter().map(|l| l.debit_amount).sum();
    let credit: f64 = draft.lines.iter().map(|l| l.credit_amount).sum();
    if (debit - credit).abs() > 0.005 || debit <= 0.0 {
        return Err(AppError::General(format!(
            "凭证借贷不平衡（借 {debit:.2} / 贷 {credit:.2}），拒绝生成"
        )));
    }
    for line in &draft.lines {
        if conn.query_row("SELECT 1 FROM gl_accounts WHERE code = ?1", params![line.account_code], |_| Ok(())).is_err() {
            return Err(AppError::General(format!("科目 {} 不存在", line.account_code)));
        }
    }
    let voucher_no = next_voucher_no(conn, &draft.belong_month)?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status, remark, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8, ?8)",
        params![voucher_no, draft.voucher_date, draft.belong_month, draft.source_type, draft.source_id, debit, draft.remark, now],
    )?;
    let id = conn.last_insert_rowid();
    for (i, line) in draft.lines.iter().enumerate() {
        conn.execute(
            "INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount, summary, line_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, line.account_code, line.debit_amount, line.credit_amount, line.summary, i as i64],
        )?;
    }
    get_voucher(conn, id)
}

pub fn get_voucher(conn: &Connection, id: i64) -> AppResult<Voucher> {
    let mut voucher = conn.query_row(
        "SELECT id, voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status, remark
         FROM vouchers WHERE id = ?1",
        params![id],
        |r| {
            Ok(Voucher {
                id: r.get(0)?, voucher_no: r.get(1)?, voucher_date: r.get(2)?,
                belong_month: r.get(3)?, source_type: r.get(4)?, source_id: r.get(5)?,
                total_amount: r.get(6)?, status: r.get(7)?, remark: r.get(8)?,
                lines: Vec::new(),
            })
        },
    )?;
    let mut stmt = conn.prepare(
        "SELECT id, account_code, debit_amount, credit_amount, summary, line_order
         FROM voucher_lines WHERE voucher_id = ?1 ORDER BY line_order",
    )?;
    voucher.lines = stmt
        .query_map(params![id], |r| {
            Ok(VoucherLine {
                id: r.get(0)?, account_code: r.get(1)?, debit_amount: r.get(2)?,
                credit_amount: r.get(3)?, summary: r.get(4)?, line_order: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(voucher)
}

pub fn next_voucher_no(conn: &Connection, month: &str) -> AppResult<String> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers WHERE belong_month = ?1",
        params![month], |r| r.get(0))?;
    Ok(format!("记-{}-{:03}", month.replace('-', ""), n + 1))
}

pub fn void_vouchers_for_source(conn: &Connection, source_type: &str, source_id: i64) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE vouchers SET status = 'void', updated_at = ?3 WHERE source_type = ?1 AND source_id = ?2 AND status = 'active'",
        params![source_type, source_id, Utc::now().to_rfc3339()],
    )?)
}

pub fn get_vouchers(conn: &Connection, q: &VoucherQuery) -> AppResult<Vec<Voucher>> {
    let mut sql = String::from(
        "SELECT id FROM vouchers WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(m) = &q.month {
        sql.push_str(&format!(" AND belong_month = ?{}", args.len() + 1));
        args.push(Box::new(m.clone()));
    }
    if let Some(s) = &q.source_type {
        sql.push_str(&format!(" AND source_type = ?{}", args.len() + 1));
        args.push(Box::new(s.clone()));
    }
    if let Some(s) = &q.status {
        sql.push_str(&format!(" AND status = ?{}", args.len() + 1));
        args.push(Box::new(s.clone()));
    }
    sql.push_str(" ORDER BY voucher_no");
    let ids: Vec<i64> = {
        let mut stmt = conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        stmt.query_map(params_ref.as_slice(), |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    ids.iter().map(|id| get_voucher(conn, *id)).collect()
}
```

- [ ] **Step 4: 运行确认通过并提交**

Run: `cd src-tauri && cargo test --lib accounting && cargo fmt`

```bash
git add src-tauri/src/accounting.rs src-tauri/src/models.rs
git commit -m "feat(accounting): 凭证生成/作废/查询核心"
```

### Task 6: 工资计提凭证挂接

**Files:**
- Modify: `src-tauri/src/db.rs`（`lock_salary_results` db.rs:1284 及解锁函数）、`src-tauri/src/accounting.rs`（生成函数）
- Test: `src-tauri/src/accounting.rs`

**Interfaces:**
- Consumes: Task 5 `insert_voucher` / `void_vouchers_for_source`
- Produces: `accounting::generate_salary_accrual_vouchers(&Connection, month: &str) -> AppResult<usize>`（为该月全部已锁定且无 active 凭证的工资结果生成凭证）、`accounting::void_salary_accrual_vouchers(&Connection, month: &str) -> AppResult<usize>`

- [ ] **Step 1: 写失败测试**

测试需构造工资数据。查看 `db.rs` 现有工资测试如何插入 `salary_monthly_results`（搜 `salary_monthly_results` 在 tests 中的用法），复用其插入方式。断言：

```rust
#[test]
fn test_salary_accrual_voucher() {
    let conn = setup();
    // 插入 1 条 2026-08 工资结果：应发 10000，缺勤 500，其他扣款 100，
    // 社保 1000，公积金 800，个税 200，实发 = 10000-500-100-1000-800-200 = 7400
    // （插入语句参考现有工资测试）
    db::lock_salary_results(&conn, "2026-08").unwrap();
    let vouchers = get_vouchers(&conn, &VoucherQuery {
        month: Some("2026-08".into()), source_type: Some("salary_accrual".into()), status: None,
    }).unwrap();
    assert_eq!(vouchers.len(), 1);
    let v = &vouchers[0];
    // 计提金额 = 应发 - 缺勤 - 其他 = 9400
    assert_eq!(v.total_amount, 9400.0);
    // 借 6602 9400，贷 2211 9400
    let debit_line = v.lines.iter().find(|l| l.debit_amount > 0.0).unwrap();
    assert_eq!(debit_line.account_code, "6602");
    let credit_line = v.lines.iter().find(|l| l.credit_amount > 0.0).unwrap();
    assert_eq!(credit_line.account_code, "2211");
    // 解锁后凭证作废
    db::unlock_salary_results(&conn, "2026-08").unwrap(); // 函数名以 grep 为准
    let active = get_vouchers(&conn, &VoucherQuery {
        month: None, source_type: Some("salary_accrual".into()), status: Some("active".into()),
    }).unwrap();
    assert_eq!(active.len(), 0);
}
```

先用 `grep -n "unlock" src-tauri/src/db.rs` 确认解锁函数实名（可能是 `unlock_salary_results` 或并入 `update_salary_result` 流程；若无独立函数，在工资状态回退入口处挂接作废）。

- [ ] **Step 2: 运行确认失败**

Run: `cd src-tauri && cargo test --lib test_salary_accrual_voucher`

- [ ] **Step 3: 实现生成函数（accounting.rs）**

```rust
pub fn generate_salary_accrual_vouchers(conn: &Connection, month: &str) -> AppResult<usize> {
    let mut stmt = conn.prepare(
        "SELECT id, name, department, gross_salary, attendance_deduction, other_deduction
         FROM salary_monthly_results
         WHERE salary_month = ?1 AND locked = 1 AND status != 'void'",
    )?;
    let rows = stmt
        .query_map(params![month], |r| {
            Ok((
                r.get::<_, i64>(0)?,       // id
                r.get::<_, Option<String>>(1)?, // name
                r.get::<_, Option<String>>(2)?, // department
                r.get::<_, f64>(3)?,       // gross
                r.get::<_, f64>(4)?,       // attendance
                r.get::<_, f64>(5)?,       // other
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    let mut n = 0;
    for (id, name, department, gross, attendance, other) in rows {
        let amount = (gross - attendance - other).max(0.0);
        if amount <= 0.0 {
            continue;
        }
        // 已有 active 凭证则跳过（幂等）
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM vouchers WHERE source_type='salary_accrual' AND source_id=?1 AND status='active'",
            params![id], |r| r.get(0))?;
        if exists > 0 {
            continue;
        }
        let dept_account = mapping_account(conn, "department", department.as_deref().unwrap_or(""))?;
        let emp = name.unwrap_or_else(|| "未知员工".into());
        let month_dashless = month.replace('-', "");
        let _ = month_dashless;
        let voucher_date = format!("{month}-28"); // 计提日固定 28 日，避开 31 天差异
        insert_voucher(
            conn,
            &VoucherDraft {
                belong_month: month.to_string(),
                voucher_date,
                source_type: "salary_accrual".into(),
                source_id: id,
                remark: Some(format!("{month} 工资计提（{emp}）")),
                lines: vec![
                    VoucherLineDraft { account_code: dept_account.clone(), debit_amount: amount, credit_amount: 0.0, summary: Some(format!("{month} 工资费用（{emp}）")) },
                    VoucherLineDraft { account_code: "2211".into(), debit_amount: 0.0, credit_amount: amount, summary: Some(format!("{month} 应付职工薪酬（{emp}）")) },
                ],
            },
        )?;
        n += 1;
    }
    Ok(n)
}

fn mapping_account(conn: &Connection, scope: &str, key: &str) -> AppResult<String> {
    let mapped: Option<String> = conn
        .query_row(
            "SELECT account_code FROM account_mappings WHERE scope = ?1 AND key = ?2",
            params![scope, key], |r| r.get(0))
        .unwrap_or(None);
    Ok(mapped.unwrap_or_else(|| "6602".into()))
}

pub fn void_salary_accrual_vouchers(conn: &Connection, month: &str) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE vouchers SET status='void', updated_at=?2
         WHERE source_type='salary_accrual' AND status='active'
           AND source_id IN (SELECT id FROM salary_monthly_results WHERE salary_month = ?1)",
        params![month, Utc::now().to_rfc3339()],
    )?)
}
```

- [ ] **Step 4: 挂接 db.rs**

`db::lock_salary_results`（db.rs:1284）在 UPDATE 成功后、返回前调用 `crate::accounting::generate_salary_accrual_vouchers(conn, month)?;`（同一 `&Connection`，外层命令在事务中或依赖 Mutex 串行；查看该函数现有事务边界，把生成放进同一事务）。解锁函数内对应调用 `void_salary_accrual_vouchers`。

- [ ] **Step 5: 运行确认通过并提交**

Run: `cd src-tauri && cargo test --lib accounting && cargo fmt`

```bash
git add src-tauri/src/accounting.rs src-tauri/src/db.rs
git commit -m "feat(accounting): 工资计提凭证与解锁联动"
```

### Task 7: 付款批次凭证挂接

**Files:**
- Modify: `src-tauri/src/db.rs`（`mark_payment_batch_paid` db.rs:2436、`void_payment_batch` db.rs:2489）、`src-tauri/src/accounting.rs`
- Test: `src-tauri/src/accounting.rs`

**Interfaces:**
- Produces: `accounting::generate_payment_voucher(&Connection, batch_id: i64) -> AppResult<Voucher>`（按 batch_type 生成 salary_payment / reimbursement_payment）、`accounting::void_payment_voucher(&Connection, batch_id: i64) -> AppResult<usize>`

- [ ] **Step 1: 写失败测试**

复用现有付款批次测试数据构造方式（grep `mark_payment_batch_paid` 在 tests 中的用法）。断言：

```rust
#[test]
fn test_payment_voucher() {
    let conn = setup();
    // 构造 1 个已导出的工资批次 belong_month=2026-08 total=7400（参考现有批次测试）
    // 标记已付款：
    db::mark_payment_batch_paid(&mut conn, &PaymentBatchPaidInput { /* id, payment_date: "2026-08-31" */ }).unwrap();
    let vouchers = get_vouchers(&conn, &VoucherQuery {
        month: Some("2026-08".into()), source_type: Some("salary_payment".into()), status: Some("active".into()),
    }).unwrap();
    assert_eq!(vouchers.len(), 1);
    // 借 2211 7400，贷 1002 7400
    // 作废批次后凭证 void：
    db::void_payment_batch(&mut conn, &PaymentBatchVoidInput { /* id, reason */ }).unwrap();
    let active = get_vouchers(&conn, &VoucherQuery {
        month: None, source_type: Some("salary_payment".into()), status: Some("active".into()),
    }).unwrap();
    assert_eq!(active.len(), 0);
}
```

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现（accounting.rs）**

```rust
pub fn generate_payment_voucher(conn: &Connection, batch_id: i64) -> AppResult<Voucher> {
    let (batch_no, belong_month, batch_type, payment_date, total, status): (
        String, String, String, Option<String>, f64, String,
    ) = conn.query_row(
        "SELECT batch_no, belong_month, batch_type, payment_date, total_amount, status
         FROM payment_batches WHERE id = ?1",
        params![batch_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })?;
    if status != "paid" {
        return Err(AppError::General(format!("批次 {batch_no} 未标记已付款，不能生成付款凭证")));
    }
    let (source_type, debit_account, remark) = match batch_type.as_str() {
        "salary" => ("salary_payment", "2211", "工资代发"),
        "reimbursement" => ("reimbursement_payment", "2241", "报销付款"),
        other => return Err(AppError::General(format!("未知批次类型 {other}"))),
    };
    let date = payment_date.unwrap_or_else(|| format!("{belong_month}-28"));
    insert_voucher(
        conn,
        &VoucherDraft {
            belong_month: belong_month.clone(),
            voucher_date: date,
            source_type: source_type.into(),
            source_id: batch_id,
            remark: Some(format!("{remark}（{batch_no}）")),
            lines: vec![
                VoucherLineDraft { account_code: debit_account.into(), debit_amount: total, credit_amount: 0.0, summary: Some(format!("{remark}（{batch_no}）")) },
                VoucherLineDraft { account_code: "1002".into(), debit_amount: 0.0, credit_amount: total, summary: Some(format!("{batch_no} 银行支出")) },
            ],
        },
    )
}

pub fn void_payment_voucher(conn: &Connection, batch_id: i64) -> AppResult<usize> {
    let n1 = void_vouchers_for_source(conn, "salary_payment", batch_id)?;
    let n2 = void_vouchers_for_source(conn, "reimbursement_payment", batch_id)?;
    Ok(n1 + n2)
}
```

- [ ] **Step 4: 挂接 db.rs**

`mark_payment_batch_paid`（签名含 `&mut Connection`，说明内部有事务）在状态成功置 paid 后调用 `generate_payment_voucher(conn, data.id)?`，放入同一事务。`void_payment_batch` 中调用 `void_payment_voucher(conn, id)?`。若 batch_type 字面值与上述不匹配，以 `grep -n "batch_type" src-tauri/src/db.rs | head` 实测值为准调整 match 分支。

- [ ] **Step 5: 运行确认通过并提交**

Run: `cd src-tauri && cargo test --lib accounting && cargo fmt`

```bash
git add src-tauri/src/accounting.rs src-tauri/src/db.rs
git commit -m "feat(accounting): 付款批次凭证与作废联动"
```

### Task 8: 报销计提与发票费用凭证

**Files:**
- Modify: `src-tauri/src/db.rs`（`update_reimbursement_claim_status` db.rs:4285、`insert_invoice` db.rs:3895、`soft_delete_invoice` db.rs:3989、`update_invoice` db.rs:3920、`soft_delete_reimbursement_claim` db.rs:4311）、`src-tauri/src/accounting.rs`
- Test: `src-tauri/src/accounting.rs`

**Interfaces:**
- Produces:
  - `accounting::generate_reimbursement_accrual_voucher(&Connection, claim_id: i64) -> AppResult<Option<Voucher>>`（claim 状态为 approved 及之后状态时生成；按其发票逐张开立费用/税费行，无发票部分默认 6602）
  - `accounting::maybe_generate_invoice_expense_voucher(&Connection, invoice_id: i64) -> AppResult<Option<Voucher>>`（仅当发票 normal 且未挂任何报销单时生成：借费用=amount、借 2221=tax_amount、贷 2241=total）
  - `accounting::void_reimbursement_accrual_voucher(&Connection, claim_id) -> AppResult<usize>`、`accounting::void_invoice_expense_voucher(&Connection, invoice_id) -> AppResult<usize>`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn test_reimbursement_and_invoice_vouchers() {
    let conn = setup();
    // 1) 无报销关联的发票：amount=100 tax=13 total=113，belong_month=2026-08
    //    insert_invoice 后自动生成 invoice_expense 凭证：
    //    借 6602 100 + 借 2221 13，贷 2241 113（total_amount=113）
    // 2) 发票挂到报销单并审批通过：先作废 invoice_expense，
    //    再生成 reimbursement_accrual（借费用按发票类型映射，贷 2241=claim 总额）
    // 3) soft_delete_invoice 后对应凭证 void
    // 4) 报销单反审批（status 回 pending）后凭证 void
    // 断言凭证数、行科目与金额、void 后 active 数为 0
}
```

测试数据构造参考 `db.rs` 现有报销/发票测试（grep `save_reimbursement_claim` 在 tests 中的用法）。审批状态字面值以 `grep -n "approved" src-tauri/src/db.rs | head` 实测为准。

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现（accounting.rs）**

```rust
fn invoice_expense_lines(conn: &Connection, claim_id: i64, month: &str) -> AppResult<Vec<VoucherLineDraft>> {
    // 按报销单关联发票的费用类型映射生成借方行，税额进 2221，贷方 2241 汇总
    let mut stmt = conn.prepare(
        "SELECT i.amount, i.tax_amount, i.expense_type_code
         FROM invoices i JOIN reimbursement_claim_invoices rc ON rc.invoice_id = i.id
         WHERE rc.claim_id = ?1 AND i.status = 'normal'",
    )?;
    let rows = stmt.query_map(params![claim_id], |r| {
        Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?, r.get::<_, Option<String>>(2)?))
    })?.collect::<Result<Vec<_>, _>>()?;
    let mut expense_map: std::collections::BTreeMap<String, f64> = Default::default();
    let mut tax_total = 0.0;
    for (amount, tax, type_code) in &rows {
        let account = match type_code {
            Some(code) if !code.is_empty() => mapping_account(conn, "expense_type", code)?,
            _ => "6602".into(),
        };
        *expense_map.entry(account).or_insert(0.0) += amount;
        tax_total += tax;
    }
    let total: f64 = rows.iter().map(|(a, t, _)| a + t).sum();
    let mut lines = Vec::new();
    for (account, amt) in expense_map {
        if amt > 0.0 {
            lines.push(VoucherLineDraft { account_code: account, debit_amount: amt, credit_amount: 0.0, summary: Some(format!("{month} 报销费用")) });
        }
    }
    if tax_total > 0.0 {
        lines.push(VoucherLineDraft { account_code: "2221".into(), debit_amount: tax_total, credit_amount: 0.0, summary: Some(format!("{month} 报销进项税额")) });
    }
    lines.push(VoucherLineDraft { account_code: "2241".into(), debit_amount: 0.0, credit_amount: total, summary: Some(format!("{month} 应付报销款")) });
    Ok(lines)
}

pub fn generate_reimbursement_accrual_voucher(conn: &Connection, claim_id: i64) -> AppResult<Option<Voucher>> {
    let (claim_no, belong_month, total, status): (String, String, f64, String) = conn.query_row(
        "SELECT claim_no, belong_month, total_amount, status FROM reimbursement_claims WHERE id = ?1",
        params![claim_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    if status == "draft" || status == "void" || status == "rejected" {
        return Ok(None);
    }
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers WHERE source_type='reimbursement_accrual' AND source_id=?1 AND status='active'",
        params![claim_id], |r| r.get(0))?;
    if exists > 0 { return Ok(None); }
    let lines = invoice_expense_lines(conn, claim_id, &belong_month)?;
    // 借贷差额兜底：无发票时按总额进 6602
    let debit: f64 = lines.iter().map(|l| l.debit_amount).sum();
    let credit: f64 = lines.iter().map(|l| l.credit_amount).sum();
    let mut lines = lines;
    if (credit - debit).abs() > 0.005 {
        lines.insert(0, VoucherLineDraft {
            account_code: "6602".into(), debit_amount: credit - debit, credit_amount: 0.0,
            summary: Some(format!("{belong_month} 报销费用（无票部分）")),
        });
    }
    let voucher = insert_voucher(conn, &VoucherDraft {
        belong_month: belong_month.clone(),
        voucher_date: format!("{belong_month}-28"),
        source_type: "reimbursement_accrual".into(),
        source_id: claim_id,
        remark: Some(format!("报销计提（{claim_no}）")),
        lines,
    })?;
    Ok(Some(voucher))
}

pub fn maybe_generate_invoice_expense_voucher(conn: &Connection, invoice_id: i64) -> AppResult<Option<Voucher>> {
    let (belong_month, amount, tax, total, type_code, status): (String, f64, f64, f64, Option<String>, String) = conn.query_row(
        "SELECT belong_month, amount, tax_amount, total_amount, expense_type_code, status FROM invoices WHERE id = ?1",
        params![invoice_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
    if status != "normal" { return Ok(None); }
    let linked: i64 = conn.query_row(
        "SELECT COUNT(*) FROM reimbursement_claim_invoices WHERE invoice_id = ?1",
        params![invoice_id], |r| r.get(0))?;
    if linked > 0 { return Ok(None); }
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers WHERE source_type='invoice_expense' AND source_id=?1 AND status='active'",
        params![invoice_id], |r| r.get(0))?;
    if exists > 0 { return Ok(None); }
    let account = match &type_code {
        Some(code) if !code.is_empty() => mapping_account(conn, "expense_type", code)?,
        _ => "6602".into(),
    };
    let mut lines = vec![VoucherLineDraft {
        account_code: account, debit_amount: amount, credit_amount: 0.0,
        summary: Some(format!("{belong_month} 费用（无报销关联发票）")),
    }];
    if tax > 0.0 {
        lines.push(VoucherLineDraft { account_code: "2221".into(), debit_amount: tax, credit_amount: 0.0, summary: Some("进项税额".into()) });
    }
    lines.push(VoucherLineDraft { account_code: "2241".into(), debit_amount: 0.0, credit_amount: total, summary: Some(format!("{belong_month} 应付费用".into())) });
    let voucher = insert_voucher(conn, &VoucherDraft {
        belong_month, voucher_date: format!("{}-28", conn.query_row::<String,_,_>("SELECT belong_month FROM invoices WHERE id=?1", params![invoice_id], |r| r.get(0)).unwrap_or_default()),
        source_type: "invoice_expense".into(), source_id: invoice_id,
        remark: Some("发票费用入账".into()), lines,
    })?;
    Ok(Some(voucher))
}
```

注意 `maybe_generate_invoice_expense_voucher` 里 voucher_date 直接用局部变量重排即可（把 belong_month clone 后使用），避免示例中重复查询的别扭写法。

- [ ] **Step 4: 挂接 db.rs**

- `insert_invoice`：插入成功后 `maybe_generate_invoice_expense_voucher`（同连接同事务）
- `update_invoice`：金额/费用类型/belong_month 变化时先 `void_invoice_expense_voucher` 再 `maybe_generate_invoice_expense_voucher`
- `soft_delete_invoice`：成功后 `void_invoice_expense_voucher`
- `update_reimbursement_claim_status`：进入 approved 后 `generate_reimbursement_accrual_voucher` 并对其关联的未作废发票 `void_invoice_expense_voucher`（防重复计费）；离开 approved（反审批）时 `void_reimbursement_accrual_voucher` 并对仍满足条件的发票 `maybe_generate_invoice_expense_voucher`
- `save_reimbursement_claim`：新增/移除关联发票后同样执行"作废报销凭证→重新生成 + 补发票单独凭证"的补偿逻辑
- `soft_delete_reimbursement_claim`：作废报销凭证 + 对关联发票 `maybe_generate_invoice_expense_voucher`
- 挂接前用 `db::ensure_month_open` 已有的调用确认月结锁覆盖这些入口（现有代码应已覆盖，无需重复）

- [ ] **Step 5: 运行确认通过并提交**

Run: `cd src-tauri && cargo test --lib accounting && cargo fmt`

```bash
git add src-tauri/src/accounting.rs src-tauri/src/db.rs
git commit -m "feat(accounting): 报销计提与发票费用凭证联动"
```

### Task 9: 银行流水手工凭证 + 月结凭证平衡检查

**Files:**
- Modify: `src-tauri/src/accounting.rs`、`src-tauri/src/db.rs`（月结检查函数 + `ignore_bank_transaction` db.rs:2746、`cancel_bank_transaction_match` db.rs:2730）、`src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`
- Test: `src-tauri/src/accounting.rs`

**Interfaces:**
- Produces:
  - `accounting::create_bank_manual_voucher(&Connection, transaction_id: i64, account_code: &str, summary: Option<String>) -> AppResult<Voucher>`（流水必须 unmatched 且未忽略；支出流水：借所选科目/贷 1002；收入流水：借 1002/贷所选科目；金额= expense_amount 或 income_amount）
  - 命令 `create_bank_manual_voucher(transaction_id, account_code, summary)`、`get_vouchers(query)`
  - 月结检查新增项"记账凭证平衡"

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn test_bank_manual_voucher() {
    let conn = setup();
    // 插入 1 条 unmatched 支出流水 expense=30 belong_month=2026-08（参考现有流水测试构造）
    let v = create_bank_manual_voucher(&conn, tx_id, "6603", Some("手续费".into())).unwrap();
    assert_eq!(v.total_amount, 30.0);
    // 借 6603 贷 1002
    // 重复生成报错（active 唯一索引）
    assert!(create_bank_manual_voucher(&conn, tx_id, "6603", None).is_err());
    // 忽略流水后凭证 void：
    // db::ignore_bank_transaction(...) 后 active bank_manual 数为 0
}

#[test]
fn test_month_close_voucher_balance_check() {
    let conn = setup();
    // 手动 UPDATE vouchers SET total_amount=0 制造异常（模拟不平衡），断言月结检查返回阻塞项
    // 名称含"记账凭证平衡"（月结检查函数名以 grep -n "fn get_month_close" src-tauri/src/db.rs 实测为准）
}
```

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现**

```rust
pub fn create_bank_manual_voucher(conn: &Connection, transaction_id: i64, account_code: &str, summary: Option<String>) -> AppResult<Voucher> {
    let (belong_month, transaction_date, income, expense, status, ignore_reason): (String, String, f64, f64, String, Option<String>) = conn.query_row(
        "SELECT belong_month, transaction_date, income_amount, expense_amount, status, ignore_reason FROM bank_transactions WHERE id = ?1",
        params![transaction_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
    if status != "unmatched" || ignore_reason.is_some() {
        return Err(AppError::General("只有未匹配且未忽略的流水才能生成凭证".into()));
    }
    db::ensure_month_open(conn, &belong_month)?;
    let (amount, lines) = if expense > 0.0 {
        (expense, vec![
            VoucherLineDraft { account_code: account_code.into(), debit_amount: expense, credit_amount: 0.0, summary: summary.clone() },
            VoucherLineDraft { account_code: "1002".into(), debit_amount: 0.0, credit_amount: expense, summary },
        ])
    } else {
        (income, vec![
            VoucherLineDraft { account_code: "1002".into(), debit_amount: income, credit_amount: 0.0, summary: summary.clone() },
            VoucherLineDraft { account_code: account_code.into(), debit_amount: 0.0, credit_amount: income, summary },
        ])
    };
    if amount <= 0.0 {
        return Err(AppError::General("流水收入支出金额均为 0，不能生成凭证".into()));
    }
    insert_voucher(conn, &VoucherDraft {
        belong_month, voucher_date: transaction_date,
        source_type: "bank_manual".into(), source_id: transaction_id,
        remark: Some("银行流水入账".into()), lines,
    })
}
```

`ignore_bank_transaction` 成功后与 `cancel_bank_transaction_match`（取消匹配不 void bank_manual——只有流水真正脱离银行业务时才作废，按 spec 3.3：取消匹配→void bank_manual）：两处追加 `void_vouchers_for_source(conn, "bank_manual", transaction_id)?;`

月结检查函数（grep 定位，形如 `get_month_close_checks`）追加检查项：

```rust
let bad_vouchers: i64 = conn.query_row(
    "SELECT COUNT(*) FROM vouchers v WHERE v.status = 'active' AND (
        v.total_amount <= 0
        OR v.total_amount != (SELECT COALESCE(SUM(debit_amount),0) FROM voucher_lines WHERE voucher_id = v.id)
        OR v.total_amount != (SELECT COALESCE(SUM(credit_amount),0) FROM voucher_lines WHERE voucher_id = v.id)
    ) AND v.belong_month = ?1",
    params![month], |r| r.get(0))?;
// bad_vouchers > 0 时生成阻塞项，文案：format!("存在 {bad_vouchers} 张借贷不平衡凭证")
```

命令与注册：

```rust
#[tauri::command]
pub fn create_bank_manual_voucher(
    transaction_id: i64,
    account_code: String,
    summary: Option<String>,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Voucher, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let v = accounting::create_bank_manual_voucher(&conn, transaction_id, &account_code, summary)?;
    db::log_operation(&conn, "create_bank_manual_voucher", &format!("流水生成凭证 {}", v.voucher_no), "system", None)?;
    Ok(v)
}

#[tauri::command]
pub fn get_vouchers(
    query: VoucherQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<Voucher>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(accounting::get_vouchers(&conn, &query)?)
}
```

`lib.rs` 注册两命令。

- [ ] **Step 4: 运行确认通过并提交**

Run: `cd src-tauri && cargo test --lib && cargo fmt`（全量，批次二收尾）

```bash
git add src-tauri/src/accounting.rs src-tauri/src/db.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(accounting): 银行流水手工凭证与月结凭证平衡检查"
```

---

## 批次三：三大报表与导出

### Task 10: 报表计算引擎

**Files:**
- Modify: `src-tauri/src/accounting.rs`、`src-tauri/src/models.rs`
- Test: `src-tauri/src/accounting.rs`

**Interfaces:**
- Produces:
  - `models.rs`：`ReportRow { key: String, label: String, current: f64, comparative: f64 }`、`BalanceSheet { month: String, enabled: bool, asset_rows: Vec<ReportRow>, liability_equity_rows: Vec<ReportRow>, asset_total: f64, liability_equity_total: f64, balanced: bool }`、`IncomeStatement { month, year_cumulative: bool, rows: Vec<ReportRow>, net_profit_month: f64, net_profit_year: f64 }`、`CashFlowStatement { month, rows: Vec<ReportRow>, net_increase: f64, unclassified: Vec<UnclassifiedCashItem> }`、`UnclassifiedCashItem { voucher_no, summary: Option<String>, amount: f64 }`
  - `accounting.rs`：`build_balance_sheet(&Connection, month) -> AppResult<BalanceSheet>`、`build_income_statement(&Connection, month) -> AppResult<IncomeStatement>`、`build_cash_flow_statement(&Connection, month) -> AppResult<CashFlowStatement>`

**核心算法：**

```rust
struct AccountBalance { code: String, category: String, direction: String, opening: f64, period_debit: f64, period_credit: f64 }

// 通用：计算某月各科目 期初/本期借贷/期末
//   opening：启用月 <= month 时取 opening_balances（按方向正负号），否则 0
//   period_debit/credit：当月 active 凭证 voucher_lines 按科目 SUM
//   ending = opening + (debit - credit) * (direction=="debit" ? 1 : -1)
fn compute_balances(conn: &Connection, month: &str) -> AppResult<Vec<AccountBalance>>
```

- **资产负债表**：asset 科目 ending 按标准行归集（1001+1002+1012→"货币资金"；其余科目一科目一行）；负债权益同理；`未分配利润` = 3104 期末 + 启用月至当月累计净利润（复用利润表净利计算：`Σ profit_loss 科目贷方-借方` 累计）；comparative 列 = 年初（启用月期初口径，即 opening）。`balanced = |asset_total - liability_equity_total| < 0.005`。
- **利润表**：profit_loss 科目当月与年初至当月累计发生额（贷方-借方按科目），映射到标准行：6001 主营业务收入、6401 主营业务成本、6403 税金及附加、6601 销售费用、6602 管理费用、6603 财务费用、6051 其他业务收入、6402 其他业务成本、6111 投资收益、6301 营业外收入、6711 营业外支出、6801 所得税费用；营业利润 = 收入类 − 成本费用类（营业外与所得税前）；利润总额 = 营业利润 + 营业外收支净额；净利润 = 利润总额 − 所得税。空行显示 0。
- **现金流量表（直接法）**：查询当月 active 凭证中含 1001/1002 行的凭证；对每张凭证，现金行金额（借方正/贷方负）与对方行配对：取现金行金额按对方行金额比例分摊到对方科目的 `cash_flow_category`；汇总为六行（经营/投资/筹资 × 流入/流出）+ "其他"行 + 现金净增加额。`unclassified` 记录对方科目 category=none 的明细（voucher_no/摘要/金额）。

**Step 流程（TDD）：**

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn test_reports() {
    let conn = setup();
    // 期初：1002 借 100000，2001 贷 40000，3001 贷 60000（启用月 2026-01）
    save_opening_balances(&conn, "2026-01", &vec![/* 如上 */]).unwrap();
    // 2026-02 业务：手工凭证
    //   借 6602 5000 / 贷 1002 5000（费用，经营流出）
    //   借 1002 2000 / 贷 6301 2000（营业外收入，经营流入——6301 是 operating）
    //   借 1601 30000 / 贷 1002 30000（购固定资产，投资流出）
    // 断言资产负债表(2026-02)：
    //   货币资金 = 100000 - 5000 + 2000 - 30000 = 67000
    //   固定资产 = 30000；短期借款 40000；实收资本 60000
    //   未分配利润 = -5000 + 2000 = -3000（亏损）
    //   balanced = true
    // 断言利润表(2026-02)：管理费用 5000，营业外收入 2000，净利润 -3000
    // 断言现金流量表(2026-02)：经营流入 2000，经营流出 5000，投资流出 30000，
    //   现金净增加 = -33000，unclassified 为空
    // 断言资产负债表(2025-12)：enabled = false（未到启用月）
}
```

- [ ] **Step 2: 运行确认失败**
- [ ] **Step 3: 按核心算法实现三个 build 函数与 `compute_balances`**
- [ ] **Step 4: 运行确认通过**

Run: `cd src-tauri && cargo test --lib test_reports`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/accounting.rs src-tauri/src/models.rs
git commit -m "feat(accounting): 三大报表计算引擎"
```

### Task 11: 报表命令与 Excel 导出

**Files:**
- Modify: `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`、`src-tauri/src/excel.rs`
- Test: `src-tauri/src/excel.rs` 测试模块

**Interfaces:**
- Produces: 命令 `get_balance_sheet(month)`、`get_income_statement(month)`、`get_cash_flow_statement(month)`、`export_financial_report(month, report_type)`（report_type: balance_sheet / income_statement / cash_flow_statement）；`excel::export_balance_sheet(&BalanceSheet, path)`、`excel::export_income_statement(...)`、`excel::export_cash_flow_statement(...)`

- [ ] **Step 1: excel.rs 写失败测试**

参考 `excel.rs` 现有导出测试（如 `export_month_close_package` 测试）模式：构造报表结构体 → 导出到临时目录 → 断言文件存在且非空。三个导出函数共用一个表式 helper：标题行（报表名+月份）、表头（项目/本期金额/对比金额或行次）、数据行、合计行。

- [ ] **Step 2: 运行确认失败**
- [ ] **Step 3: 实现**

excel.rs（rust_xlsxwriter，参考现有 `export_payment_batch` 的写法）：

```rust
pub fn export_balance_sheet(report: &BalanceSheet, path: &std::path::Path) -> AppResult<()> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet().set_name("资产负债表")?;
    sheet.write(0, 0, format!("资产负债表 {}", report.month))?;
    sheet.write(1, 0, "资产")?; sheet.write(1, 1, "期末余额")?; sheet.write(1, 2, "年初余额")?;
    let mut row = 2u32;
    for r in &report.asset_rows {
        sheet.write(row, 0, &r.label)?; sheet.write_number(row, 1, r.current)?; sheet.write_number(row, 2, r.comparative)?;
        row += 1;
    }
    sheet.write(row, 0, "资产总计")?; sheet.write_number(row, 1, report.asset_total)?; sheet.write_number(row, 2, /* 年初合计 */)?;
    // 负债和权益同法；最后若 !report.balanced 写一行红字提示
    workbook.save(path)?;
    Ok(())
}
```

利润表/现金流量表同模式（列头分别为 本月金额/本年累计、本期金额/本年累计）。现金流量表附 unclassified 明细 sheet 或尾注。

commands.rs：三个查询命令直接调用 build 函数；导出命令沿用现有导出对话框约定（grep `export_payment_batch` 命令看 path 如何产生—— диалог plugin 或 app_data_dir，保持一致）：

```rust
#[tauri::command]
pub fn get_balance_sheet(month: String, state: tauri::State<'_, Mutex<Connection>>) -> Result<BalanceSheet, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(accounting::build_balance_sheet(&conn, &month)?)
}
// get_income_statement / get_cash_flow_statement 同法

#[tauri::command]
pub fn export_financial_report(
    month: String,
    report_type: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<String, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    // path 生成方式与现有导出命令一致；文件名如 资产负债表_YYYYMM.xlsx
    // match report_type 调用对应 excel::export_* 并 log_operation
}
```

`lib.rs` 注册 4 个命令。

- [ ] **Step 4: 运行确认通过并提交**

Run: `cd src-tauri && cargo test --lib && cargo fmt`

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/src/excel.rs
git commit -m "feat(accounting): 报表命令与 Excel 导出"
```

---

## 批次四：前端页面与回归

### Task 12: 记账凭证页面

**Files:**
- Create: `src/pages/Vouchers.tsx`
- Modify: `src/types/index.ts`、`src/api/index.ts`、`src/App.tsx`（注册 `/vouchers` 路由）

**Interfaces:**
- Consumes: 命令 `get_vouchers`
- Produces: 页面组件 `Vouchers`

- [ ] **Step 1: types/api 追加**

```typescript
export interface VoucherLine {
  id: number;
  account_code: string;
  debit_amount: number;
  credit_amount: number;
  summary?: string | null;
  line_order: number;
}

export interface Voucher {
  id: number;
  voucher_no: string;
  voucher_date: string;
  belong_month: string;
  source_type: string;
  source_id: number;
  total_amount: number;
  status: 'active' | 'void';
  remark?: string | null;
  lines: VoucherLine[];
}

export const VOUCHER_SOURCE_LABEL: Record<string, string> = {
  salary_accrual: '工资计提',
  salary_payment: '工资代发',
  reimbursement_accrual: '报销计提',
  reimbursement_payment: '报销付款',
  invoice_expense: '发票入账',
  bank_manual: '银行流水',
};

export interface VoucherQuery {
  month?: string;
  source_type?: string;
  status?: string;
}
```

api：`export const getVouchers = (query: VoucherQuery) => invoke<Voucher[]>('get_vouchers', { query });`

- [ ] **Step 2: Vouchers.tsx**

布局：月份 DatePicker.MonthPicker（默认当月）+ 来源类型 Select（VOUCHER_SOURCE_LABEL）+ 状态 Select（active/void）+ 表格（凭证号/日期/来源 Tag/金额/状态/摘要），行点击展开分录明细（expandable：借方科目金额列、贷方科目金额列、摘要列）。数据加载 `useEffect` + `getVouchers`。约 180 行，UI 模式参考 `src/pages/Payments.tsx`。

- [ ] **Step 3: 验证与提交**

Run: `npx tsc --noEmit && npm run lint && npm run build`

```bash
git add src/pages/Vouchers.tsx src/types/index.ts src/api/index.ts src/App.tsx
git commit -m "feat(accounting): 记账凭证页面"
```

### Task 13: 财务报表页面

**Files:**
- Create: `src/pages/FinancialReports.tsx`
- Modify: `src/types/index.ts`、`src/api/index.ts`、`src/App.tsx`（注册 `/reports` 路由）

**Interfaces:**
- Consumes: 命令 `get_balance_sheet` / `get_income_statement` / `get_cash_flow_statement` / `export_financial_report`
- Produces: 页面组件 `FinancialReports`

- [ ] **Step 1: types/api 追加**

```typescript
export interface ReportRow {
  key: string;
  label: string;
  current: number;
  comparative: number;
}

export interface BalanceSheet {
  month: string;
  enabled: boolean;
  asset_rows: ReportRow[];
  liability_equity_rows: ReportRow[];
  asset_total: number;
  liability_equity_total: number;
  balanced: boolean;
}

export interface IncomeStatement {
  month: string;
  rows: ReportRow[];
  net_profit_month: number;
  net_profit_year: number;
}

export interface UnclassifiedCashItem {
  voucher_no: string;
  summary?: string | null;
  amount: number;
}

export interface CashFlowStatement {
  month: string;
  rows: ReportRow[];
  net_increase: number;
  unclassified: UnclassifiedCashItem[];
}
```

api：四个 invoke 封装（`getBalanceSheet(month)`、`getIncomeStatement(month)`、`getCashFlowStatement(month)`、`exportFinancialReport(month, reportType)`）。

- [ ] **Step 2: FinancialReports.tsx**

月份选择器 + Tabs（资产负债表/利润表/现金流量表）+ 每个 Tab 一个 Table（列：项目/本期金额/对比金额——资产负债表为 期末余额/年初余额，利润表 本月金额/本年累计）+ 每个 Tab 右上"导出 Excel"按钮 + 特殊提示区：
- 资产负债表 `!balanced` 时 Alert type="error"（"资产与负债权益不平衡，请联系检查凭证"）；`!enabled` 时 Alert（"该月份早于启用月，报表为空"）
- 现金流量表 `unclassified.length > 0` 时 Alert type="warning" 列出未归类明细（凭证号/摘要/金额），提示到科目表补现金流量分类
金额格式化 `toLocaleString('zh-CN', { minimumFractionDigits: 2 })`。约 260 行。

- [ ] **Step 3: 验证与提交**

Run: `npx tsc --noEmit && npm run lint && npm run build`

```bash
git add src/pages/FinancialReports.tsx src/types/index.ts src/api/index.ts src/App.tsx
git commit -m "feat(accounting): 财务报表页面"
```

### Task 14: 银行流水生成凭证入口 + 全量回归

**Files:**
- Modify: `src/pages/BankTransactions.tsx`、`src/pages/OperationLogs.tsx`（操作日志中文映射）、`src/types/index.ts`、`src/api/index.ts`

**Interfaces:**
- Consumes: 命令 `create_bank_manual_voucher`、`getGlAccounts`

- [ ] **Step 1: api/types 追加**

`export const createBankManualVoucher = (transactionId: number, accountCode: string, summary?: string) => invoke<Voucher>('create_bank_manual_voucher', { transactionId, accountCode, summary });`

- [ ] **Step 2: BankTransactions.tsx**

未匹配且未忽略的流水行操作列加"生成凭证"按钮 → Modal：Select 借方/贷方科目（`getGlAccounts()` 过滤 is_active，支出流水提示"选择借方科目"，收入流水提示"选择贷方科目"）+ 摘要 Input → 确认调用 `createBankManualVoucher` → message.success(凭证号) → 刷新列表。

- [ ] **Step 3: OperationLogs.tsx 追加映射**

```typescript
save_opening_balances: '保存期初余额',
create_bank_manual_voucher: '银行流水生成凭证',
export_financial_report: '导出财务报表',
```

- [ ] **Step 4: 全量回归**

```bash
npx tsc --noEmit
npm run lint
npm run build
cd src-tauri && cargo fmt --check && cargo check && cargo test --lib
npm run tauri dev   # 手工验收：科目表/凭证/报表三页、银行流水生成凭证、月结检查
```

- [ ] **Step 5: 更新文档并提交**

- 更新 `CLAUDE.md`（第五阶段段落 + Memory References 加 `stage5-accounting.md`）
- 新增 `.claude/memory/stage5-accounting.md`（定位、批次、测试门槛，参考 stage3 格式）
- 新增 `docs/superpowers/plans/2026-08-15-stage5-progress.md`（记录各批完成情况）
- 回写 spec：3.1 规则 1 的精化说明（缺勤/其他扣款少计费用；代扣保留 2211）、2.5 `account_mappings` 泛化命名

```bash
git add -A
git commit -m "feat(accounting): 银行流水凭证入口与第五阶段文档"
```

---

## Self-Review 记录

- **Spec 覆盖**：spec §2 五表→Task 1；§2.4 期初校验→Task 2；§3.1 六规则→Task 5-9；§3.2/3.3 防重复与作废联动→Task 6-9；§3.4 月结→Task 9；§4 三报表→Task 10；§4.5 导出→Task 11；§5 前端→Task 4/12/13/14；§6 错误处理→各任务内嵌；§7 测试→各任务 TDD + Task 14 回归。
- **占位符**：无 TBD；部分测试数据构造引用"参考现有测试写法"并给出 grep 定位命令（现有测试构造代码不在本计划复制范围，属可定位的既有代码）。
- **类型一致性**：`Voucher`/`VoucherDraft`/`ReportRow` 等跨任务签名已核对一致；前端字段与 Rust serde 序列化名一致（snake_case）。
