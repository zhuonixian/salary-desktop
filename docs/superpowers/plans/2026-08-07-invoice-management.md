# 发票管理模块 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增独立"发票管理"菜单，调用百度 OCR `vat_invoice` 接口识别增值税发票，按"发票代码+号码"唯一索引硬拦截重复报销，支持多维度归类与 Excel 导出。

**Architecture:** 后端新建 `invoice.rs` 业务模块（OCR 调用 + 解析 + 查重 + 图片复制 + CRUD），复用 `ocr.rs` 的 `get_baidu_access_token`（升级为 `pub(crate)`）。前端新增 `Invoices.tsx` 页面 + 菜单 + 路由，复用现有 Ant Design 风格。数据库新增 `invoices` / `invoice_expense_types` 两张表。与工资计算完全解耦。

**Tech Stack:** Rust（Tauri 2 / rusqlite 0.31 / reqwest 0.12 blocking / serde / chrono / base64 0.22）+ React 18 + TypeScript + Ant Design 5 + Vite + rust_xlsxwriter 0.79。

**Spec:** `docs/superpowers/specs/2026-08-07-invoice-management-design.md`

## Global Constraints

- 所有 Rust 命令签名风格与现有 `commands.rs` 一致：`fn xxx(args..., state: tauri::State<'_, Mutex<Connection>>) -> Result<T, AppError>`。
- 数据库连接从 `state.lock()` 获取，错误用 `AppError::General(e.to_string())` 包装。
- 时间戳统一用 `Utc::now().to_rfc3339()`。
- 前端字段映射：后端 `snake_case` ↔ 前端 `camelCase`，沿用 `src/api/index.ts` 现有 normalize 风格。
- 百度 OCR token 复用 `ocr.rs::get_baidu_access_token`，禁止重复实现缓存。
- `image` 不允许作为变量名（与 `image` crate 同名易混淆；本项目不引入该 crate）。
- 不能跳过 hooks，不能 `--no-verify`。
- 测试不依赖真实百度 API；用样例 JSON 测解析层。

## File Structure

**新建：**
- `src-tauri/src/invoice.rs` — 发票业务核心（OCR 调用、字段映射、查重、归类、图片复制）
- `src/pages/Invoices.tsx` — 发票管理页面（列表 + 筛选 + 上传 Modal + 编辑 Modal + 导出 + 费用类型 Drawer）

**修改：**
- `src-tauri/src/ocr.rs` — `get_baidu_access_token` 改 `pub(crate)`（行 130）
- `src-tauri/src/models.rs` — 新增 4 个结构体
- `src-tauri/src/db.rs` — `create_tables` 与 `insert_default_data` 增加发票相关；新增 10 个 pub 函数
- `src-tauri/src/commands.rs` — 新增 9 个 `#[tauri::command]`
- `src-tauri/src/lib.rs` — `mod invoice;` + 注册 9 个命令到 `generate_handler!`
- `src-tauri/src/excel.rs` — 新增 `export_invoice_list`
- `src/api/index.ts` — 新增 9 个 API 函数 + 后端字段映射
- `src/types/index.ts` — 新增 5 个接口
- `src/App.tsx` — 菜单 +1 项、路由 +1 条
- `database/schema.sql` — 同步发票表 schema（文档同步）

## 任务清单

- Task 1: 数据库 schema 与默认数据
- Task 2: models.rs 新增结构体
- Task 3: db.rs 发票 CRUD 函数
- Task 4: ocr.rs 改造 + invoice.rs OCR 解析层
- Task 5: invoice.rs 业务层（save / update / delete / 图片复制）
- Task 6: commands.rs 与 lib.rs 注册命令
- Task 7: excel.rs 导出发票清单
- Task 8: 前端类型与 API 封装
- Task 9: 前端 Invoices.tsx 主页面
- Task 10: App.tsx 菜单注册与端到端验收

---

### Task 1: 数据库 schema 与默认数据

**Files:**
- Modify: `src-tauri/src/db.rs:28-216`（`create_tables` 与 `insert_default_data`）
- Modify: `database/schema.sql`（同步 schema 文档）

**Interfaces:**
- Produces: 表 `invoices` / `invoice_expense_types`，后续 Task 3 的 CRUD 函数依赖此 schema。

- [ ] **Step 1: 在 `create_tables` 函数末尾追加两张表**

修改 `src-tauri/src/db.rs:28`，在 `create_tables` 函数中 `punch_card_batches` 表之后（`execute_batch` 字符串内）追加：

```rust
        CREATE TABLE IF NOT EXISTS invoice_expense_types (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            code TEXT UNIQUE NOT NULL,
            name TEXT NOT NULL,
            sort_order INTEGER DEFAULT 0,
            enabled INTEGER DEFAULT 1,
            remark TEXT
        );

        CREATE TABLE IF NOT EXISTS invoices (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            invoice_code TEXT,
            invoice_number TEXT,
            invoice_type TEXT,
            issue_date TEXT,
            check_code TEXT,
            amount REAL DEFAULT 0,
            tax_amount REAL DEFAULT 0,
            total_amount REAL DEFAULT 0,
            seller_name TEXT,
            seller_tax_id TEXT,
            buyer_name TEXT,
            buyer_tax_id TEXT,
            expense_type_code TEXT,
            employee_id INTEGER,
            belong_month TEXT,
            status TEXT DEFAULT 'normal',
            remark TEXT,
            image_path TEXT,
            raw_ocr_json TEXT,
            created_at TEXT,
            updated_at TEXT,
            FOREIGN KEY (employee_id) REFERENCES employees(id) ON DELETE SET NULL,
            FOREIGN KEY (expense_type_code) REFERENCES invoice_expense_types(code) ON DELETE SET NULL
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_invoices_code_number
            ON invoices(invoice_code, invoice_number);
        CREATE INDEX IF NOT EXISTS idx_invoices_employee ON invoices(employee_id);
        CREATE INDEX IF NOT EXISTS idx_invoices_month ON invoices(belong_month);
        CREATE INDEX IF NOT EXISTS idx_invoices_expense_type ON invoices(expense_type_code);
```

- [ ] **Step 2: 在 `insert_default_data` 函数末尾追加默认费用类型**

修改 `src-tauri/src/db.rs:160`，在 `insert_default_data` 函数末尾（个税默认数据后）追加：

```rust
    let expense_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM invoice_expense_types",
        [],
        |row| row.get(0),
    )?;

    if expense_count == 0 {
        let default_expense_types = vec![
            ("office",        "办公费",   1),
            ("travel",        "差旅费",   2),
            ("meal",          "餐饮费",   3),
            ("transport",     "交通费",   4),
            ("accommodation", "住宿费",   5),
            ("communication", "通讯费",   6),
            ("other",         "其他",     99),
        ];

        for (code, name, sort_order) in &default_expense_types {
            conn.execute(
                "INSERT INTO invoice_expense_types (code, name, sort_order) VALUES (?1, ?2, ?3)",
                params![code, name, sort_order],
            )?;
        }
    }
```

- [ ] **Step 3: 同步 schema.sql 文档**

在 `database/schema.sql` 末尾追加与 Step 1 相同的两张表 SQL 与索引。

- [ ] **Step 4: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 编译通过，无错误。

- [ ] **Step 5: 启动应用验证表创建**

Run: `cd src-tauri && cargo build` 然后启动应用一次（让 `init_db` 跑一遍），关闭后用 sqlite3 检查：
```bash
sqlite3 ~/.local/share/salary-desktop/salary.db ".tables" | grep -E "invoices|invoice_expense_types"
```
Expected: 输出包含 `invoices` 和 `invoice_expense_types`。

```bash
sqlite3 ~/.local/share/salary-desktop/salary.db "SELECT code, name FROM invoice_expense_types ORDER BY sort_order;"
```
Expected: 输出 7 条预设类型。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/db.rs database/schema.sql
git commit -m "feat(invoice): add invoices and invoice_expense_types tables"
```

---

### Task 2: models.rs 新增结构体

**Files:**
- Modify: `src-tauri/src/models.rs`（文件末尾追加）

**Interfaces:**
- Produces: `Invoice`、`InvoiceInput`、`InvoiceOcrPreview`、`InvoiceQuery`、`InvoiceExpenseType`、`InvoiceExpenseTypeInput`。后续 Task 3-6 全部依赖。

- [ ] **Step 1: 在 `src-tauri/src/models.rs` 末尾追加结构体**

```rust
// ==================== Invoice ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceExpenseType {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub sort_order: i32,
    pub enabled: i32,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceExpenseTypeInput {
    pub id: Option<i64>,
    pub code: Option<String>,
    pub name: Option<String>,
    pub sort_order: Option<i32>,
    pub enabled: Option<i32>,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    pub id: i64,
    pub invoice_code: Option<String>,
    pub invoice_number: Option<String>,
    pub invoice_type: Option<String>,
    pub issue_date: Option<String>,
    pub check_code: Option<String>,
    pub amount: f64,
    pub tax_amount: f64,
    pub total_amount: f64,
    pub seller_name: Option<String>,
    pub seller_tax_id: Option<String>,
    pub buyer_name: Option<String>,
    pub buyer_tax_id: Option<String>,
    pub expense_type_code: Option<String>,
    pub employee_id: Option<i64>,
    pub belong_month: Option<String>,
    pub status: Option<String>,
    pub remark: Option<String>,
    pub image_path: Option<String>,
    pub raw_ocr_json: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceInput {
    pub invoice_code: Option<String>,
    pub invoice_number: Option<String>,
    pub invoice_type: Option<String>,
    pub issue_date: Option<String>,
    pub check_code: Option<String>,
    pub amount: Option<f64>,
    pub tax_amount: Option<f64>,
    pub total_amount: Option<f64>,
    pub seller_name: Option<String>,
    pub seller_tax_id: Option<String>,
    pub buyer_name: Option<String>,
    pub buyer_tax_id: Option<String>,
    pub expense_type_code: Option<String>,
    pub employee_id: Option<i64>,
    pub belong_month: Option<String>,
    pub remark: Option<String>,
    pub image_path: Option<String>,
    pub raw_ocr_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceOcrPreview {
    pub invoice_code: Option<String>,
    pub invoice_number: Option<String>,
    pub invoice_type: Option<String>,
    pub issue_date: Option<String>,
    pub check_code: Option<String>,
    pub amount: f64,
    pub tax_amount: f64,
    pub total_amount: f64,
    pub seller_name: Option<String>,
    pub seller_tax_id: Option<String>,
    pub buyer_name: Option<String>,
    pub buyer_tax_id: Option<String>,
    pub raw_ocr_json: String,
    pub warnings: Vec<String>,
    pub is_duplicate: bool,
    pub duplicate_invoice_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InvoiceQuery {
    pub belong_month: Option<String>,
    pub employee_id: Option<i64>,
    pub expense_type_code: Option<String>,
    pub invoice_type: Option<String>,
    pub keyword: Option<String>,
    pub status: Option<String>,
}
```

- [ ] **Step 2: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 编译通过（可能有 unused warning，无妨）。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/models.rs
git commit -m "feat(invoice): add invoice domain models"
```

---

### Task 3: db.rs 发票 CRUD 函数

**Files:**
- Modify: `src-tauri/src/db.rs`（末尾追加新章节）
- Modify: `src-tauri/src/db.rs`（在 `#[cfg(test)]` 模块中新增测试，若不存在则新建）

**Interfaces:**
- Consumes: Task 1 的表 schema、Task 2 的结构体。
- Produces: `get_invoice_expense_types`、`insert_invoice_expense_type`、`update_invoice_expense_type`、`delete_invoice_expense_type`、`count_invoices_by_expense_type`、`find_invoice_by_code_number`、`get_invoice`、`insert_invoice`、`update_invoice`、`soft_delete_invoice`、`query_invoices`。Task 5、6 依赖。

- [ ] **Step 1: 在 `src-tauri/src/db.rs` 末尾追加 CRUD 章节**

```rust
// ==================== Invoice Expense Types ====================

pub fn get_invoice_expense_types(conn: &Connection) -> AppResult<Vec<InvoiceExpenseType>> {
    let mut stmt = conn.prepare(
        "SELECT id, code, name, sort_order, enabled, remark FROM invoice_expense_types ORDER BY sort_order, id"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(InvoiceExpenseType {
            id: row.get(0)?,
            code: row.get(1)?,
            name: row.get(2)?,
            sort_order: row.get(3)?,
            enabled: row.get(4)?,
            remark: row.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn insert_invoice_expense_type(conn: &Connection, data: &InvoiceExpenseTypeInput) -> AppResult<InvoiceExpenseType> {
    let code = data.code.as_ref().ok_or_else(|| AppError::InvalidParam("code 必填".into()))?;
    let name = data.name.as_ref().ok_or_else(|| AppError::InvalidParam("name 必填".into()))?;
    let sort_order = data.sort_order.unwrap_or(99);
    let enabled = data.enabled.unwrap_or(1);
    conn.execute(
        "INSERT INTO invoice_expense_types (code, name, sort_order, enabled, remark) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![code, name, sort_order, enabled, data.remark],
    )?;
    let id = conn.last_insert_rowid();
    Ok(InvoiceExpenseType {
        id, code: code.clone(), name: name.clone(), sort_order, enabled, remark: data.remark.clone(),
    })
}

pub fn update_invoice_expense_type(conn: &Connection, id: i64, data: &InvoiceExpenseTypeInput) -> AppResult<InvoiceExpenseType> {
    let existing = conn.query_row(
        "SELECT id, code, name, sort_order, enabled, remark FROM invoice_expense_types WHERE id = ?1",
        params![id],
        |row| Ok(InvoiceExpenseType {
            id: row.get(0)?, code: row.get(1)?, name: row.get(2)?,
            sort_order: row.get(3)?, enabled: row.get(4)?, remark: row.get(5)?,
        }),
    ).map_err(|e| AppError::NotFound(format!("费用类型ID={id}未找到: {e}")))?;

    // 不允许修改 code（避免破坏外键关联）
    let name = data.name.as_ref().unwrap_or(&existing.name);
    let sort_order = data.sort_order.unwrap_or(existing.sort_order);
    let enabled = data.enabled.unwrap_or(existing.enabled);
    let remark = data.remark.as_ref().or(existing.remark.as_ref());

    conn.execute(
        "UPDATE invoice_expense_types SET name=?1, sort_order=?2, enabled=?3, remark=?4 WHERE id=?5",
        params![name, sort_order, enabled, remark, id],
    )?;

    Ok(InvoiceExpenseType {
        id, code: existing.code, name: name.clone(), sort_order, enabled, remark: remark.cloned(),
    })
}

pub fn delete_invoice_expense_type(conn: &Connection, id: i64) -> AppResult<bool> {
    // 不允许删除"other"
    let code: String = conn.query_row(
        "SELECT code FROM invoice_expense_types WHERE id = ?1",
        params![id],
        |row| row.get(0),
    ).map_err(|e| AppError::NotFound(format!("费用类型ID={id}未找到: {e}")))?;

    if code == "other" {
        return Err(AppError::InvalidParam("「其他」类型不允许删除".into()));
    }

    let in_use = count_invoices_by_expense_type(conn, &code)?;
    if in_use > 0 {
        return Err(AppError::InvalidParam(format!(
            "费用类型「{code}」已被 {in_use} 张发票引用，请改用禁用"
        )));
    }

    let deleted = conn.execute("DELETE FROM invoice_expense_types WHERE id=?1", params![id])?;
    Ok(deleted > 0)
}

pub fn count_invoices_by_expense_type(conn: &Connection, code: &str) -> AppResult<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM invoices WHERE expense_type_code = ?1 AND status != 'void'",
        params![code],
        |row| row.get(0),
    )?;
    Ok(count)
}

// ==================== Invoices ====================

const INVOICE_SELECT_FIELDS: &str = "id, invoice_code, invoice_number, invoice_type, issue_date, check_code, amount, tax_amount, total_amount, seller_name, seller_tax_id, buyer_name, buyer_tax_id, expense_type_code, employee_id, belong_month, status, remark, image_path, raw_ocr_json, created_at, updated_at";

fn row_to_invoice(row: &rusqlite::Row<'_>) -> rusqlite::Result<Invoice> {
    Ok(Invoice {
        id: row.get(0)?,
        invoice_code: row.get(1)?,
        invoice_number: row.get(2)?,
        invoice_type: row.get(3)?,
        issue_date: row.get(4)?,
        check_code: row.get(5)?,
        amount: row.get(6)?,
        tax_amount: row.get(7)?,
        total_amount: row.get(8)?,
        seller_name: row.get(9)?,
        seller_tax_id: row.get(10)?,
        buyer_name: row.get(11)?,
        buyer_tax_id: row.get(12)?,
        expense_type_code: row.get(13)?,
        employee_id: row.get(14)?,
        belong_month: row.get(15)?,
        status: row.get(16)?,
        remark: row.get(17)?,
        image_path: row.get(18)?,
        raw_ocr_json: row.get(19)?,
        created_at: row.get(20)?,
        updated_at: row.get(21)?,
    })
}

pub fn find_invoice_by_code_number(conn: &Connection, code: &str, number: &str) -> AppResult<Option<Invoice>> {
    let sql = format!("SELECT {INVOICE_SELECT_FIELDS} FROM invoices WHERE invoice_code = ?1 AND invoice_number = ?2 AND status != 'void' LIMIT 1");
    let result = conn.query_row(&sql, params![code, number], row_to_invoice);
    match result {
        Ok(inv) => Ok(Some(inv)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::from(e)),
    }
}

pub fn get_invoice(conn: &Connection, id: i64) -> AppResult<Invoice> {
    let sql = format!("SELECT {INVOICE_SELECT_FIELDS} FROM invoices WHERE id = ?1");
    conn.query_row(&sql, params![id], row_to_invoice)
        .map_err(|e| AppError::NotFound(format!("发票ID={id}未找到: {e}")))
}

pub fn insert_invoice(conn: &Connection, data: &InvoiceInput, image_path: &str) -> AppResult<Invoice> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO invoices (invoice_code, invoice_number, invoice_type, issue_date, check_code, amount, tax_amount, total_amount, seller_name, seller_tax_id, buyer_name, buyer_tax_id, expense_type_code, employee_id, belong_month, status, remark, image_path, raw_ocr_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 'normal', ?16, ?17, ?18, ?19, ?20)",
        params![
            data.invoice_code, data.invoice_number, data.invoice_type, data.issue_date,
            data.check_code, data.amount.unwrap_or(0.0), data.tax_amount.unwrap_or(0.0),
            data.total_amount.unwrap_or(0.0), data.seller_name, data.seller_tax_id,
            data.buyer_name, data.buyer_tax_id, data.expense_type_code, data.employee_id,
            data.belong_month, data.remark, image_path, data.raw_ocr_json, now, now
        ],
    )?;
    get_invoice(conn, conn.last_insert_rowid())
}

pub fn update_invoice(conn: &Connection, id: i64, data: &InvoiceInput, new_image_path: Option<&str>) -> AppResult<bool> {
    let now = Utc::now().to_rfc3339();
    let existing = get_invoice(conn, id)?;
    let image_path = new_image_path.unwrap_or(existing.image_path.as_deref().unwrap_or(""));

    // 若改了 code/number，需校验不撞其他记录
    let new_code = data.invoice_code.as_ref().or(existing.invoice_code.as_ref());
    let new_number = data.invoice_number.as_ref().or(existing.invoice_number.as_ref());
    if let (Some(c), Some(n)) = (new_code, new_number) {
        if let Some(other) = find_invoice_by_code_number(conn, c, n)? {
            if other.id != id {
                return Err(AppError::General(format!(
                    "发票代码{c}+号码{n}已被记录ID={}占用", other.id
                )));
            }
        }
    }

    let updated = conn.execute(
        "UPDATE invoices SET invoice_code=?1, invoice_number=?2, invoice_type=?3, issue_date=?4, check_code=?5, amount=?6, tax_amount=?7, total_amount=?8, seller_name=?9, seller_tax_id=?10, buyer_name=?11, buyer_tax_id=?12, expense_type_code=?13, employee_id=?14, belong_month=?15, remark=?16, image_path=?17, raw_ocr_json=?18, updated_at=?19 WHERE id=?20",
        params![
            data.invoice_code.as_ref().or(existing.invoice_code.as_ref()),
            data.invoice_number.as_ref().or(existing.invoice_number.as_ref()),
            data.invoice_type.as_ref().or(existing.invoice_type.as_ref()),
            data.issue_date.as_ref().or(existing.issue_date.as_ref()),
            data.check_code.as_ref().or(existing.check_code.as_ref()),
            data.amount.unwrap_or(existing.amount),
            data.tax_amount.unwrap_or(existing.tax_amount),
            data.total_amount.unwrap_or(existing.total_amount),
            data.seller_name.as_ref().or(existing.seller_name.as_ref()),
            data.seller_tax_id.as_ref().or(existing.seller_tax_id.as_ref()),
            data.buyer_name.as_ref().or(existing.buyer_name.as_ref()),
            data.buyer_tax_id.as_ref().or(existing.buyer_tax_id.as_ref()),
            data.expense_type_code.as_ref().or(existing.expense_type_code.as_ref()),
            data.employee_id.or(existing.employee_id),
            data.belong_month.as_ref().or(existing.belong_month.as_ref()),
            data.remark.as_ref().or(existing.remark.as_ref()),
            image_path,
            data.raw_ocr_json.as_ref().or(existing.raw_ocr_json.as_ref()),
            now, id
        ],
    )?;
    Ok(updated > 0)
}

pub fn soft_delete_invoice(conn: &Connection, id: i64) -> AppResult<bool> {
    let now = Utc::now().to_rfc3339();
    let updated = conn.execute(
        "UPDATE invoices SET status='void', updated_at=?1 WHERE id=?2 AND status != 'void'",
        params![now, id],
    )?;
    Ok(updated > 0)
}

pub fn query_invoices(conn: &Connection, q: &InvoiceQuery) -> AppResult<Vec<Invoice>> {
    let mut where_clauses: Vec<String> = vec!["status != 'void'".to_string()];
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(m) = &q.belong_month {
        where_clauses.push(format!("belong_month = ?{idx}"));
        params_vec.push(Box::new(m.clone()));
        idx += 1;
    }
    if let Some(eid) = q.employee_id {
        where_clauses.push(format!("employee_id = ?{idx}"));
        params_vec.push(Box::new(eid));
        idx += 1;
    }
    if let Some(code) = &q.expense_type_code {
        where_clauses.push(format!("expense_type_code = ?{idx}"));
        params_vec.push(Box::new(code.clone()));
        idx += 1;
    }
    if let Some(t) = &q.invoice_type {
        where_clauses.push(format!("invoice_type = ?{idx}"));
        params_vec.push(Box::new(t.clone()));
        idx += 1;
    }
    if let Some(s) = &q.status {
        where_clauses[0] = format!("status = ?{idx}");
        params_vec.push(Box::new(s.clone()));
        idx += 1;
    }
    if let Some(kw) = &q.keyword {
        let pat = format!("%{kw}%");
        where_clauses.push(format!("(seller_name LIKE ?{idx} OR buyer_name LIKE ?{idx} OR remark LIKE ?{idx})"));
        params_vec.push(Box::new(pat));
        idx += 1;
    }

    let sql = format!(
        "SELECT {INVOICE_SELECT_FIELDS} FROM invoices WHERE {} ORDER BY issue_date DESC, id DESC",
        where_clauses.join(" AND ")
    );

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), row_to_invoice)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}
```

- [ ] **Step 2: 在 `src-tauri/src/db.rs` 文件末尾添加测试模块**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("
            CREATE TABLE employees (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT);
            CREATE TABLE invoice_expense_types (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                code TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                sort_order INTEGER DEFAULT 0,
                enabled INTEGER DEFAULT 1,
                remark TEXT
            );
            CREATE TABLE invoices (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                invoice_code TEXT, invoice_number TEXT, invoice_type TEXT,
                issue_date TEXT, check_code TEXT,
                amount REAL DEFAULT 0, tax_amount REAL DEFAULT 0, total_amount REAL DEFAULT 0,
                seller_name TEXT, seller_tax_id TEXT, buyer_name TEXT, buyer_tax_id TEXT,
                expense_type_code TEXT, employee_id INTEGER, belong_month TEXT,
                status TEXT DEFAULT 'normal', remark TEXT,
                image_path TEXT, raw_ocr_json TEXT,
                created_at TEXT, updated_at TEXT
            );
            CREATE UNIQUE INDEX idx_invoices_code_number ON invoices(invoice_code, invoice_number);
            INSERT INTO invoice_expense_types (code, name, sort_order) VALUES
                ('office', '办公费', 1), ('other', '其他', 99);
            INSERT INTO employees (id, name) VALUES (1, '张三');
        ").unwrap();
        conn
    }

    fn sample_input(code: &str, num: &str) -> InvoiceInput {
        InvoiceInput {
            invoice_code: Some(code.into()),
            invoice_number: Some(num.into()),
            invoice_type: Some("普通发票".into()),
            issue_date: Some("2026-08-01".into()),
            check_code: None,
            amount: Some(100.0), tax_amount: Some(6.0), total_amount: Some(106.0),
            seller_name: Some("测试销售方".into()), seller_tax_id: Some("91XXXX".into()),
            buyer_name: Some("测试购买方".into()), buyer_tax_id: Some("92XXXX".into()),
            expense_type_code: Some("office".into()),
            employee_id: Some(1),
            belong_month: Some("2026-08".into()),
            remark: None, image_path: Some("/tmp/x.pdf".into()), raw_ocr_json: Some("{}".into()),
        }
    }

    #[test]
    fn test_insert_and_find_invoice() {
        let conn = setup_db();
        let inv = insert_invoice(&conn, &sample_input("12345", "67890"), "/stored/x.pdf").unwrap();
        assert_eq!(inv.invoice_code.as_deref(), Some("12345"));
        let found = find_invoice_by_code_number(&conn, "12345", "67890").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, inv.id);
    }

    #[test]
    fn test_find_nonexistent_returns_none() {
        let conn = setup_db();
        let found = find_invoice_by_code_number(&conn, "X", "Y").unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn test_unique_index_blocks_duplicate() {
        let conn = setup_db();
        insert_invoice(&conn, &sample_input("111", "222"), "/a.pdf").unwrap();
        let result = insert_invoice(&conn, &sample_input("111", "222"), "/b.pdf");
        assert!(result.is_err(), "重复插入应被唯一索引拦截");
    }

    #[test]
    fn test_soft_delete_hides_record() {
        let conn = setup_db();
        let inv = insert_invoice(&conn, &sample_input("333", "444"), "/c.pdf").unwrap();
        assert!(soft_delete_invoice(&conn, inv.id).unwrap());
        // find 应该返回 None（因为 status='void' 被过滤）
        assert!(find_invoice_by_code_number(&conn, "333", "444").unwrap().is_none());
        // query_invoices 默认也应过滤
        let list = query_invoices(&conn, &InvoiceQuery::default()).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_query_invoices_filters() {
        let conn = setup_db();
        let mut a = sample_input("555", "001"); a.belong_month = Some("2026-07".into());
        let mut b = sample_input("555", "002"); b.belong_month = Some("2026-08".into());
        insert_invoice(&conn, &a, "/a.pdf").unwrap();
        insert_invoice(&conn, &b, "/b.pdf").unwrap();

        let july = query_invoices(&conn, &InvoiceQuery { belong_month: Some("2026-07".into()), ..Default::default() }).unwrap();
        assert_eq!(july.len(), 1);
        assert_eq!(july[0].invoice_number.as_deref(), Some("001"));
    }

    #[test]
    fn test_delete_other_expense_type_blocked() {
        let conn = setup_db();
        let other_id: i64 = conn.query_row(
            "SELECT id FROM invoice_expense_types WHERE code='other'", [], |r| r.get(0)
        ).unwrap();
        let result = delete_invoice_expense_type(&conn, other_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_used_expense_type_blocked() {
        let conn = setup_db();
        insert_invoice(&conn, &sample_input("777", "888"), "/d.pdf").unwrap();
        let office_id: i64 = conn.query_row(
            "SELECT id FROM invoice_expense_types WHERE code='office'", [], |r| r.get(0)
        ).unwrap();
        let result = delete_invoice_expense_type(&conn, office_id);
        assert!(result.is_err(), "被引用的费用类型不允许删除");
    }
}
```

- [ ] **Step 3: 运行测试**

Run: `cd src-tauri && cargo test --lib db::tests`
Expected: 7 个测试全部 PASS。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/db.rs
git commit -m "feat(invoice): add invoice CRUD functions with tests"
```

---

### Task 4: ocr.rs 改造 + invoice.rs OCR 解析层

**Files:**
- Modify: `src-tauri/src/ocr.rs:130`（`get_baidu_access_token` 改 `pub(crate)`）
- Create: `src-tauri/src/invoice.rs`

**Interfaces:**
- Consumes: `ocr::get_baidu_access_token(conn: &Connection) -> AppResult<String>`、`db::find_invoice_by_code_number`。
- Produces: `invoice::ocr_invoice(image_path, conn) -> AppResult<InvoiceOcrPreview>`、内部纯函数 `map_baidu_response` 和 `parse_amount`。Task 5、6 依赖 `ocr_invoice`。

- [ ] **Step 1: 把 `ocr.rs` 中 `get_baidu_access_token` 改为 `pub(crate)`**

修改 `src-tauri/src/ocr.rs:130`：

```rust
pub(crate) fn get_baidu_access_token(conn: &Connection) -> AppResult<String> {
```

- [ ] **Step 2: 创建 `src-tauri/src/invoice.rs`**

```rust
use rusqlite::Connection;
use serde::Deserialize;

use crate::db;
use crate::errors::{AppError, AppResult};
use crate::models::*;
use crate::ocr;

const BAIDU_VAT_INVOICE_URL: &str =
    "https://aip.baidubce.com/rest/2.0/ocr/v1/vat_invoice";

// ==================== Baidu Response Types ====================

#[derive(Debug, Deserialize)]
pub(crate) struct BaiduVatInvoiceResponse {
    #[serde(default)]
    words_result: serde_json::Value,
    error_code: Option<i32>,
    error_msg: Option<String>,
    // vat_invoice 顶层可能有 InvoiceTypeLog/TotalAmount/etc.
    // 我们从 words_result 与顶层字段双路取值
    #[serde(flatten)]
    extra: serde_json::Value,
}

// ==================== OCR Entry ====================

pub fn ocr_invoice(image_path: &str, conn: &Connection) -> AppResult<InvoiceOcrPreview> {
    let image_data = std::fs::read(image_path)
        .map_err(|e| AppError::Io(e))?;
    let image_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD, &image_data
    );

    let token = ocr::get_baidu_access_token(conn)?;
    let url = format!("{BAIDU_VAT_INVOICE_URL}?access_token={token}");

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[("image", image_b64.as_str())])
        .send()
        .map_err(|e| AppError::Network(format!("百度发票OCR请求失败: {e}")))?;

    let raw_text = response.text()
        .map_err(|e| AppError::Network(format!("读取响应失败: {e}")))?;

    let parsed: BaiduVatInvoiceResponse = serde_json::from_str(&raw_text)
        .map_err(|e| AppError::Ocr(format!("百度发票OCR响应解析失败: {e}")))?;

    if let Some(code) = parsed.error_code {
        let msg = parsed.error_msg.unwrap_or_default();
        return Err(AppError::Ocr(translate_baidu_error(code, &msg)));
    }

    let mut preview = map_baidu_response(&parsed, &raw_text);

    // 查重
    if let (Some(c), Some(n)) = (preview.invoice_code.as_ref(), preview.invoice_number.as_ref()) {
        if let Some(existing) = db::find_invoice_by_code_number(conn, c, n)? {
            preview.is_duplicate = true;
            preview.duplicate_invoice_id = Some(existing.id);
            preview.warnings.push(format!(
                "该发票已存在于系统（ID={}，录入时间={}）",
                existing.id,
                existing.created_at.unwrap_or_default()
            ));
        }
    } else {
        preview.warnings.push("未能识别发票代码或号码，需手工补全".to_string());
    }

    Ok(preview)
}

// ==================== Pure Mapping Functions ====================

fn translate_baidu_error(code: i32, msg: &str) -> String {
    match code {
        18 => "百度OCR QPS超限，请稍后再试".to_string(),
        216201 => "图片不存在或格式错误".to_string(),
        216202 => "图片模糊，无法识别".to_string(),
        216678 => "发票类型不支持或图片非发票".to_string(),
        _ => format!("百度OCR错误({code}): {msg}"),
    }
}

fn map_baidu_response(resp: &BaiduVatInvoiceResponse, raw_text: &str) -> InvoiceOcrPreview {
    let words = &resp.words_result;
    let extra = &resp.extra;

    InvoiceOcrPreview {
        invoice_code: pick_str(words, extra, "InvoiceCode"),
        invoice_number: pick_str(words, extra, "InvoiceNum"),
        invoice_type: pick_str(words, extra, "InvoiceType")
            .or_else(|| pick_str(words, extra, "InvoiceTypeLog")),
        issue_date: pick_str(words, extra, "IssueDate"),
        check_code: pick_str(words, extra, "CheckCode"),
        amount: parse_amount(&pick_str(words, extra, "TotalAmount"))
            .unwrap_or(0.0),
        tax_amount: parse_amount(&pick_str(words, extra, "TotalTax"))
            .unwrap_or(0.0),
        total_amount: parse_amount(&pick_str(words, extra, "AmountInFiguers"))
            .unwrap_or(0.0),
        seller_name: pick_str(words, extra, "SellerName"),
        seller_tax_id: pick_str(words, extra, "SellerRegisterNum"),
        buyer_name: pick_str(words, extra, "PurchaserName"),
        buyer_tax_id: pick_str(words, extra, "PurchaserRegisterNum"),
        raw_ocr_json: raw_text.to_string(),
        warnings: Vec::new(),
        is_duplicate: false,
        duplicate_invoice_id: None,
    }
}

/// 从 words_result（对象，每个字段是 {word: "..."}）或 extra（顶层字段，直接字符串）取值
fn pick_str(words: &serde_json::Value, extra: &serde_json::Value, key: &str) -> Option<String> {
    if let Some(obj) = words.get(key).and_then(|v| v.as_object()) {
        if let Some(w) = obj.get("word").and_then(|v| v.as_str()) {
            return Some(w.trim().to_string()).filter(|s| !s.is_empty());
        }
    }
    extra.get(key).and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 解析金额：去千分位逗号、去 ¥/￥/$ 符号、去「元」、解析为 f64
fn parse_amount(s: &Option<String>) -> Option<f64> {
    let s = s.as_ref()?;
    let cleaned: String = s.chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    cleaned.parse::<f64>().ok()
}

// ==================== Unit Tests ====================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_response(json_value: serde_json::Value) -> BaiduVatInvoiceResponse {
        // 拆出 words_result 和 extra
        let words_result = json_value.get("words_result").cloned().unwrap_or(json!( {}));
        let extra = {
            let mut e = json_value.clone();
            if e.get("words_result").is_some() {
                e.as_object_mut().map(|o| o.remove("words_result"));
            }
            if e.get("error_code").is_some() {
                e.as_object_mut().map(|o| o.remove("error_code"));
            }
            if e.get("error_msg").is_some() {
                e.as_object_mut().map(|o| o.remove("error_msg"));
            }
            e
        };
        BaiduVatInvoiceResponse {
            words_result,
            error_code: json_value.get("error_code").and_then(|v| v.as_i64()).map(|v| v as i32),
            error_msg: json_value.get("error_msg").and_then(|v| v.as_str()).map(String::from),
            extra,
        }
    }

    #[test]
    fn test_map_full_response() {
        let resp_json = json!({
            "words_result": {
                "InvoiceCode": {"word": "044001800211"},
                "InvoiceNum": {"word": "12345678"},
                "InvoiceType": {"word": "增值税普通发票"},
                "IssueDate": {"word": "2026-08-01"},
                "TotalAmount": {"word": "100.00"},
                "TotalTax": {"word": "6.00"},
                "AmountInFiguers": {"word": "￥106.00"},
                "SellerName": {"word": "测试销售方"},
                "SellerRegisterNum": {"word": "91XXXX"},
                "PurchaserName": {"word": "测试购买方"},
                "PurchaserRegisterNum": {"word": "92XXXX"},
            }
        });
        let resp = make_response(resp_json);
        let preview = map_baidu_response(&resp, "");
        assert_eq!(preview.invoice_code.as_deref(), Some("044001800211"));
        assert_eq!(preview.invoice_number.as_deref(), Some("12345678"));
        assert_eq!(preview.invoice_type.as_deref(), Some("增值税普通发票"));
        assert!((preview.amount - 100.0).abs() < 1e-6);
        assert!((preview.tax_amount - 6.0).abs() < 1e-6);
        assert!((preview.total_amount - 106.0).abs() < 1e-6);
    }

    #[test]
    fn test_map_partial_response() {
        let resp_json = json!({
            "words_result": {
                "InvoiceCode": {"word": "044001800211"},
                "InvoiceNum": {"word": "12345678"},
            }
        });
        let resp = make_response(resp_json);
        let preview = map_baidu_response(&resp, "");
        assert!(preview.invoice_type.is_none());
        assert!((preview.amount - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_parse_amount() {
        assert_eq!(parse_amount(&Some("100.00".into())), Some(100.0));
        assert_eq!(parse_amount(&Some("￥1,234.56".into())), Some(1234.56));
        assert_eq!(parse_amount(&Some("1,234.56元".into())), Some(1234.56));
        assert_eq!(parse_amount(&Some("".into())), None);
        assert_eq!(parse_amount(&Some("abc".into())), None);
        assert_eq!(parse_amount(&None), None);
    }

    #[test]
    fn test_translate_baidu_error() {
        assert!(translate_baidu_error(18, "").contains("QPS"));
        assert!(translate_baidu_error(216201, "").contains("图片"));
        assert!(translate_baidu_error(999, "raw msg").contains("raw msg"));
    }
}
```

- [ ] **Step 3: 在 `lib.rs` 注册 `invoice` 模块**

修改 `src-tauri/src/lib.rs:1-7` 的模块声明，加入 `mod invoice;`：

```rust
mod commands;
mod db;
mod errors;
mod excel;
mod invoice;
mod models;
mod ocr;
mod salary;
```

- [ ] **Step 4: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 编译通过。

- [ ] **Step 5: 运行单元测试**

Run: `cd src-tauri && cargo test --lib invoice::tests`
Expected: 4 个测试 PASS。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/ocr.rs src-tauri/src/invoice.rs src-tauri/src/lib.rs
git commit -m "feat(invoice): add invoice OCR with Baidu vat_invoice endpoint"
```

---

### Task 5: invoice.rs 业务层（save / update / delete / 图片复制）

**Files:**
- Modify: `src-tauri/src/invoice.rs`（追加业务函数）

**Interfaces:**
- Consumes: Task 3 的 `db::insert_invoice` / `update_invoice` / `soft_delete_invoice` / `find_invoice_by_code_number`、Task 4 的 `ocr_invoice`。
- Produces: `invoice::save_invoice(input, conn, app_data_dir) -> AppResult<Invoice>`、`invoice::update_invoice(id, input, conn, app_data_dir)`、`invoice::delete_invoice(id, conn)`。Task 6 依赖。

- [ ] **Step 1: 在 `invoice.rs` 末尾追加业务函数（在 `#[cfg(test)]` 之前）**

```rust
// ==================== Business Layer ====================

pub fn save_invoice(
    input: &InvoiceInput,
    conn: &Connection,
    app_data_dir: &std::path::Path,
) -> AppResult<Invoice> {
    // 二次查重
    if let (Some(c), Some(n)) = (input.invoice_code.as_ref(), input.invoice_number.as_ref()) {
        if let Some(existing) = db::find_invoice_by_code_number(conn, c, n)? {
            return Err(AppError::General(format!(
                "发票已存在：代码{c} 号码{n}，记录ID={}",
                existing.id
            )));
        }
    } else {
        return Err(AppError::InvalidParam("发票代码和号码必填".into()));
    }

    // 复制原图到应用目录
    let target_path = match input.image_path.as_deref() {
        Some(src) if !src.is_empty() => {
            Some(copy_image_to_app_dir(src, input.belong_month.as_deref(), app_data_dir)?)
        }
        _ => None,
    };

    let invoice = db::insert_invoice(conn, input, target_path.as_deref().unwrap_or(""))?;

    db::log_operation(
        conn,
        "save_invoice",
        &format!(
            "录入发票：代码{} 号码{} 价税合计{:.2}",
            input.invoice_code.as_deref().unwrap_or(""),
            input.invoice_number.as_deref().unwrap_or(""),
            input.total_amount.unwrap_or(0.0)
        ),
        "system",
        None,
    )?;

    Ok(invoice)
}

pub fn update_invoice(
    id: i64,
    input: &InvoiceInput,
    conn: &Connection,
    app_data_dir: &std::path::Path,
) -> AppResult<bool> {
    let existing = db::get_invoice(conn, id)?;
    let new_image_path = if let Some(new_src) = input.image_path.as_deref() {
        if !new_src.is_empty() && new_src != existing.image_path.as_deref().unwrap_or("") {
            // 用户换图，复制新图
            let copied = copy_image_to_app_dir(
                new_src,
                input.belong_month.as_deref().or(existing.belong_month.as_deref()),
                app_data_dir,
            )?;
            Some(copied)
        } else {
            None
        }
    } else {
        None
    };

    let result = db::update_invoice(conn, id, input, new_image_path.as_deref())?;

    if result {
        db::log_operation(
            conn,
            "update_invoice",
            &format!("更新发票ID={id}"),
            "system",
            None,
        )?;
    }

    Ok(result)
}

pub fn delete_invoice(id: i64, conn: &Connection) -> AppResult<bool> {
    let result = db::soft_delete_invoice(conn, id)?;
    if result {
        db::log_operation(
            conn,
            "delete_invoice",
            &format!("删除发票ID={id}"),
            "system",
            None,
        )?;
    }
    Ok(result)
}

/// 复制源文件到 {app_data_dir}/invoices/{belong_month}/{timestamp}_{filename}
pub(crate) fn copy_image_to_app_dir(
    src: &str,
    belong_month: Option<&str>,
    app_data_dir: &std::path::Path,
) -> AppResult<String> {
    let src_path = std::path::Path::new(src);
    let filename = src_path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "invoice.bin".to_string());

    let month = belong_month.unwrap_or("unclassified");
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let target_name = format!("{timestamp}_{filename}");

    let target_dir = app_data_dir.join("invoices").join(month);
    std::fs::create_dir_all(&target_dir)?;

    let target_path = target_dir.join(target_name);
    std::fs::copy(src_path, &target_path)?;

    Ok(target_path.to_string_lossy().to_string())
}
```

- [ ] **Step 2: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 编译通过。

- [ ] **Step 3: 添加业务层集成测试到 `invoice.rs` 的 `#[cfg(test)]` 模块**

```rust
#[cfg(test)]
mod business_tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("
            CREATE TABLE employees (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT);
            CREATE TABLE invoice_expense_types (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                code TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                sort_order INTEGER DEFAULT 0,
                enabled INTEGER DEFAULT 1,
                remark TEXT
            );
            CREATE TABLE invoices (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                invoice_code TEXT, invoice_number TEXT, invoice_type TEXT,
                issue_date TEXT, check_code TEXT,
                amount REAL DEFAULT 0, tax_amount REAL DEFAULT 0, total_amount REAL DEFAULT 0,
                seller_name TEXT, seller_tax_id TEXT, buyer_name TEXT, buyer_tax_id TEXT,
                expense_type_code TEXT, employee_id INTEGER, belong_month TEXT,
                status TEXT DEFAULT 'normal', remark TEXT,
                image_path TEXT, raw_ocr_json TEXT,
                created_at TEXT, updated_at TEXT
            );
            CREATE UNIQUE INDEX idx_invoices_code_number ON invoices(invoice_code, invoice_number);
            CREATE TABLE operation_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                operation_type TEXT NOT NULL, description TEXT,
                operator TEXT, detail TEXT, created_at TEXT
            );
            INSERT INTO invoice_expense_types (code, name, sort_order) VALUES ('office', '办公费', 1);
            INSERT INTO employees (id, name) VALUES (1, '张三');
        ").unwrap();
        conn
    }

    fn sample_input() -> InvoiceInput {
        InvoiceInput {
            invoice_code: Some("12345".into()),
            invoice_number: Some("67890".into()),
            invoice_type: Some("普通发票".into()),
            issue_date: Some("2026-08-01".into()),
            check_code: None,
            amount: Some(100.0), tax_amount: Some(6.0), total_amount: Some(106.0),
            seller_name: Some("销售方".into()), seller_tax_id: Some("91X".into()),
            buyer_name: Some("购买方".into()), buyer_tax_id: Some("92X".into()),
            expense_type_code: Some("office".into()),
            employee_id: Some(1),
            belong_month: Some("2026-08".into()),
            remark: None,
            image_path: None,
            raw_ocr_json: Some("{}".into()),
        }
    }

    #[test]
    fn test_save_invoice_blocks_duplicate() {
        let conn = setup_db();
        let tmp = std::env::temp_dir();
        let input = sample_input();
        save_invoice(&input, &conn, &tmp).unwrap();
        let result = save_invoice(&input, &conn, &tmp);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("发票已存在"));
    }

    #[test]
    fn test_save_invoice_requires_code_and_number() {
        let conn = setup_db();
        let mut input = sample_input();
        input.invoice_code = None;
        let result = save_invoice(&input, &conn, &std::env::temp_dir());
        assert!(result.is_err());
    }

    #[test]
    fn test_save_invoice_logs_operation() {
        let conn = setup_db();
        save_invoice(&sample_input(), &conn, &std::env::temp_dir()).unwrap();
        let log_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM operation_logs WHERE operation_type = 'save_invoice'",
            [], |r| r.get(0)
        ).unwrap();
        assert_eq!(log_count, 1);
    }

    #[test]
    fn test_copy_image_to_app_dir() {
        let tmp = std::env::temp_dir();
        let src = tmp.join("test_invoice_src.txt");
        std::fs::write(&src, b"hello").unwrap();
        let dest = copy_image_to_app_dir(
            src.to_str().unwrap(),
            Some("2026-08"),
            &tmp.join("app_data"),
        ).unwrap();
        assert!(dest.contains("invoices/2026-08/"));
        let content = std::fs::read_to_string(&dest).unwrap();
        assert_eq!(content, "hello");
    }

    #[test]
    fn test_copy_image_to_app_dir_unclassified_month() {
        let tmp = std::env::temp_dir();
        let src = tmp.join("test_invoice_src2.txt");
        std::fs::write(&src, b"hello").unwrap();
        let dest = copy_image_to_app_dir(
            src.to_str().unwrap(),
            None,
            &tmp.join("app_data2"),
        ).unwrap();
        assert!(dest.contains("invoices/unclassified/"));
    }
}
```

- [ ] **Step 4: 运行测试**

Run: `cd src-tauri && cargo test --lib invoice`
Expected: 4 个业务测试 + 4 个 Task 4 测试全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/invoice.rs
git commit -m "feat(invoice): add business layer with image copy and dedup guards"
```

---

### Task 6: commands.rs 与 lib.rs 注册命令

**Files:**
- Modify: `src-tauri/src/commands.rs`（末尾追加 9 个命令）
- Modify: `src-tauri/src/lib.rs:87-123`（注册命令到 `generate_handler!`）

**Interfaces:**
- Consumes: Task 3 的 db CRUD、Task 4 的 `invoice::ocr_invoice`、Task 5 的 `invoice::save_invoice` / `update_invoice` / `delete_invoice`。
- Produces: 9 个 `#[tauri::command]` 函数。Task 8 的前端 API 调用依赖。

- [ ] **Step 1: 在 `src-tauri/src/commands.rs` 末尾追加 9 个命令**

```rust
// ==================== Invoice Commands ====================

#[tauri::command]
pub fn get_invoice_expense_types(state: tauri::State<'_, Mutex<Connection>>) -> Result<Vec<InvoiceExpenseType>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_invoice_expense_types(&conn)
}

#[tauri::command]
pub fn save_invoice_expense_type(data: InvoiceExpenseTypeInput, state: tauri::State<'_, Mutex<Connection>>) -> Result<InvoiceExpenseType, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    if let Some(id) = data.id {
        let result = db::update_invoice_expense_type(&conn, id, &data)?;
        db::log_operation(&conn, "update_expense_type", &format!("更新费用类型: {}", result.name), "system", None)?;
        Ok(result)
    } else {
        let result = db::insert_invoice_expense_type(&conn, &data)?;
        db::log_operation(&conn, "create_expense_type", &format!("新增费用类型: {}", result.name), "system", None)?;
        Ok(result)
    }
}

#[tauri::command]
pub fn delete_invoice_expense_type(id: i64, state: tauri::State<'_, Mutex<Connection>>) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::delete_invoice_expense_type(&conn, id)?;
    if result {
        db::log_operation(&conn, "delete_expense_type", &format!("删除费用类型ID={id}"), "system", None)?;
    }
    Ok(result)
}

#[tauri::command]
pub fn ocr_invoice(image_path: String, app: tauri::AppHandle, state: tauri::State<'_, Mutex<Connection>>) -> Result<InvoiceOcrPreview, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    crate::invoice::ocr_invoice(&image_path, &conn)
}

#[tauri::command]
pub fn save_invoice(data: InvoiceInput, app: tauri::AppHandle, state: tauri::State<'_, Mutex<Connection>>) -> Result<Invoice, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let app_data_dir = app.path().app_data_dir()
        .map_err(|e| AppError::General(format!("获取app_data_dir失败: {e}")))?;
    crate::invoice::save_invoice(&data, &conn, &app_data_dir)
}

#[tauri::command]
pub fn update_invoice(id: i64, data: InvoiceInput, app: tauri::AppHandle, state: tauri::State<'_, Mutex<Connection>>) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let app_data_dir = app.path().app_data_dir()
        .map_err(|e| AppError::General(format!("获取app_data_dir失败: {e}")))?;
    crate::invoice::update_invoice(id, &data, &conn, &app_data_dir)
}

#[tauri::command]
pub fn delete_invoice(id: i64, state: tauri::State<'_, Mutex<Connection>>) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    crate::invoice::delete_invoice(id, &conn)
}

#[tauri::command]
pub fn query_invoices(query: InvoiceQuery, state: tauri::State<'_, Mutex<Connection>>) -> Result<Vec<Invoice>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::query_invoices(&conn, &query)
}

#[tauri::command]
pub fn export_invoice_list(query: InvoiceQuery, path: String, state: tauri::State<'_, Mutex<Connection>>) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let invoices = db::query_invoices(&conn, &query)?;
    excel::export_invoice_list(&invoices, &path)?;
    db::log_operation(&conn, "export_invoices", &format!("导出发票清单: {}条到{}", invoices.len(), path), "system", None)?;
    Ok(true)
}
```

- [ ] **Step 2: 在 `lib.rs` 的 `generate_handler!` 列表中追加新命令**

修改 `src-tauri/src/lib.rs:87-123`，在 `commands::ocr_recognize_punch_card,` 之后追加：

```rust
        commands::get_invoice_expense_types,
        commands::save_invoice_expense_type,
        commands::delete_invoice_expense_type,
        commands::ocr_invoice,
        commands::save_invoice,
        commands::update_invoice,
        commands::delete_invoice,
        commands::query_invoices,
        commands::export_invoice_list,
```

- [ ] **Step 3: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 编译通过。`excel::export_invoice_list` 暂未实现会报错，下个 Task 解决；若本步报错请确认错误信息仅为 `cannot find function export_invoice_list`，这属于预期。

> 如果 `cargo check` 因 `export_invoice_list` 未定义而失败，可临时在 `excel.rs` 加占位：
> ```rust
> pub fn export_invoice_list(_invoices: &[crate::models::Invoice], _path: &str) -> crate::errors::AppResult<bool> { Ok(true) }
> ```
> Task 7 会替换为真实实现。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(invoice): wire invoice tauri commands"
```

---

### Task 7: excel.rs 导出发票清单

**Files:**
- Modify: `src-tauri/src/excel.rs`（追加导出函数，替换 Task 6 的占位）

**Interfaces:**
- Consumes: Task 2 的 `Invoice` 结构体。
- Produces: `excel::export_invoice_list(invoices, path) -> AppResult<bool>`。Task 6 的 `commands::export_invoice_list` 依赖。

- [ ] **Step 1: 在 `src-tauri/src/excel.rs` 末尾追加导出函数**

先在文件顶部 `use` 区确认/补充：

```rust
use crate::models::Invoice;
```

然后追加函数：

```rust
pub fn export_invoice_list(invoices: &[Invoice], path: &str) -> AppResult<bool> {
    use rust_xlsxwriter::{Format, FormatBorder};

    let workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("发票清单")?;

    let header_fmt = Format::new()
        .set_bold()
        .set_background_color("#D9E1F2")
        .set_border(FormatBorder::Thin);

    let headers = [
        "归属月份", "报销人ID", "发票类型", "发票代码", "发票号码",
        "开票日期", "金额", "税额", "价税合计",
        "销售方", "销售方税号", "购买方", "购买方税号",
        "费用类型", "状态", "备注", "录入时间",
    ];
    for (col, h) in headers.iter().enumerate() {
        worksheet.write_with_format(0, col as u16, *h, &header_fmt)?;
    }

    for (row_idx, inv) in invoices.iter().enumerate() {
        let row = (row_idx + 1) as u32;
        worksheet.write_string(row, 0, inv.belong_month.clone().unwrap_or_default())?;
        worksheet.write_number(row, 1, inv.employee_id.unwrap_or(0) as f64)?;
        worksheet.write_string(row, 2, inv.invoice_type.clone().unwrap_or_default())?;
        worksheet.write_string(row, 3, inv.invoice_code.clone().unwrap_or_default())?;
        worksheet.write_string(row, 4, inv.invoice_number.clone().unwrap_or_default())?;
        worksheet.write_string(row, 5, inv.issue_date.clone().unwrap_or_default())?;
        worksheet.write_number(row, 6, inv.amount)?;
        worksheet.write_number(row, 7, inv.tax_amount)?;
        worksheet.write_number(row, 8, inv.total_amount)?;
        worksheet.write_string(row, 9, inv.seller_name.clone().unwrap_or_default())?;
        worksheet.write_string(row, 10, inv.seller_tax_id.clone().unwrap_or_default())?;
        worksheet.write_string(row, 11, inv.buyer_name.clone().unwrap_or_default())?;
        worksheet.write_string(row, 12, inv.buyer_tax_id.clone().unwrap_or_default())?;
        worksheet.write_string(row, 13, inv.expense_type_code.clone().unwrap_or_default())?;
        worksheet.write_string(row, 14, inv.status.clone().unwrap_or_default())?;
        worksheet.write_string(row, 15, inv.remark.clone().unwrap_or_default())?;
        worksheet.write_string(row, 16, inv.created_at.clone().unwrap_or_default())?;
    }

    worksheet.set_column_width(0, 10)?;
    worksheet.set_column_width(3, 16)?;
    worksheet.set_column_width(4, 12)?;
    worksheet.set_column_width(9, 30)?;
    worksheet.set_column_width(11, 30)?;
    worksheet.set_column_width(16, 22)?;

    workbook.save(path)?;
    Ok(true)
}
```

- [ ] **Step 2: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 编译通过（含 Task 6 的占位函数与真实实现冲突的话，删掉占位即可）。

如果之前在 Task 6 Step 3 加了占位函数，现在删除它。

- [ ] **Step 3: 运行所有测试**

Run: `cd src-tauri && cargo test --lib`
Expected: 所有测试 PASS。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/excel.rs
git commit -m "feat(invoice): export invoice list to Excel"
```

---

### Task 8: 前端类型与 API 封装

**Files:**
- Modify: `src/types/index.ts`（追加 5 个接口）
- Modify: `src/api/index.ts`（追加 9 个 API 函数）

**Interfaces:**
- Consumes: Task 6 的 9 个 tauri commands。
- Produces: 前端类型 `Invoice`、`InvoiceInput`、`InvoiceOcrPreview`、`InvoiceQuery`、`InvoiceExpenseType`、`InvoiceExpenseTypeInput` 与 9 个 API 函数。Task 9 依赖。

- [ ] **Step 1: 在 `src/types/index.ts` 末尾追加发票相关类型**

```ts
// ==================== 发票相关 ====================

export type InvoiceStatus = 'normal' | 'void';

export interface InvoiceExpenseType {
  id: number;
  code: string;
  name: string;
  sort_order: number;
  enabled: number;
  remark?: string;
}

export interface InvoiceExpenseTypeInput {
  id?: number;
  code?: string;
  name?: string;
  sort_order?: number;
  enabled?: number;
  remark?: string;
}

export interface Invoice {
  id: number;
  invoice_code?: string;
  invoice_number?: string;
  invoice_type?: string;
  issue_date?: string;
  check_code?: string;
  amount: number;
  tax_amount: number;
  total_amount: number;
  seller_name?: string;
  seller_tax_id?: string;
  buyer_name?: string;
  buyer_tax_id?: string;
  expense_type_code?: string;
  employee_id?: number;
  belong_month?: string;
  status: InvoiceStatus;
  remark?: string;
  image_path?: string;
  raw_ocr_json?: string;
  created_at?: string;
  updated_at?: string;
}

export interface InvoiceInput {
  invoice_code?: string;
  invoice_number?: string;
  invoice_type?: string;
  issue_date?: string;
  check_code?: string;
  amount?: number;
  tax_amount?: number;
  total_amount?: number;
  seller_name?: string;
  seller_tax_id?: string;
  buyer_name?: string;
  buyer_tax_id?: string;
  expense_type_code?: string;
  employee_id?: number;
  belong_month?: string;
  remark?: string;
  image_path?: string;
  raw_ocr_json?: string;
}

export interface InvoiceOcrPreview {
  invoice_code?: string;
  invoice_number?: string;
  invoice_type?: string;
  issue_date?: string;
  check_code?: string;
  amount: number;
  tax_amount: number;
  total_amount: number;
  seller_name?: string;
  seller_tax_id?: string;
  buyer_name?: string;
  buyer_tax_id?: string;
  raw_ocr_json: string;
  warnings: string[];
  is_duplicate: boolean;
  duplicate_invoice_id?: number;
}

export interface InvoiceQuery {
  belong_month?: string;
  employee_id?: number;
  expense_type_code?: string;
  invoice_type?: string;
  keyword?: string;
  status?: InvoiceStatus;
}
```

- [ ] **Step 2: 在 `src/api/index.ts` 追加 9 个 API 函数（文件末尾）**

注意：后端字段已是 snake_case，与 Invoice 接口字段一致，无需 normalize。

```ts
// ==================== 发票管理 ====================

export async function getInvoiceExpenseTypes(): Promise<InvoiceExpenseType[]> {
  return invoke<InvoiceExpenseType[]>('get_invoice_expense_types');
}

export async function saveInvoiceExpenseType(data: InvoiceExpenseTypeInput): Promise<InvoiceExpenseType> {
  return invoke<InvoiceExpenseType>('save_invoice_expense_type', { data });
}

export async function deleteInvoiceExpenseType(id: number): Promise<void> {
  await invoke('delete_invoice_expense_type', { id });
}

export async function ocrInvoice(filePath: string): Promise<InvoiceOcrPreview> {
  return invoke<InvoiceOcrPreview>('ocr_invoice', { imagePath: filePath });
}

export async function saveInvoice(data: InvoiceInput): Promise<Invoice> {
  return invoke<Invoice>('save_invoice', { data });
}

export async function updateInvoice(id: number, data: InvoiceInput): Promise<boolean> {
  return invoke<boolean>('update_invoice', { id, data });
}

export async function deleteInvoice(id: number): Promise<void> {
  await invoke('delete_invoice', { id });
}

export async function queryInvoices(query: InvoiceQuery): Promise<Invoice[]> {
  return invoke<Invoice[]>('query_invoices', { query });
}

export async function exportInvoiceList(query: InvoiceQuery, savePath: string): Promise<void> {
  await invoke('export_invoice_list', { query, path: savePath });
}
```

并在文件顶部 `import type { ... } from '@/types';` 中添加新类型：

```ts
import type {
  // ... 已有的类型 ...
  InvoiceExpenseType,
  InvoiceExpenseTypeInput,
  Invoice,
  InvoiceInput,
  InvoiceOcrPreview,
  InvoiceQuery,
} from '@/types';
```

- [ ] **Step 3: 类型检查**

Run: `cd /home/zhang/workspace/Project/salary/salary-desktop && npx tsc --noEmit`
Expected: 无类型错误。

- [ ] **Step 4: Commit**

```bash
git add src/types/index.ts src/api/index.ts
git commit -m "feat(invoice): add frontend types and API wrappers"
```

---

### Task 9: 前端 Invoices.tsx 主页面

**Files:**
- Create: `src/pages/Invoices.tsx`

**Interfaces:**
- Consumes: Task 8 的全部 API 与类型；现有 `getEmployees` API（用于报销人下拉）；`@tauri-apps/plugin-dialog`（文件选择）。

- [ ] **Step 1: 创建 `src/pages/Invoices.tsx`**

```tsx
import { useState, useEffect, useCallback } from 'react';
import {
  Button, Table, Card, Row, Col, Input, Select, DatePicker, Modal,
  message, Space, Tag, Form, Upload, Drawer, Spin, Alert, Statistic,
} from 'antd';
import {
  UploadOutlined, ExportOutlined, SettingOutlined, ScanOutlined,
  EditOutlined, DeleteOutlined, EyeOutlined, PlusOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import { open, save } from '@tauri-apps/plugin-dialog';
import {
  getInvoiceExpenseTypes, saveInvoiceExpenseType, deleteInvoiceExpenseType,
  ocrInvoice, saveInvoice, updateInvoice, deleteInvoice, queryInvoices, exportInvoiceList,
  getEmployees,
} from '@/api';
import type {
  Invoice, InvoiceInput, InvoiceOcrPreview, InvoiceQuery,
  InvoiceExpenseType, InvoiceExpenseTypeInput, Employee,
} from '@/types';

const { TextArea } = Input;

const Invoices: React.FC = () => {
  const [list, setList] = useState<Invoice[]>([]);
  const [loading, setLoading] = useState(false);
  const [expenseTypes, setExpenseTypes] = useState<InvoiceExpenseType[]>([]);
  const [employees, setEmployees] = useState<Employee[]>([]);

  const [filterMonth, setFilterMonth] = useState<dayjs.Dayjs | null>(dayjs());
  const [filterEmployee, setFilterEmployee] = useState<number | undefined>(undefined);
  const [filterExpenseType, setFilterExpenseType] = useState<string | undefined>(undefined);
  const [filterInvoiceType, setFilterInvoiceType] = useState<string | undefined>(undefined);
  const [filterKeyword, setFilterKeyword] = useState('');

  const [uploadModal, setUploadModal] = useState<{
    visible: boolean;
    ocrLoading: boolean;
    preview: InvoiceOcrPreview | null;
    selectedFilePath: string | null;
    editingId: number | null;
    form: InvoiceInput;
  }>({
    visible: false, ocrLoading: false, preview: null,
    selectedFilePath: null, editingId: null, form: {},
  });

  const [viewDrawer, setViewDrawer] = useState<Invoice | null>(null);
  const [expenseDrawer, setExpenseDrawer] = useState(false);
  const [expenseForm, setExpenseForm] = useState<InvoiceExpenseTypeInput>({});

  const fetchList = useCallback(async () => {
    setLoading(true);
    try {
      const query: InvoiceQuery = {
        belong_month: filterMonth ? filterMonth.format('YYYY-MM') : undefined,
        employee_id: filterEmployee,
        expense_type_code: filterExpenseType,
        invoice_type: filterInvoiceType || undefined,
        keyword: filterKeyword || undefined,
      };
      const data = await queryInvoices(query);
      setList(data);
    } catch (e: unknown) {
      message.error('查询失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [filterMonth, filterEmployee, filterExpenseType, filterInvoiceType, filterKeyword]);

  const fetchExpenseTypes = useCallback(async () => {
    try { setExpenseTypes(await getInvoiceExpenseTypes()); } catch { /* ignore */ }
  }, []);

  const fetchEmployees = useCallback(async () => {
    try { setEmployees(await getEmployees()); } catch { /* ignore */ }
  }, []);

  useEffect(() => { fetchList(); }, [fetchList]);
  useEffect(() => { fetchExpenseTypes(); fetchEmployees(); }, [fetchExpenseTypes, fetchEmployees]);

  const totalAmount = list.reduce((s, i) => s + (i.total_amount || 0), 0);
  const duplicateCount = 0; // 当前查询结果里的重复数（实际入库的不会是重复，留作未来扩展）

  const handleUploadClick = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: '发票图片/PDF', extensions: ['pdf', 'png', 'jpg', 'jpeg', 'bmp'] }],
      });
      if (!selected) return;
      const filePath = selected as string;

      // 体积检查（10MB 上限）
      // Tauri dialog 不返回 size，需要用 fs stat；这里简化：直接发请求让后端报错

      setUploadModal({
        visible: true, ocrLoading: true, preview: null,
        selectedFilePath: filePath, editingId: null, form: {},
      });

      try {
        const preview = await ocrInvoice(filePath);
        setUploadModal(prev => ({
          ...prev,
          ocrLoading: false,
          preview,
          form: {
            invoice_code: preview.invoice_code,
            invoice_number: preview.invoice_number,
            invoice_type: preview.invoice_type,
            issue_date: preview.issue_date,
            check_code: preview.check_code,
            amount: preview.amount,
            tax_amount: preview.tax_amount,
            total_amount: preview.total_amount,
            seller_name: preview.seller_name,
            seller_tax_id: preview.seller_tax_id,
            buyer_name: preview.buyer_name,
            buyer_tax_id: preview.buyer_tax_id,
            belong_month: filterMonth?.format('YYYY-MM'),
            image_path: filePath,
            raw_ocr_json: preview.raw_ocr_json,
          },
        }));
      } catch (e: unknown) {
        const msg = e instanceof Error ? e.message : String(e);
        message.error('OCR识别失败: ' + msg + '（可手工录入）');
        setUploadModal(prev => ({
          ...prev, ocrLoading: false, preview: null,
          form: { belong_month: filterMonth?.format('YYYY-MM'), image_path: filePath },
        }));
      }
    } catch (e: unknown) {
      message.error('选择文件失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const handleManualAdd = () => {
    setUploadModal({
      visible: true, ocrLoading: false, preview: null,
      selectedFilePath: null, editingId: null,
      form: { belong_month: filterMonth?.format('YYYY-MM') },
    });
  };

  const handleSaveInvoice = async () => {
    const { form, editingId, selectedFilePath } = uploadModal;
    if (!form.invoice_code || !form.invoice_number) {
      message.warning('发票代码和号码必填');
      return;
    }
    if (!form.employee_id) {
      message.warning('请选择报销人');
      return;
    }
    if (!form.expense_type_code) {
      message.warning('请选择费用类型');
      return;
    }
    if (uploadModal.preview?.is_duplicate && !editingId) {
      message.error('该发票已存在，禁止保存');
      return;
    }

    try {
      const payload: InvoiceInput = { ...form, image_path: selectedFilePath ?? form.image_path };
      if (editingId) {
        await updateInvoice(editingId, payload);
        message.success('更新成功');
      } else {
        await saveInvoice(payload);
        message.success('保存成功');
      }
      setUploadModal(prev => ({ ...prev, visible: false }));
      fetchList();
    } catch (e: unknown) {
      message.error('保存失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const handleEdit = (record: Invoice) => {
    setUploadModal({
      visible: true,
      ocrLoading: false,
      preview: null,
      selectedFilePath: null,
      editingId: record.id,
      form: {
        invoice_code: record.invoice_code,
        invoice_number: record.invoice_number,
        invoice_type: record.invoice_type,
        issue_date: record.issue_date,
        check_code: record.check_code,
        amount: record.amount,
        tax_amount: record.tax_amount,
        total_amount: record.total_amount,
        seller_name: record.seller_name,
        seller_tax_id: record.seller_tax_id,
        buyer_name: record.buyer_name,
        buyer_tax_id: record.buyer_tax_id,
        expense_type_code: record.expense_type_code,
        employee_id: record.employee_id,
        belong_month: record.belong_month,
        remark: record.remark,
        image_path: record.image_path,
      },
    });
  };

  const handleDelete = async (id: number) => {
    Modal.confirm({
      title: '确认删除',
      content: '删除后发票记录将标记为作废，不会物理删除。是否继续？',
      okType: 'danger',
      okText: '删除',
      cancelText: '取消',
      onOk: async () => {
        try {
          await deleteInvoice(id);
          message.success('已删除');
          fetchList();
        } catch (e: unknown) {
          message.error('删除失败: ' + (e instanceof Error ? e.message : String(e)));
        }
      },
    });
  };

  const handleExport = async () => {
    try {
      const savePath = await save({
        defaultPath: `发票清单_${filterMonth?.format('YYYY-MM') ?? 'all'}.xlsx`,
        filters: [{ name: 'Excel', extensions: ['xlsx'] }],
      });
      if (!savePath) return;
      const query: InvoiceQuery = {
        belong_month: filterMonth ? filterMonth.format('YYYY-MM') : undefined,
        employee_id: filterEmployee,
        expense_type_code: filterExpenseType,
        invoice_type: filterInvoiceType || undefined,
        keyword: filterKeyword || undefined,
      };
      await exportInvoiceList(query, savePath);
      message.success('已导出');
    } catch (e: unknown) {
      message.error('导出失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const handleSaveExpenseType = async () => {
    if (!expenseForm.name || !expenseForm.code) {
      message.warning('编码和名称必填');
      return;
    }
    try {
      await saveInvoiceExpenseType(expenseForm);
      message.success('保存成功');
      setExpenseForm({});
      fetchExpenseTypes();
    } catch (e: unknown) {
      message.error('保存失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const handleDeleteExpenseType = (id: number, code: string) => {
    if (code === 'other') {
      message.warning('「其他」类型不允许删除');
      return;
    }
    Modal.confirm({
      title: '确认删除',
      content: '删除后无法恢复。在用的类型不允许删除。',
      okType: 'danger',
      onOk: async () => {
        try {
          await deleteInvoiceExpenseType(id);
          message.success('已删除');
          fetchExpenseTypes();
        } catch (e: unknown) {
          message.error('删除失败: ' + (e instanceof Error ? e.message : String(e)));
        }
      },
    });
  };

  const columns = [
    {
      title: '发票代码/号码',
      key: 'code_number',
      width: 200,
      render: (_: unknown, r: Invoice) => (
        <div>
          <div style={{ fontSize: 12, color: '#999' }}>{r.invoice_code || '-'}</div>
          <div style={{ fontWeight: 500 }}>{r.invoice_number || '-'}</div>
        </div>
      ),
    },
    { title: '类型', dataIndex: 'invoice_type', key: 'type', width: 120 },
    { title: '开票日期', dataIndex: 'issue_date', key: 'date', width: 110 },
    { title: '销售方', dataIndex: 'seller_name', key: 'seller', ellipsis: true },
    {
      title: '报销人',
      key: 'employee',
      width: 100,
      render: (_: unknown, r: Invoice) => {
        const emp = employees.find(e => e.id === r.employee_id);
        return emp?.name || '-';
      },
    },
    {
      title: '费用类型',
      key: 'expense',
      width: 100,
      render: (_: unknown, r: Invoice) => {
        const t = expenseTypes.find(e => e.code === r.expense_type_code);
        return t ? <Tag>{t.name}</Tag> : '-';
      },
    },
    {
      title: '价税合计',
      dataIndex: 'total_amount',
      key: 'total',
      width: 110,
      align: 'right' as const,
      render: (v: number) => `¥${(v || 0).toFixed(2)}`,
    },
    {
      title: '操作',
      key: 'actions',
      width: 160,
      render: (_: unknown, r: Invoice) => (
        <Space>
          <Button size="small" icon={<EyeOutlined />} onClick={() => setViewDrawer(r)} />
          <Button size="small" icon={<EditOutlined />} onClick={() => handleEdit(r)} />
          <Button size="small" danger icon={<DeleteOutlined />} onClick={() => handleDelete(r.id)} />
        </Space>
      ),
    },
  ];

  const isSaveDisabled =
    !uploadModal.form.invoice_code ||
    !uploadModal.form.invoice_number ||
    !uploadModal.form.employee_id ||
    !uploadModal.form.expense_type_code ||
    (!!uploadModal.preview?.is_duplicate && !uploadModal.editingId);

  return (
    <div>
      <div className="page-header" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <span className="page-title">发票管理</span>
        <Space>
          <Button icon={<PlusOutlined />} onClick={handleManualAdd}>手工录入</Button>
          <Button type="primary" icon={<UploadOutlined />} onClick={handleUploadClick}>上传发票识别</Button>
          <Button icon={<ExportOutlined />} onClick={handleExport}>导出清单</Button>
          <Button icon={<SettingOutlined />} onClick={() => setExpenseDrawer(true)}>费用类型</Button>
        </Space>
      </div>

      <Row gutter={16} style={{ marginBottom: 16 }}>
        <Col span={6}><Card><Statistic title="发票张数" value={list.length} /></Card></Col>
        <Col span={6}><Card><Statistic title="价税合计" value={totalAmount} precision={2} prefix="¥" /></Card></Col>
        <Col span={6}><Card><Statistic title="本月去重拦截" value={duplicateCount} /></Card></Col>
      </Row>

      <Card style={{ marginBottom: 16 }}>
        <Space wrap>
          <DatePicker
            picker="month" allowClear
            value={filterMonth}
            onChange={(d) => setFilterMonth(d)}
            placeholder="归属月份"
          />
          <Select
            style={{ width: 160 }} allowClear placeholder="报销人"
            value={filterEmployee}
            onChange={(v) => setFilterEmployee(v)}
            options={employees.map(e => ({ value: e.id, label: `${e.name} (${e.employee_no})` }))}
          />
          <Select
            style={{ width: 160 }} allowClear placeholder="费用类型"
            value={filterExpenseType}
            onChange={(v) => setFilterExpenseType(v)}
            options={expenseTypes.map(t => ({ value: t.code, label: t.name }))}
          />
          <Select
            style={{ width: 160 }} allowClear placeholder="发票类型"
            value={filterInvoiceType}
            onChange={(v) => setFilterInvoiceType(v)}
            options={[
              { value: '增值税普通发票', label: '增值税普通发票' },
              { value: '增值税专用发票', label: '增值税专用发票' },
              { value: '增值税电子普通发票', label: '增值税电子普通发票' },
            ]}
          />
          <Input.Search
            style={{ width: 240 }} allowClear placeholder="销售方/购买方/备注"
            value={filterKeyword}
            onChange={(e) => setFilterKeyword(e.target.value)}
            onSearch={fetchList}
          />
          <Button type="primary" onClick={fetchList}>查询</Button>
        </Space>
      </Card>

      <Card>
        <Table
          rowKey="id"
          loading={loading}
          columns={columns}
          dataSource={list}
          size="small"
          scroll={{ x: 'max-content' }}
        />
      </Card>

      {/* 上传/编辑 Modal */}
      <Modal
        title={uploadModal.editingId ? '编辑发票' : '上传发票'}
        open={uploadModal.visible}
        onCancel={() => setUploadModal(prev => ({ ...prev, visible: false }))}
        onOk={handleSaveInvoice}
        okText="保存"
        cancelText="取消"
        okButtonProps={{ disabled: isSaveDisabled }}
        width={900}
      >
        <Spin spinning={uploadModal.ocrLoading} tip="正在识别...">
          {uploadModal.preview?.is_duplicate && (
            <Alert
              style={{ marginBottom: 12 }}
              type="error"
              showIcon
              message="重复发票"
              description={`该发票已存在（ID=${uploadModal.preview.duplicate_invoice_id}），不能重复报销。`}
            />
          )}
          {uploadModal.preview?.warnings && uploadModal.preview.warnings.length > 0 && (
            <Alert
              style={{ marginBottom: 12 }}
              type="warning"
              showIcon
              message="识别提醒"
              description={uploadModal.preview.warnings.map((w, i) => <div key={i}>{w}</div>)}
            />
          )}
          <Row gutter={16}>
            <Col span={12}>
              <Form layout="vertical" size="small">
                <Form.Item label="发票代码" required>
                  <Input
                    value={uploadModal.form.invoice_code || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, invoice_code: e.target.value }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="发票号码" required>
                  <Input
                    value={uploadModal.form.invoice_number || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, invoice_number: e.target.value }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="发票类型">
                  <Input
                    value={uploadModal.form.invoice_type || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, invoice_type: e.target.value }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="开票日期">
                  <Input
                    value={uploadModal.form.issue_date || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, issue_date: e.target.value }
                    }))}
                    placeholder="2026-08-01"
                  />
                </Form.Item>
                <Form.Item label="金额（不含税）">
                  <Input
                    type="number"
                    value={uploadModal.form.amount ?? ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, amount: parseFloat(e.target.value) || 0 }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="税额">
                  <Input
                    type="number"
                    value={uploadModal.form.tax_amount ?? ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, tax_amount: parseFloat(e.target.value) || 0 }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="价税合计">
                  <Input
                    type="number"
                    value={uploadModal.form.total_amount ?? ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, total_amount: parseFloat(e.target.value) || 0 }
                    }))}
                  />
                </Form.Item>
              </Form>
            </Col>
            <Col span={12}>
              <Form layout="vertical" size="small">
                <Form.Item label="销售方">
                  <Input
                    value={uploadModal.form.seller_name || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, seller_name: e.target.value }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="销售方税号">
                  <Input
                    value={uploadModal.form.seller_tax_id || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, seller_tax_id: e.target.value }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="购买方">
                  <Input
                    value={uploadModal.form.buyer_name || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, buyer_name: e.target.value }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="报销人" required>
                  <Select
                    value={uploadModal.form.employee_id}
                    onChange={(v) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, employee_id: v }
                    }))}
                    options={employees.map(e => ({ value: e.id, label: `${e.name} (${e.employee_no})` }))}
                    placeholder="选择报销人"
                  />
                </Form.Item>
                <Form.Item label="费用类型" required>
                  <Select
                    value={uploadModal.form.expense_type_code}
                    onChange={(v) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, expense_type_code: v }
                    }))}
                    options={expenseTypes.map(t => ({ value: t.code, label: t.name }))}
                    placeholder="选择费用类型"
                  />
                </Form.Item>
                <Form.Item label="归属月份">
                  <Input
                    value={uploadModal.form.belong_month || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, belong_month: e.target.value }
                    }))}
                    placeholder="2026-08"
                  />
                </Form.Item>
                <Form.Item label="备注">
                  <TextArea
                    rows={2}
                    value={uploadModal.form.remark || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, remark: e.target.value }
                    }))}
                  />
                </Form.Item>
              </Form>
            </Col>
          </Row>
        </Spin>
      </Modal>

      {/* 详情 Drawer */}
      <Drawer
        title="发票详情"
        open={!!viewDrawer}
        onClose={() => setViewDrawer(null)}
        width={500}
      >
        {viewDrawer && (
          <div>
            {viewDrawer.image_path && (
              <div style={{ marginBottom: 16 }}>
                {viewDrawer.image_path.endsWith('.pdf') ? (
                  <iframe src={viewDrawer.image_path} style={{ width: '100%', height: 400 }} title="发票原图" />
                ) : (
                  <img src={viewDrawer.image_path} alt="发票原图" style={{ width: '100%' }} />
                )}
              </div>
            )}
            <p><b>发票代码：</b>{viewDrawer.invoice_code || '-'}</p>
            <p><b>发票号码：</b>{viewDrawer.invoice_number || '-'}</p>
            <p><b>类型：</b>{viewDrawer.invoice_type || '-'}</p>
            <p><b>开票日期：</b>{viewDrawer.issue_date || '-'}</p>
            <p><b>金额：</b>¥{(viewDrawer.amount || 0).toFixed(2)}</p>
            <p><b>税额：</b>¥{(viewDrawer.tax_amount || 0).toFixed(2)}</p>
            <p><b>价税合计：</b>¥{(viewDrawer.total_amount || 0).toFixed(2)}</p>
            <p><b>销售方：</b>{viewDrawer.seller_name || '-'}</p>
            <p><b>购买方：</b>{viewDrawer.buyer_name || '-'}</p>
            <p><b>录入时间：</b>{viewDrawer.created_at || '-'}</p>
          </div>
        )}
      </Drawer>

      {/* 费用类型管理 Drawer */}
      <Drawer
        title="费用类型管理"
        open={expenseDrawer}
        onClose={() => setExpenseDrawer(false)}
        width={500}
      >
        <Card title="新增/编辑类型" size="small" style={{ marginBottom: 16 }}>
          <Form layout="vertical" size="small">
            <Form.Item label="编码（创建后不可改）">
              <Input
                value={expenseForm.code || ''}
                onChange={(e) => setExpenseForm(prev => ({ ...prev, code: e.target.value }))}
                disabled={!!expenseForm.id}
              />
            </Form.Item>
            <Form.Item label="名称">
              <Input
                value={expenseForm.name || ''}
                onChange={(e) => setExpenseForm(prev => ({ ...prev, name: e.target.value }))}
              />
            </Form.Item>
            <Form.Item label="排序">
              <Input
                type="number"
                value={expenseForm.sort_order ?? 99}
                onChange={(e) => setExpenseForm(prev => ({ ...prev, sort_order: parseInt(e.target.value) || 99 }))}
              />
            </Form.Item>
            <Space>
              <Button type="primary" onClick={handleSaveExpenseType}>保存</Button>
              <Button onClick={() => setExpenseForm({})}>重置</Button>
            </Space>
          </Form>
        </Card>
        <Card title="已有类型" size="small">
          {expenseTypes.map(t => (
            <div key={t.id} style={{ display: 'flex', justifyContent: 'space-between', padding: '6px 0' }}>
              <span>{t.name} <Tag>{t.code}</Tag> {t.enabled === 0 && <Tag color="default">已禁用</Tag>}</span>
              <Space>
                <Button size="small" onClick={() => setExpenseForm(t)}>编辑</Button>
                <Button size="small" danger onClick={() => handleDeleteExpenseType(t.id, t.code)}>删除</Button>
              </Space>
            </div>
          ))}
        </Card>
      </Drawer>
    </div>
  );
};

export default Invoices;
```

- [ ] **Step 2: 类型检查**

Run: `cd /home/zhang/workspace/Project/salary/salary-desktop && npx tsc --noEmit`
Expected: 无类型错误。如有未使用 import 警告（如 `Upload`），删除即可。

- [ ] **Step 3: Commit**

```bash
git add src/pages/Invoices.tsx
git commit -m "feat(invoice): add Invoices page with upload/edit/dedupe/export"
```

---

### Task 10: App.tsx 菜单注册与端到端验收

**Files:**
- Modify: `src/App.tsx:4-15`（图标 import）、`src/App.tsx:30-39`（菜单项）、`src/App.tsx:93-102`（路由）

**Interfaces:**
- Consumes: Task 9 的 `Invoices` 页面组件。

- [ ] **Step 1: 修改 `src/App.tsx`**

在 `@ant-design/icons` import 中加 `FileTextOutlined`：

```tsx
import {
  DashboardOutlined, TeamOutlined, CalendarOutlined, ScanOutlined, SettingOutlined,
  CalculatorOutlined, ExportOutlined, MenuFoldOutlined, MenuUnfoldOutlined,
  FormOutlined, FileTextOutlined,
} from '@ant-design/icons';
```

在 `menuItems` 数组中（在 `导出中心` 之前）插入：

```tsx
  { key: '/invoices', label: '发票管理', icon: <FileTextOutlined /> },
```

完整 `menuItems`：
```tsx
const menuItems = [
  { key: '/', label: '首页仪表盘', icon: <DashboardOutlined /> },
  { key: '/employees', label: '员工管理', icon: <TeamOutlined /> },
  { key: '/attendance', label: '考勤管理', icon: <CalendarOutlined /> },
  { key: '/punch-card', label: '打卡表管理', icon: <FormOutlined /> },
  { key: '/ocr', label: 'OCR识别中心', icon: <ScanOutlined /> },
  { key: '/invoices', label: '发票管理', icon: <FileTextOutlined /> },
  { key: '/rules', label: '规则配置', icon: <SettingOutlined /> },
  { key: '/salary', label: '工资计算', icon: <CalculatorOutlined /> },
  { key: '/export', label: '导出中心', icon: <ExportOutlined /> },
];
```

在顶部组件 import 中加：

```tsx
import Invoices from '@/pages/Invoices';
```

在 `<Routes>` 中加：

```tsx
<Route path="/invoices" element={<Invoices />} />
```

- [ ] **Step 2: 类型检查**

Run: `cd /home/zhang/workspace/Project/salary/salary-desktop && npx tsc --noEmit`
Expected: 无类型错误。

- [ ] **Step 3: 启动开发模式**

Run: `cd /home/zhang/workspace/Project/salary/salary-desktop && npm run tauri dev`
Expected: 应用启动后侧边栏出现「发票管理」菜单，点击进入页面无报错。

- [ ] **Step 4: 端到端验收测试**

执行以下手工测试（参考 spec 第 8 节）：

1. **进入页面**：菜单点击「发票管理」，页面正常加载，无 console 错误。
2. **费用类型默认值**：点开「费用类型」Drawer，确认 7 个预设类型存在（办公/差旅/餐饮/交通/住宿/通讯/其他）。
3. **上传发票 OCR**：
   - 准备一张真实增值税普通发票图片。
   - 点「上传发票识别」→ 选图 → 等 OCR 完成 → 表单字段自动填入。
   - 选报销人、费用类型 → 点保存 → toast「保存成功」。
   - 列表刷新出现新记录。
4. **重复报销拦截**：
   - 用同一张发票再次「上传发票识别」。
   - 表单出现红色 Alert「重复发票」。
   - 保存按钮 disabled。
5. **手工录入**：
   - 点「手工录入」→ 弹空表单。
   - 手工填代码/号码/报销人/费用类型 → 保存成功。
6. **编辑**：
   - 列表点编辑图标 → Modal 显示 → 改备注 → 保存成功。
7. **删除**：
   - 点删除图标 → 确认 → 列表里消失。
8. **导出**：
   - 点「导出清单」→ 选保存路径 → 打开 xlsx 文件 → 列和数据正确。
9. **筛选**：
   - 切换归属月份、报销人、费用类型 → 列表过滤正确。
10. **回归**：
    - 切到「OCR识别中心」 → 上传考勤图 → 识别功能正常（验证 ocr.rs 改造没破坏）。
    - 切到「员工管理」 → 员工列表正常。

- [ ] **Step 5: Commit**

```bash
git add src/App.tsx
git commit -m "feat(invoice): register invoice menu and route"
```

- [ ] **Step 6: 推送（可选）**

如果用户允许：
```bash
git push
```

---

## Self-Review

完成所有任务后，对照 spec 自检：

**1. Spec 覆盖**

| Spec 章节 | 实现任务 |
|----------|---------|
| 数据库设计（2 表 + 4 索引 + 7 默认） | Task 1 |
| models.rs 结构体 | Task 2 |
| db CRUD | Task 3 |
| OCR 调用（vat_invoice） | Task 4 |
| 业务层（save/update/delete + 图片复制） | Task 5 |
| 9 个 commands | Task 6 |
| Excel 导出 | Task 7 |
| 前端类型 + API | Task 8 |
| 前端页面（列表+上传+编辑+导出+费用类型） | Task 9 |
| 菜单注册 | Task 10 |
| 错误处理（重复拦截、OCR 失败兜底、删除拦截） | Task 4/5/9 内嵌 |
| 测试策略（单元 + 集成 + 手工） | Task 3/4/5/10 |

**2. 类型一致性**
- `Invoice` 字段名（snake_case）前后端一致。
- `ocr_invoice` 返回 `InvoiceOcrPreview`，前端调用同名 API。
- `copy_image_to_app_dir` 在 Task 5 定义，在 Task 6 commands 间接调用（通过 `invoice::save_invoice`），无类型不一致。
- `app_data_dir` 命名一致。

**3. 占位符扫描**
- Task 6 Step 3 提到的 `export_invoice_list` 占位是临时性的，Task 7 立即替换，已明确说明。
- Task 9 中 `Upload` import 可能 unused，Step 2 已提示删除。

**4. 范围**
- 10 个任务在单一实现计划范围内，每个任务 30-90 分钟可完成，TDD 节奏明确。
