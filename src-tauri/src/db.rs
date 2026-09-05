use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::{Datelike, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::errors::{AppError, AppResult};
use crate::models::*;

#[allow(dead_code)]
pub type DbState = Mutex<Connection>;

pub fn init_db(app_data_dir: &str) -> AppResult<Connection> {
    let mut db_path = PathBuf::from(app_data_dir);
    db_path.push("salary.db");

    let conn = Connection::open(&db_path)?;

    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    create_tables(&conn)?;
    seed_gl_accounts(&conn)?;
    insert_default_data(&conn)?;

    Ok(conn)
}

/// 建表（幂等）。pub 供 accounting 等业务模块测试初始化内存库使用。
pub fn create_tables(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS employees (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            employee_no TEXT UNIQUE NOT NULL,
            name TEXT NOT NULL,
            department TEXT,
            position TEXT,
            id_card TEXT,
            phone TEXT,
            bank_account TEXT,
            bank_name TEXT,
            hire_date TEXT,
            status TEXT DEFAULT 'active',
            base_salary REAL DEFAULT 0,
            position_salary REAL DEFAULT 0,
            performance_salary REAL DEFAULT 0,
            social_security_base REAL DEFAULT 0,
            housing_fund_base REAL DEFAULT 0,
            special_deduction REAL DEFAULT 0,
            remark TEXT,
            created_at TEXT,
            updated_at TEXT
        );

        CREATE TABLE IF NOT EXISTS attendance_records (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            salary_month TEXT NOT NULL,
            employee_no TEXT NOT NULL,
            name TEXT,
            expected_days REAL DEFAULT 0,
            actual_days REAL DEFAULT 0,
            late_count INTEGER DEFAULT 0,
            early_leave_count INTEGER DEFAULT 0,
            personal_leave_days REAL DEFAULT 0,
            sick_leave_days REAL DEFAULT 0,
            absent_days REAL DEFAULT 0,
            overtime_hours REAL DEFAULT 0,
            source_type TEXT,
            ocr_batch_id INTEGER,
            remark TEXT,
            created_at TEXT,
            updated_at TEXT
        );

        CREATE TABLE IF NOT EXISTS salary_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            rule_key TEXT UNIQUE NOT NULL,
            rule_name TEXT NOT NULL,
            rule_value REAL DEFAULT 0,
            rule_type TEXT,
            enabled INTEGER DEFAULT 1,
            remark TEXT
        );

        CREATE TABLE IF NOT EXISTS tax_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            min_amount REAL NOT NULL,
            max_amount REAL,
            tax_rate REAL NOT NULL,
            quick_deduction REAL NOT NULL
        );

        CREATE TABLE IF NOT EXISTS salary_monthly_results (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            salary_month TEXT NOT NULL,
            employee_no TEXT NOT NULL,
            name TEXT,
            department TEXT,
            base_salary REAL DEFAULT 0,
            position_salary REAL DEFAULT 0,
            performance_salary REAL DEFAULT 0,
            overtime_salary REAL DEFAULT 0,
            meal_allowance REAL DEFAULT 0,
            transport_allowance REAL DEFAULT 0,
            other_allowance REAL DEFAULT 0,
            gross_salary REAL DEFAULT 0,
            social_security_personal REAL DEFAULT 0,
            housing_fund_personal REAL DEFAULT 0,
            attendance_deduction REAL DEFAULT 0,
            tax_amount REAL DEFAULT 0,
            other_deduction REAL DEFAULT 0,
            net_salary REAL DEFAULT 0,
            status TEXT DEFAULT 'draft',
            locked INTEGER DEFAULT 0,
            remark TEXT,
            created_at TEXT,
            updated_at TEXT
        );

        CREATE TABLE IF NOT EXISTS ocr_batches (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            batch_name TEXT,
            salary_month TEXT,
            image_path TEXT,
            raw_text TEXT,
            parsed_json TEXT,
            status TEXT DEFAULT 'pending',
            created_at TEXT
        );

        CREATE TABLE IF NOT EXISTS operation_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            operation_type TEXT NOT NULL,
            description TEXT,
            operator TEXT,
            detail TEXT,
            created_at TEXT
        );

        CREATE TABLE IF NOT EXISTS app_settings (
            key TEXT PRIMARY KEY,
            value TEXT
        );

        CREATE TABLE IF NOT EXISTS punch_card_batches (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            salary_month TEXT NOT NULL,
            department TEXT,
            position TEXT,
            shift_type TEXT DEFAULT 'day',
            image_path TEXT,
            status TEXT DEFAULT 'pending',
            created_at TEXT
        );

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
            image_encrypted INTEGER NOT NULL DEFAULT 0,
            created_at TEXT,
            updated_at TEXT,
            FOREIGN KEY (employee_id) REFERENCES employees(id) ON DELETE SET NULL,
            FOREIGN KEY (expense_type_code) REFERENCES invoice_expense_types(code) ON DELETE SET NULL
        );

        DROP INDEX IF EXISTS idx_invoices_code_number;
        CREATE UNIQUE INDEX IF NOT EXISTS idx_invoices_code_number
            ON invoices(COALESCE(invoice_code, ''), invoice_number) WHERE status != 'void';
        CREATE INDEX IF NOT EXISTS idx_invoices_employee ON invoices(employee_id);
        CREATE INDEX IF NOT EXISTS idx_invoices_month ON invoices(belong_month);
        CREATE INDEX IF NOT EXISTS idx_invoices_expense_type ON invoices(expense_type_code);

        CREATE TABLE IF NOT EXISTS reimbursement_claims (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            claim_no TEXT UNIQUE NOT NULL,
            employee_id INTEGER,
            belong_month TEXT NOT NULL,
            title TEXT NOT NULL,
            total_amount REAL DEFAULT 0,
            invoice_count INTEGER DEFAULT 0,
            status TEXT DEFAULT 'draft',
            payment_status TEXT DEFAULT 'unpaid',
            payment_date TEXT,
            remark TEXT,
            created_at TEXT,
            updated_at TEXT,
            FOREIGN KEY (employee_id) REFERENCES employees(id) ON DELETE SET NULL
        );

        CREATE TABLE IF NOT EXISTS reimbursement_claim_invoices (
            claim_id INTEGER NOT NULL,
            invoice_id INTEGER NOT NULL,
            created_at TEXT,
            PRIMARY KEY (claim_id, invoice_id),
            FOREIGN KEY (claim_id) REFERENCES reimbursement_claims(id) ON DELETE CASCADE,
            FOREIGN KEY (invoice_id) REFERENCES invoices(id) ON DELETE RESTRICT
        );

        CREATE INDEX IF NOT EXISTS idx_reimbursement_claims_month ON reimbursement_claims(belong_month);
        CREATE INDEX IF NOT EXISTS idx_reimbursement_claims_employee ON reimbursement_claims(employee_id);
        CREATE INDEX IF NOT EXISTS idx_reimbursement_claims_status ON reimbursement_claims(status, payment_status);
        CREATE INDEX IF NOT EXISTS idx_reimbursement_claim_invoices_invoice ON reimbursement_claim_invoices(invoice_id);

        CREATE TABLE IF NOT EXISTS month_closes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            month TEXT UNIQUE NOT NULL,
            status TEXT NOT NULL DEFAULT 'open',
            summary_json TEXT,
            checks_json TEXT,
            closed_at TEXT,
            closed_by TEXT,
            reopened_at TEXT,
            reopen_reason TEXT,
            remark TEXT,
            created_at TEXT,
            updated_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_month_closes_status ON month_closes(status);

        CREATE TABLE IF NOT EXISTS payment_batches (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            batch_no TEXT UNIQUE NOT NULL,
            belong_month TEXT NOT NULL,
            batch_type TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'draft',
            total_amount REAL DEFAULT 0,
            item_count INTEGER DEFAULT 0,
            payment_date TEXT,
            remark TEXT,
            created_at TEXT,
            updated_at TEXT
        );

        CREATE TABLE IF NOT EXISTS payment_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            batch_id INTEGER NOT NULL,
            source_type TEXT NOT NULL,
            source_id INTEGER NOT NULL,
            employee_id INTEGER,
            employee_no TEXT,
            employee_name TEXT,
            bank_name TEXT,
            bank_account TEXT,
            amount REAL NOT NULL,
            status TEXT DEFAULT 'pending',
            remark TEXT,
            created_at TEXT,
            FOREIGN KEY (batch_id) REFERENCES payment_batches(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_payment_batches_month ON payment_batches(belong_month, batch_type, status);
        CREATE INDEX IF NOT EXISTS idx_payment_items_batch ON payment_items(batch_id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_payment_items_active_source
            ON payment_items(source_type, source_id)
            WHERE status != 'void';

        CREATE TABLE IF NOT EXISTS bank_transactions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            transaction_date TEXT NOT NULL,
            belong_month TEXT NOT NULL,
            summary TEXT,
            counterparty_name TEXT,
            counterparty_account TEXT,
            income_amount REAL DEFAULT 0,
            expense_amount REAL DEFAULT 0,
            balance REAL,
            status TEXT NOT NULL DEFAULT 'unmatched',
            ignore_reason TEXT,
            imported_file TEXT,
            raw_json TEXT,
            created_at TEXT,
            updated_at TEXT
        );

        CREATE TABLE IF NOT EXISTS bank_transaction_matches (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            transaction_id INTEGER NOT NULL,
            payment_batch_id INTEGER NOT NULL,
            match_score INTEGER DEFAULT 0,
            remark TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT,
            FOREIGN KEY (transaction_id) REFERENCES bank_transactions(id) ON DELETE CASCADE,
            FOREIGN KEY (payment_batch_id) REFERENCES payment_batches(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_bank_transactions_month ON bank_transactions(belong_month, status);
        CREATE INDEX IF NOT EXISTS idx_bank_transactions_date ON bank_transactions(transaction_date);
        -- 银行流水去重唯一索引由第七阶段迁移统一创建（含 fund_account_id 账户维度），
        -- 因该索引依赖迁移补列，不能在本批次直接建。
        CREATE UNIQUE INDEX IF NOT EXISTS idx_bank_matches_active_transaction
            ON bank_transaction_matches(transaction_id)
            WHERE status = 'active';
        CREATE UNIQUE INDEX IF NOT EXISTS idx_bank_matches_active_batch
            ON bank_transaction_matches(payment_batch_id)
            WHERE status = 'active';

        CREATE TABLE IF NOT EXISTS budgets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            month TEXT NOT NULL,
            department TEXT,
            expense_type_code TEXT,
            budget_amount REAL NOT NULL,
            remark TEXT,
            created_at TEXT,
            updated_at TEXT
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_budgets_scope
            ON budgets(month, COALESCE(department,''), COALESCE(expense_type_code,''));
        CREATE INDEX IF NOT EXISTS idx_budgets_month ON budgets(month);

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
                'reimbursement_payment','invoice_expense','bank_manual','period_close',
                'fund_document')),
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

        CREATE TABLE IF NOT EXISTS security_state (
          id INTEGER PRIMARY KEY CHECK (id = 1),
          password_hash TEXT NOT NULL,
          password_kek_salt TEXT NOT NULL,
          wrapped_dek_by_password TEXT NOT NULL,
          wrapped_dek_by_password_nonce TEXT NOT NULL,
          recovery_kek_salt TEXT NOT NULL,
          wrapped_dek_by_recovery TEXT NOT NULL,
          wrapped_dek_by_recovery_nonce TEXT NOT NULL,
          security_question TEXT NOT NULL,
          question_kek_salt TEXT NOT NULL,
          wrapped_dek_by_question TEXT NOT NULL,
          wrapped_dek_by_question_nonce TEXT NOT NULL,
          security_answer_hash TEXT NOT NULL,
          idle_timeout_seconds INTEGER NOT NULL DEFAULT 300,
          idle_lock_enabled INTEGER NOT NULL DEFAULT 1,
          sensitive_reveal_seconds INTEGER NOT NULL DEFAULT 300,
          failed_attempts INTEGER NOT NULL DEFAULT 0,
          lock_until TEXT,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS legacy_migration_state (
          id INTEGER PRIMARY KEY CHECK (id = 1),
          status TEXT NOT NULL DEFAULT 'pending',
          total_invoices INTEGER NOT NULL DEFAULT 0,
          processed_invoices INTEGER NOT NULL DEFAULT 0,
          token_migrated INTEGER NOT NULL DEFAULT 0,
          started_at TEXT,
          completed_at TEXT
        );
        ",
    )?;
    migrate_existing_schema(conn)?;
    migrate_stage7_schema(conn)?;

    Ok(())
}

/// 《小企业会计准则》预置一级科目表
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

/// 插入《小企业会计准则》预置科目（INSERT OR IGNORE，幂等），返回本次新插入行数
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

fn migrate_existing_schema(conn: &Connection) -> AppResult<()> {
    ensure_column(conn, "month_closes", "summary_json", "TEXT")?;
    ensure_column(conn, "month_closes", "checks_json", "TEXT")?;
    ensure_column(conn, "month_closes", "closed_at", "TEXT")?;
    ensure_column(conn, "month_closes", "closed_by", "TEXT")?;
    ensure_column(conn, "month_closes", "reopened_at", "TEXT")?;
    ensure_column(conn, "month_closes", "reopen_reason", "TEXT")?;
    ensure_column(conn, "month_closes", "remark", "TEXT")?;
    ensure_column(conn, "month_closes", "created_at", "TEXT")?;
    ensure_column(conn, "month_closes", "updated_at", "TEXT")?;
    ensure_column(
        conn,
        "salary_monthly_results",
        "payment_status",
        "TEXT DEFAULT 'unpaid'",
    )?;
    ensure_column(conn, "salary_monthly_results", "payment_date", "TEXT")?;
    ensure_column(
        conn,
        "salary_monthly_results",
        "payment_batch_id",
        "INTEGER",
    )?;
    ensure_column(conn, "reimbursement_claims", "payment_batch_id", "INTEGER")?;
    ensure_column(conn, "bank_transactions", "ignore_reason", "TEXT")?;
    ensure_column(
        conn,
        "tax_rules",
        "scope",
        "TEXT NOT NULL DEFAULT 'monthly'",
    )?;
    ensure_column(conn, "budgets", "remark", "TEXT")?;
    ensure_column(conn, "budgets", "created_at", "TEXT")?;
    ensure_column(conn, "budgets", "updated_at", "TEXT")?;
    // 第六阶段：工资结果增加社保/公积金单位部分
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
    // 兼容旧库：invoices 增加 image_encrypted
    ensure_column(
        conn,
        "invoices",
        "image_encrypted",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, column_type: &str) -> AppResult<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for existing in columns {
        if existing? == column {
            return Ok(());
        }
    }
    conn.execute_batch(&format!(
        "ALTER TABLE {table} ADD COLUMN {column} {column_type}"
    ))?;
    Ok(())
}

// ==================== 第七阶段（出纳运营闭环）schema 迁移 ====================

/// 资金账户允许挂接的总账科目（库存现金 / 银行存款 / 其他货币资金）
pub(crate) const STAGE7_FUND_GL_CODES: &[&str] = &["1001", "1002", "1012"];

/// 第七阶段资金领域新表清单（迁移后自检用）
const STAGE7_NEW_TABLES: &[&str] = &[
    "fund_accounts",
    "business_partners",
    "operator_profiles",
    "fund_documents",
    "approval_events",
    "business_attachments",
    "bank_reconciliation_allocations",
    "bank_reconciliation_periods",
    "advance_settlement_links",
];

/// 第七阶段 schema 迁移入口（幂等，可在新旧库上重复执行）：
/// 0. 重建 `vouchers` 表扩展 `source_type` 白名单（`'fund_document'`，spec 4.7），保留原 id、
///    分录外键关系与全部索引；新库 DDL 已含该类型时跳过；
/// 0b. 重建 `approval_events` 表扩展 `action` 白名单（`'unbatch'`，批次释放资金单，spec 5.3）；
/// 1. 建七张资金领域新表（资金账户/往来单位/操作人/资金单据/审批事件/业务附件/多对多核销/余额调节表）；
/// 2. 为凭证分录、付款批次、银行流水补可空 `fund_account_id`（历史数据保持 NULL 进待归集，不猜测归属）；
/// 3. 银行流水去重唯一索引重建为含账户维度，避免不同账户同日同金额流水误判重复；
/// 4. 迁移结束运行 `PRAGMA foreign_key_check`，发现悬空引用整体回滚；
/// 5. 迁移状态与待归集数量写入 app_settings（`stage7_migration_*` 键）。
///
/// 全程在单事务中执行，任一步失败整体回滚（含建表/加列/重建索引），不留半成品。
/// 注意：不在迁移中伪造默认资金账户，默认账户由归集向导确认后创建（spec 9.2）。
pub fn migrate_stage7_schema(conn: &Connection) -> AppResult<Stage7MigrationReport> {
    // vouchers 表重建必须在外键开关可切换的 autocommit 状态下进行（事务内切换 pragma 无效），
    // 自身仍包在独立事务中，失败整体回滚
    rebuild_vouchers_source_type_check(conn)?;
    // approval_events 重建放行 'unbatch'（无子表引用，无需切换外键开关）
    rebuild_approval_events_action_check(conn)?;
    run_migration_in_transaction(conn, |c| {
        create_stage7_tables(c)?;
        // 兼容迁移：三处资金辅助核算列（可空、无默认值），带外键引用
        ensure_column(
            c,
            "voucher_lines",
            "fund_account_id",
            "INTEGER REFERENCES fund_accounts(id)",
        )?;
        ensure_column(
            c,
            "payment_batches",
            "fund_account_id",
            "INTEGER REFERENCES fund_accounts(id)",
        )?;
        ensure_column(
            c,
            "bank_transactions",
            "fund_account_id",
            "INTEGER REFERENCES fund_accounts(id)",
        )?;
        // Task 14（spec 4.11）：借款核销方式与预计归还日（可空，历史数据保持 NULL 兼容）
        ensure_column(c, "fund_documents", "settlement_mode", "TEXT")?;
        ensure_column(c, "fund_documents", "due_date", "TEXT")?;
        rebuild_stage7_indexes(c)?;

        // 新表自检：防止部分建表被静默吞掉
        for table in STAGE7_NEW_TABLES {
            let exists: i64 = c.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                params![table],
                |r| r.get(0),
            )?;
            if exists == 0 {
                return Err(AppError::General(format!(
                    "第七阶段迁移异常：表 {table} 创建失败"
                )));
            }
        }

        // 旧银行匹配迁移为多对多核销（spec 4.9/9.4）：可唯一定位付款凭证资金行的旧匹配
        // 写入 allocation（legacy_match_id 幂等）；不可定位的进入迁移报告不静默丢失。
        // 在外层迁移事务内执行，整体回滚语义与 DDL 一致；归集向导补充账户后重跑可转换更多。
        migrate_legacy_bank_matches_in_tx(c)?;

        // 外键一致性校验：存量脏数据（悬空引用）视为升级阻断项，整体回滚
        let fk_errors = count_fk_check_violations(c)?;
        if fk_errors > 0 {
            return Err(AppError::General(format!(
                "第七阶段迁移中止：发现 {fk_errors} 处外键悬空引用（PRAGMA foreign_key_check），请先通过数据安全中心备份数据并修复后重试"
            )));
        }

        let mut report = build_stage7_report(c)?;
        record_stage7_state(c, &report)?;
        // 回填首次/本次迁移完成时间戳
        report.completed_at = get_setting(c, "stage7_migration_completed_at")?;
        Ok(report)
    })
}

/// 重建 `vouchers` 表扩展 `source_type` CHECK 白名单（`'fund_document'`，spec 4.7）。
///
/// 旧库的 CHECK 约束固化在表定义里，只能重建表放行新类型；新库 DDL 已含该类型时跳过（幂等）。
/// 重建在独立事务中进行：新建 `vouchers_new` → 按 id 原序复制全部行 → 删除旧表 → 改名回
/// `vouchers` → 重建原有三条索引，保留原 id、分录外键关系与索引定义。
///
/// SQLite 在 `foreign_keys=ON` 时 `DROP TABLE` 父表会触发隐式删除并级联清空 `voucher_lines`，
/// 因此重建期间临时关闭外键（事务内切换 pragma 无效，必须在 autocommit 下操作），
/// 提交前用 `PRAGMA foreign_key_check` 全量校验兜底——并非"关外键后无校验覆盖"。
/// 函数返回时恢复连接原有的外键开关状态。
fn rebuild_vouchers_source_type_check(conn: &Connection) -> AppResult<()> {
    let create_sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='vouchers'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(None);
    // 表不存在（最小测试库）或已是新定义时无需重建
    let Some(sql) = create_sql else {
        return Ok(());
    };
    if sql.contains("fund_document") {
        return Ok(());
    }
    let fk_before: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA foreign_keys=OFF;")?;
    let result = run_migration_in_transaction(conn, |c| {
        // 列清单必须与 create_tables 中 vouchers 定义一致；新增列时同步此处
        c.execute_batch(
            "
            CREATE TABLE vouchers_new (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                voucher_no TEXT UNIQUE NOT NULL,
                voucher_date TEXT NOT NULL,
                belong_month TEXT NOT NULL,
                source_type TEXT NOT NULL CHECK (source_type IN (
                    'salary_accrual','salary_payment','reimbursement_accrual',
                    'reimbursement_payment','invoice_expense','bank_manual','period_close',
                    'fund_document')),
                source_id INTEGER NOT NULL,
                total_amount REAL NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','void')),
                remark TEXT,
                created_at TEXT,
                updated_at TEXT
            );
            INSERT INTO vouchers_new
                (id, voucher_no, voucher_date, belong_month, source_type, source_id,
                 total_amount, status, remark, created_at, updated_at)
            SELECT id, voucher_no, voucher_date, belong_month, source_type, source_id,
                   total_amount, status, remark, created_at, updated_at
            FROM vouchers ORDER BY id;
            DROP TABLE vouchers;
            ALTER TABLE vouchers_new RENAME TO vouchers;
            CREATE INDEX IF NOT EXISTS idx_vouchers_month ON vouchers(belong_month, status);
            CREATE INDEX IF NOT EXISTS idx_vouchers_source ON vouchers(source_type, source_id);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_vouchers_source_active
                ON vouchers(source_type, source_id) WHERE status = 'active';
            ",
        )?;
        // 结构防漂移：重建后列清单必须与预期一致，防止后续演进只改一处
        let actual: String = {
            let mut stmt = c.prepare("PRAGMA table_info(vouchers)")?;
            let names = stmt
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            names.join(",")
        };
        let expected = "id,voucher_no,voucher_date,belong_month,source_type,source_id,\
                        total_amount,status,remark,created_at,updated_at";
        if actual != expected {
            return Err(AppError::General(format!(
                "vouchers 表重建异常：列清单与预期不一致（{actual}）"
            )));
        }
        // 提交前全量外键校验：悬空引用（含 vouchers 之外的表）视为升级阻断项，整体回滚
        let violations = count_fk_check_violations(c)?;
        if violations > 0 {
            return Err(AppError::General(format!(
                "vouchers 表重建中止：发现 {violations} 处外键悬空引用（PRAGMA foreign_key_check），请先通过数据安全中心备份数据并修复后重试"
            )));
        }
        Ok(())
    });
    conn.execute_batch(&format!(
        "PRAGMA foreign_keys={};",
        if fk_before != 0 { "ON" } else { "OFF" }
    ))?;
    result
}

/// 重建 `approval_events` 表扩展 `action` CHECK 白名单（`'unbatch'`，批次释放资金单，
/// spec 5.3）。旧库 CHECK 固化在表定义里，只能重建表放行新动作；新库 DDL 已含该动作时
/// 跳过（幂等）。按 id 原序复制全部行并重建索引；本表无子表引用（无级联风险），
/// 全程在单事务中执行，任一步失败整体回滚。
fn rebuild_approval_events_action_check(conn: &Connection) -> AppResult<()> {
    let create_sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='approval_events'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(None);
    // 表不存在（最小测试库）或已是新定义时无需重建
    let Some(sql) = create_sql else {
        return Ok(());
    };
    if sql.contains("unbatch") {
        return Ok(());
    }
    run_migration_in_transaction(conn, |c| {
        // 列清单必须与 create_tables 中 approval_events 定义一致；新增列时同步此处
        c.execute_batch(
            "
            CREATE TABLE approval_events_new (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_type TEXT NOT NULL CHECK (entity_type IN ('reimbursement_claim','fund_document')),
                entity_id INTEGER NOT NULL,
                action TEXT NOT NULL CHECK (action IN
                    ('submit','approve','reject','settle','void','reverse','withdraw','batch',
                     'unbatch')),
                from_status TEXT,
                to_status TEXT,
                operator_id INTEGER,
                comment TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (operator_id) REFERENCES operator_profiles(id)
            );
            INSERT INTO approval_events_new
                (id, entity_type, entity_id, action, from_status, to_status,
                 operator_id, comment, created_at)
            SELECT id, entity_type, entity_id, action, from_status, to_status,
                   operator_id, comment, created_at
            FROM approval_events ORDER BY id;
            DROP TABLE approval_events;
            ALTER TABLE approval_events_new RENAME TO approval_events;
            CREATE INDEX IF NOT EXISTS idx_approval_events_entity
                ON approval_events(entity_type, entity_id, id);
            ",
        )?;
        // 结构防漂移：重建后列清单必须与预期一致
        let actual: String = {
            let mut stmt = c.prepare("PRAGMA table_info(approval_events)")?;
            let names = stmt
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            names.join(",")
        };
        let expected = "id,entity_type,entity_id,action,from_status,to_status,\
                        operator_id,comment,created_at";
        if actual != expected {
            return Err(AppError::General(format!(
                "approval_events 表重建异常：列清单与预期不一致（{actual}）"
            )));
        }
        Ok(())
    })
}

/// 在单个事务中执行迁移步骤；任一步失败整体回滚，库保持迁移前状态。
/// SQLite 的 DDL（建表/加列/建索引）同样参与事务回滚，因此失败后无半成品。
pub(crate) fn run_migration_in_transaction<F, T>(conn: &Connection, steps: F) -> AppResult<T>
where
    F: FnOnce(&Connection) -> AppResult<T>,
{
    let tx = conn.unchecked_transaction()?;
    match steps(&tx) {
        Ok(value) => {
            tx.commit()?;
            Ok(value)
        }
        Err(err) => {
            // 显式回滚，保证失败场景不留任何残留
            let _ = tx.rollback();
            Err(err)
        }
    }
}

/// 建第七阶段资金领域六张新表与索引（全部 IF NOT EXISTS，幂等）
fn create_stage7_tables(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS fund_accounts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_code TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            account_type TEXT NOT NULL CHECK (account_type IN ('bank','cash','third_party')),
            bank_name TEXT,
            account_no TEXT,
            currency TEXT NOT NULL DEFAULT 'CNY' CHECK (currency = 'CNY'),
            gl_account_code TEXT NOT NULL,
            opening_date TEXT,
            opening_balance REAL NOT NULL DEFAULT 0,
            is_default INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            created_at TEXT,
            updated_at TEXT,
            FOREIGN KEY (gl_account_code) REFERENCES gl_accounts(code)
        );
        -- 同一账户类型最多一个默认账户
        CREATE UNIQUE INDEX IF NOT EXISTS idx_fund_accounts_default_per_type
            ON fund_accounts(account_type) WHERE is_default = 1;
        CREATE INDEX IF NOT EXISTS idx_fund_accounts_type ON fund_accounts(account_type, is_active);

        CREATE TABLE IF NOT EXISTS business_partners (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            partner_code TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            partner_type TEXT NOT NULL CHECK (partner_type IN ('supplier','customer','other')),
            tax_id TEXT,
            contact_person TEXT,
            phone TEXT,
            bank_name TEXT,
            bank_account TEXT,
            gl_account_code TEXT,
            status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','inactive')),
            remark TEXT,
            created_at TEXT,
            updated_at TEXT,
            FOREIGN KEY (gl_account_code) REFERENCES gl_accounts(code)
        );
        -- 名称 + 税号去重（税号可空，按空串归一化）
        CREATE UNIQUE INDEX IF NOT EXISTS idx_business_partners_name_tax
            ON business_partners(name, COALESCE(tax_id, ''));
        CREATE INDEX IF NOT EXISTS idx_business_partners_type ON business_partners(partner_type, status);

        CREATE TABLE IF NOT EXISTS operator_profiles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            role TEXT NOT NULL CHECK (role IN ('requester','approver','cashier','admin')),
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            created_at TEXT,
            updated_at TEXT
        );

        -- 通用资金单据（spec 4.4）：收款/付款/内部转账/员工借款/借款核销 + 冲正单
        -- 金额统一正数（方向由 document_type 决定）；状态只允许经领域层命令流转
        CREATE TABLE IF NOT EXISTS fund_documents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            document_no TEXT NOT NULL UNIQUE,
            document_type TEXT NOT NULL CHECK (document_type IN
                ('receipt','payment','transfer','advance','advance_settlement','reversal')),
            belong_month TEXT NOT NULL,
            document_date TEXT NOT NULL,
            amount REAL NOT NULL CHECK (amount > 0),
            summary TEXT NOT NULL,
            department TEXT,
            expense_type TEXT,
            remark TEXT,
            partner_id INTEGER REFERENCES business_partners(id),
            employee_id INTEGER REFERENCES employees(id),
            source_account_id INTEGER REFERENCES fund_accounts(id),
            target_account_id INTEGER REFERENCES fund_accounts(id),
            counter_account_code TEXT REFERENCES gl_accounts(code),
            status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN
                ('draft','submitted','approved','rejected','batched','settled','void','reversed')),
            payment_batch_id INTEGER REFERENCES payment_batches(id),
            reversal_of_id INTEGER REFERENCES fund_documents(id),
            submitted_by INTEGER REFERENCES operator_profiles(id),
            submitted_at TEXT,
            approved_by INTEGER REFERENCES operator_profiles(id),
            approved_at TEXT,
            settled_by INTEGER REFERENCES operator_profiles(id),
            settled_at TEXT,
            voided_by INTEGER REFERENCES operator_profiles(id),
            voided_at TEXT,
            created_by INTEGER REFERENCES operator_profiles(id),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            -- 内部转账两侧账户不得相同（金额相等于单金额自然成立）
            CHECK (source_account_id IS NULL OR target_account_id IS NULL
                   OR source_account_id != target_account_id)
        );
        CREATE INDEX IF NOT EXISTS idx_fund_documents_month
            ON fund_documents(belong_month, status);
        CREATE INDEX IF NOT EXISTS idx_fund_documents_partner ON fund_documents(partner_id);
        CREATE INDEX IF NOT EXISTS idx_fund_documents_employee ON fund_documents(employee_id);
        CREATE INDEX IF NOT EXISTS idx_fund_documents_reversal_of
            ON fund_documents(reversal_of_id);
        CREATE INDEX IF NOT EXISTS idx_fund_documents_batch ON fund_documents(payment_batch_id);

        -- 追加式审批事件（spec 4.5）：报销单与资金单据共用，不允许 UPDATE/DELETE。
        -- action 覆盖资金单状态机全部命令（withdraw=撤回、batch=进入付款批次）
        CREATE TABLE IF NOT EXISTS approval_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            entity_type TEXT NOT NULL CHECK (entity_type IN ('reimbursement_claim','fund_document')),
            entity_id INTEGER NOT NULL,
            action TEXT NOT NULL CHECK (action IN
                ('submit','approve','reject','settle','void','reverse','withdraw','batch',
                 'unbatch')),
            from_status TEXT,
            to_status TEXT,
            operator_id INTEGER,
            comment TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (operator_id) REFERENCES operator_profiles(id)
        );
        -- 实体维度轨迹查询（按 id 升序返回完整轨迹）
        CREATE INDEX IF NOT EXISTS idx_approval_events_entity
            ON approval_events(entity_type, entity_id, id);

        CREATE TABLE IF NOT EXISTS business_attachments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            entity_type TEXT NOT NULL,
            entity_id INTEGER NOT NULL,
            file_name TEXT NOT NULL,
            file_path TEXT NOT NULL UNIQUE,
            encrypted INTEGER NOT NULL DEFAULT 0,
            file_size INTEGER,
            belong_month TEXT,
            uploaded_by TEXT,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_business_attachments_entity
            ON business_attachments(entity_type, entity_id);

        -- 银行流水多对多核销（spec 4.9，Task 12）：流水 ↔ 带资金辅助核算的账面分录。
        -- 核销只是对账链接，不产生会计分录；取消核销置 cancelled 保留原记录可追溯；
        -- legacy_match_id 承接旧 bank_transaction_matches（spec 9.4），唯一索引保证迁移幂等。
        CREATE TABLE IF NOT EXISTS bank_reconciliation_allocations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            transaction_id INTEGER NOT NULL REFERENCES bank_transactions(id) ON DELETE CASCADE,
            voucher_line_id INTEGER NOT NULL REFERENCES voucher_lines(id),
            allocated_amount REAL NOT NULL CHECK (allocated_amount > 0),
            status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','cancelled')),
            match_method TEXT NOT NULL DEFAULT 'manual'
                CHECK (match_method IN ('auto','manual','migrated')),
            score INTEGER,
            remark TEXT,
            operator_name TEXT,
            legacy_match_id INTEGER,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_bank_alloc_tx
            ON bank_reconciliation_allocations(transaction_id, status);
        CREATE INDEX IF NOT EXISTS idx_bank_alloc_line
            ON bank_reconciliation_allocations(voucher_line_id, status);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_bank_alloc_legacy
            ON bank_reconciliation_allocations(legacy_match_id) WHERE legacy_match_id IS NOT NULL;

        -- 银行余额调节表快照（spec 4.10，Task 13）：账户+月份唯一；
        -- 重新生成即覆盖快照并回到 draft，确认后记录确认人与时间
        CREATE TABLE IF NOT EXISTS bank_reconciliation_periods (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            fund_account_id INTEGER NOT NULL REFERENCES fund_accounts(id),
            belong_month TEXT NOT NULL,
            statement_opening_balance REAL NOT NULL DEFAULT 0,
            statement_closing_balance REAL NOT NULL DEFAULT 0,
            statement_source TEXT NOT NULL DEFAULT 'derived',
            book_closing_balance REAL NOT NULL DEFAULT 0,
            outstanding_tx_amount REAL NOT NULL DEFAULT 0,
            outstanding_line_amount REAL NOT NULL DEFAULT 0,
            adjusted_book_balance REAL NOT NULL DEFAULT 0,
            adjusted_bank_balance REAL NOT NULL DEFAULT 0,
            difference REAL NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','confirmed')),
            detail_json TEXT,
            confirmed_by TEXT,
            confirmed_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(fund_account_id, belong_month)
        );
        CREATE INDEX IF NOT EXISTS idx_bank_recon_periods_account
            ON bank_reconciliation_periods(fund_account_id, belong_month);

        -- 员工借款核销关系（spec 4.11，Task 14）：借款单 ↔ 核销单多对多，
        -- 累计核销（active 合计）不得超过借款金额；取消核销置 cancelled 保留追溯
        CREATE TABLE IF NOT EXISTS advance_settlement_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            advance_id INTEGER NOT NULL REFERENCES fund_documents(id),
            settlement_id INTEGER NOT NULL REFERENCES fund_documents(id),
            allocated_amount REAL NOT NULL CHECK (allocated_amount > 0),
            status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','cancelled')),
            remark TEXT,
            created_by INTEGER REFERENCES operator_profiles(id),
            created_at TEXT NOT NULL,
            cancelled_by INTEGER REFERENCES operator_profiles(id),
            cancelled_at TEXT,
            cancel_reason TEXT,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_advance_links_advance
            ON advance_settlement_links(advance_id, status);
        CREATE INDEX IF NOT EXISTS idx_advance_links_settlement
            ON advance_settlement_links(settlement_id, status);
        ",
    )?;
    Ok(())
}

/// 重建涉及 fund_account_id 的索引：
/// 银行流水去重唯一索引加入账户维度（COALESCE(fund_account_id,0)，历史无账户按 0 归一），
/// 并为三处资金辅助列补查询索引。 DROP+CREATE 幂等：旧库替换旧定义；
/// 去重索引已是账户维度定义时跳过重建，避免每次启动都重刷唯一索引。
fn rebuild_stage7_indexes(conn: &Connection) -> AppResult<()> {
    let dedup_sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='index' AND name='idx_bank_transactions_dedup'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(None);
    if dedup_sql
        .map(|sql| !sql.contains("fund_account_id"))
        .unwrap_or(true)
    {
        conn.execute_batch(
            "
            DROP INDEX IF EXISTS idx_bank_transactions_dedup;
            CREATE UNIQUE INDEX IF NOT EXISTS idx_bank_transactions_dedup
                ON bank_transactions(transaction_date, COALESCE(summary,''), COALESCE(counterparty_name,''),
                                     COALESCE(counterparty_account,''), income_amount, expense_amount,
                                     COALESCE(balance,0), COALESCE(fund_account_id, 0));
            ",
        )?;
    }
    conn.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_voucher_lines_fund_account ON voucher_lines(fund_account_id);
        CREATE INDEX IF NOT EXISTS idx_payment_batches_fund_account ON payment_batches(fund_account_id);
        CREATE INDEX IF NOT EXISTS idx_bank_transactions_fund_account ON bank_transactions(fund_account_id);
        ",
    )?;
    Ok(())
}

/// 统计 PRAGMA foreign_key_check 违规行数（不依赖 foreign_keys pragma 开关）
fn count_fk_check_violations(conn: &Connection) -> AppResult<i64> {
    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    let mut rows = stmt.query([])?;
    let mut count = 0i64;
    while rows.next()?.is_some() {
        count += 1;
    }
    Ok(count)
}

/// 统计待归集数量：无资金账户的银行流水、付款批次与资金科目（1001/1002/1012）凭证分录。
/// 历史数据不做归属猜测（保持 NULL），仅计数供归集向导展示。
/// Task 10 口径修正（Task 2 挂账承接）：void 凭证分录与 void 付款批次不计入——已作废数据
/// 无资金意义且永远无法归集，计入会导致待归集计数永远无法清零。
pub(crate) fn build_stage7_report(conn: &Connection) -> AppResult<Stage7MigrationReport> {
    let unassigned_bank_transactions: i64 = conn.query_row(
        "SELECT COUNT(*) FROM bank_transactions WHERE fund_account_id IS NULL",
        [],
        |r| r.get(0),
    )?;
    let unassigned_payment_batches: i64 = conn.query_row(
        "SELECT COUNT(*) FROM payment_batches
         WHERE fund_account_id IS NULL AND status != 'void'",
        [],
        |r| r.get(0),
    )?;
    let fund_codes = STAGE7_FUND_GL_CODES.join("','");
    let unassigned_voucher_lines: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM voucher_lines vl
             JOIN vouchers v ON v.id = vl.voucher_id
             WHERE vl.fund_account_id IS NULL
               AND (vl.debit_amount > 0 OR vl.credit_amount > 0)
               AND vl.account_code IN ('{fund_codes}')
               AND v.status != 'void'"
        ),
        [],
        |r| r.get(0),
    )?;
    let pending_count =
        unassigned_bank_transactions + unassigned_payment_batches + unassigned_voucher_lines;
    Ok(Stage7MigrationReport {
        status: "done".to_string(),
        pending_count,
        unassigned_bank_transactions,
        unassigned_payment_batches,
        unassigned_voucher_lines,
        completed_at: None,
    })
}

/// 将迁移状态写入 app_settings：状态、待归集数量（总数 + 明细）、首次完成时间戳（重跑不覆盖）。
/// Task 10 起归集向导在归集事务内复用本函数刷新待归集计数，保持 `stage7_migration_*` 键实时。
pub(crate) fn record_stage7_state(
    conn: &Connection,
    report: &Stage7MigrationReport,
) -> AppResult<()> {
    set_setting(conn, "stage7_migration_status", &report.status)?;
    set_setting(
        conn,
        "stage7_migration_pending_count",
        &report.pending_count.to_string(),
    )?;
    set_setting(
        conn,
        "stage7_migration_unassigned_bank_transactions",
        &report.unassigned_bank_transactions.to_string(),
    )?;
    set_setting(
        conn,
        "stage7_migration_unassigned_payment_batches",
        &report.unassigned_payment_batches.to_string(),
    )?;
    set_setting(
        conn,
        "stage7_migration_unassigned_voucher_lines",
        &report.unassigned_voucher_lines.to_string(),
    )?;
    if get_setting(conn, "stage7_migration_completed_at")?.is_none() {
        set_setting(
            conn,
            "stage7_migration_completed_at",
            &Utc::now().to_rfc3339(),
        )?;
    }
    Ok(())
}

// ==================== Task 12：旧银行匹配迁移为多对多核销（spec 4.9/9.4） ====================

/// 旧 bank_transaction_matches 在迁移后只读保留一个版本周期：新引擎从不写入旧表，
/// 仅由本迁移单向把可转换的旧匹配写入 bank_reconciliation_allocations。
/// 迁移幂等（legacy_match_id 唯一索引），归集向导补充账户后可重跑转换更多历史匹配。
pub fn migrate_legacy_bank_matches(conn: &Connection) -> AppResult<LegacyBankMatchReport> {
    let tx = conn.unchecked_transaction()?;
    let report = migrate_legacy_bank_matches_in_tx(&tx)?;
    tx.commit()?;
    Ok(report)
}

/// 迁移核心（无自身事务，供启动迁移复用外层事务）：
/// 仅迁移 active 旧匹配；要求批次已付款且双方归集同一资金账户，且付款凭证资金行
/// （挂该账户、贷方、active）能唯一定位；金额取流水剩余可核销额（支出扣流水侧已核销）
/// 与分录剩余可核销额（贷方扣分录侧已核销）的较小值，防历史脏数据写出超额核销。
/// 无法定位的逐条记入报告不静默丢失；单行异常不阻断整体迁移。
pub fn migrate_legacy_bank_matches_in_tx(conn: &Connection) -> AppResult<LegacyBankMatchReport> {
    // 表可能尚不存在（极旧库在 stage7 建表前调用时由外层先建表；最小测试库直接跳过）
    let alloc_table: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='bank_reconciliation_allocations'",
        [],
        |r| r.get(0),
    )?;
    let match_table: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='bank_transaction_matches'",
        [],
        |r| r.get(0),
    )?;
    if alloc_table == 0 || match_table == 0 {
        return Ok(LegacyBankMatchReport::default());
    }

    let mut report = LegacyBankMatchReport::default();
    report.total = conn.query_row("SELECT COUNT(*) FROM bank_transaction_matches", [], |r| {
        r.get(0)
    })?;

    // 已迁移的旧匹配（幂等跳过）
    let migrated_ids: std::collections::HashSet<i64> = conn
        .prepare(
            "SELECT legacy_match_id FROM bank_reconciliation_allocations
             WHERE legacy_match_id IS NOT NULL",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<Result<std::collections::HashSet<_>, _>>()?;

    let mut stmt = conn.prepare(
        "SELECT id, transaction_id, payment_batch_id, match_score, remark
         FROM bank_transaction_matches WHERE status='active' ORDER BY id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<i32>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let now = Utc::now().to_rfc3339();
    for (mid, tx_id, batch_id, score, remark) in rows {
        report.active_total += 1;
        if migrated_ids.contains(&mid) {
            report.already_migrated += 1;
            continue;
        }
        let mut unconvert = |reason: String| {
            report.unconverted.push(LegacyBankMatchUnconverted {
                match_id: mid,
                transaction_id: tx_id,
                payment_batch_id: batch_id,
                reason,
            });
        };
        // 逐行容错：单条历史脏数据不阻断整体迁移（记录进报告由人工处理）
        let outcome = match (|| -> AppResult<Option<()>> {
            let tx_account: Option<i64> = conn
                .query_row(
                    "SELECT fund_account_id FROM bank_transactions WHERE id=?1",
                    params![tx_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(AppError::from)?
                .flatten();
            let Some(tx_account) = tx_account else {
                unconvert("流水不存在或未归集资金账户，请先完成历史归集".into());
                return Ok(None);
            };
            let (batch_status, batch_account, _batch_total): (String, Option<i64>, f64) = conn
                .query_row(
                    "SELECT status, fund_account_id, total_amount FROM payment_batches WHERE id=?1",
                    params![batch_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(AppError::from)?
                .ok_or_else(|| AppError::NotFound(format!("付款批次 {batch_id} 不存在")))?;
            if batch_status != "paid" {
                unconvert("付款批次未付款（或已作废），无对应付款凭证资金行".into());
                return Ok(None);
            }
            let Some(batch_account) = batch_account else {
                unconvert("付款批次未归集资金账户，请先完成历史归集后重跑迁移".into());
                return Ok(None);
            };
            if batch_account != tx_account {
                unconvert("流水与批次的资金账户不一致".into());
                return Ok(None);
            }

            // 付款凭证资金行：salary/reimbursement 批次级凭证 + general 单据结算凭证，
            // 取挂该账户、贷方、active 的分录；不唯一则无法定位
            let mut line_stmt = conn.prepare(
                "SELECT vl.id, vl.credit_amount FROM voucher_lines vl
                 JOIN vouchers v ON v.id = vl.voucher_id
                 WHERE v.status='active'
                   AND vl.fund_account_id=?1 AND vl.credit_amount > 0
                   AND ((v.source_type IN ('salary_payment','reimbursement_payment')
                         AND v.source_id=?2)
                     OR (v.source_type='fund_document' AND v.source_id IN
                         (SELECT id FROM fund_documents WHERE payment_batch_id=?2)))
                 ORDER BY vl.id",
            )?;
            let lines = line_stmt
                .query_map(params![batch_account, batch_id], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            drop(line_stmt);
            if lines.len() != 1 {
                unconvert(format!(
                    "付款凭证资金行不唯一（{0} 条），无法唯一定位核销分录",
                    lines.len()
                ));
                return Ok(None);
            }
            let (line_id, line_credit) = lines[0];

            let tx_expense: f64 = conn.query_row(
                "SELECT expense_amount FROM bank_transactions WHERE id=?1",
                params![tx_id],
                |r| r.get(0),
            )?;
            if tx_expense <= 0.0 {
                unconvert("流水非支出方向，无法对应付款凭证贷方资金行".into());
                return Ok(None);
            }
            let line_allocated: f64 = conn.query_row(
                "SELECT COALESCE(SUM(allocated_amount),0) FROM bank_reconciliation_allocations
                 WHERE voucher_line_id=?1 AND status='active'",
                params![line_id],
                |r| r.get(0),
            )?;
            // 流水侧已核销同样占用该流水余额：历史脏数据下旧匹配金额可能大于
            // 流水剩余额，迁移额必须取两侧剩余的较小值，否则会写出超流水余额的核销
            let tx_allocated: f64 = conn.query_row(
                "SELECT COALESCE(SUM(allocated_amount),0) FROM bank_reconciliation_allocations
                 WHERE transaction_id=?1 AND status='active'",
                params![tx_id],
                |r| r.get(0),
            )?;
            let amount = (tx_expense - tx_allocated).min(line_credit - line_allocated);
            if amount <= 0.005 {
                unconvert("可核销余额不足（流水或分录已在其他核销中耗尽）".into());
                return Ok(None);
            }

            // spec 9.4：legacy ID 保留进 remark，另存 legacy_match_id 列保证迁移幂等
            let mut final_remark = format!("旧匹配#{mid} 迁移");
            if let Some(old) = remark.as_deref().filter(|r| !r.trim().is_empty()) {
                final_remark.push_str(&format!("；{old}"));
            }
            conn.execute(
                "INSERT INTO bank_reconciliation_allocations
                    (transaction_id, voucher_line_id, allocated_amount, status, match_method,
                     score, remark, operator_name, legacy_match_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'active', 'migrated', ?4, ?5, 'migration', ?6, ?7, ?7)",
                params![tx_id, line_id, amount, score, final_remark, mid, now],
            )?;
            Ok(Some(()))
        })() {
            Ok(v) => v,
            Err(e) => {
                unconvert(format!("迁移异常：{e}"));
                None
            }
        };
        if outcome.is_some() {
            report.migrated += 1;
        }
    }
    Ok(report)
}

pub fn insert_default_data(conn: &Connection) -> AppResult<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM salary_rules", [], |row| row.get(0))?;

    if count == 0 {
        let default_rules = vec![
            ("late_penalty", "迟到扣款（每次）", 20.0, "attendance"),
            (
                "early_leave_penalty",
                "早退扣款（每次）",
                20.0,
                "attendance",
            ),
            ("personal_leave_rate", "事假扣款倍率", 1.0, "attendance"),
            ("sick_leave_rate", "病假扣款倍率", 0.5, "attendance"),
            ("absent_rate", "旷工扣款倍率", 2.0, "attendance"),
            ("overtime_rate", "加班工资倍率", 1.5, "attendance"),
            ("social_security_rate", "社保个人比例", 0.105, "insurance"),
            ("housing_fund_rate", "公积金个人比例", 0.12, "insurance"),
            ("tax_threshold", "个税起征点", 5000.0, "tax"),
            ("meal_allowance", "餐补（每月）", 0.0, "allowance"),
            ("transport_allowance", "交通补助（每月）", 0.0, "allowance"),
        ];

        for (key, name, value, rule_type) in &default_rules {
            conn.execute(
                "INSERT INTO salary_rules (rule_key, rule_name, rule_value, rule_type) VALUES (?1, ?2, ?3, ?4)",
                params![key, name, value, rule_type],
            )?;
        }
    }

    let tax_count: i64 = conn.query_row("SELECT COUNT(*) FROM tax_rules", [], |row| row.get(0))?;

    if tax_count == 0 {
        let default_tax = vec![
            (0.0, 3000.0, 0.03, 0.0),
            (3000.0, 12000.0, 0.10, 210.0),
            (12000.0, 25000.0, 0.20, 1410.0),
            (25000.0, 35000.0, 0.25, 2660.0),
            (35000.0, 55000.0, 0.30, 4410.0),
            (55000.0, 80000.0, 0.35, 7160.0),
            (80000.0, 999999999.0, 0.45, 15160.0),
        ];

        for (min, max, rate, deduction) in &default_tax {
            conn.execute(
                "INSERT INTO tax_rules (min_amount, max_amount, tax_rate, quick_deduction) VALUES (?1, ?2, ?3, ?4)",
                params![min, max, rate, deduction],
            )?;
        }
    }

    // 累计预扣率表（scope='cumulative'），用于个税累计预扣法
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

    let expense_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM invoice_expense_types", [], |row| {
            row.get(0)
        })?;

    if expense_count == 0 {
        let default_expense_types = vec![
            ("office", "办公费", 1),
            ("travel", "差旅费", 2),
            ("meal", "餐饮费", 3),
            ("transport", "交通费", 4),
            ("accommodation", "住宿费", 5),
            ("communication", "通讯费", 6),
            ("other", "其他", 99),
        ];

        for (code, name, sort_order) in &default_expense_types {
            conn.execute(
                "INSERT INTO invoice_expense_types (code, name, sort_order) VALUES (?1, ?2, ?3)",
                params![code, name, sort_order],
            )?;
        }
    }

    Ok(())
}

// ==================== Employee CRUD ====================

pub fn employee_no_exists(
    conn: &Connection,
    employee_no: &str,
    exclude_id: Option<i64>,
) -> AppResult<bool> {
    let employee_no = employee_no.trim();
    let count: i64 = if let Some(id) = exclude_id {
        conn.query_row(
            "SELECT COUNT(*) FROM employees WHERE LOWER(TRIM(employee_no)) = LOWER(?1) AND id != ?2",
            params![employee_no, id],
            |row| row.get(0),
        )?
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM employees WHERE LOWER(TRIM(employee_no)) = LOWER(?1)",
            params![employee_no],
            |row| row.get(0),
        )?
    };
    Ok(count > 0)
}

pub fn get_employees(conn: &Connection) -> AppResult<Vec<Employee>> {
    let mut stmt = conn.prepare(
        "SELECT id, employee_no, name, department, position, id_card, phone, bank_account, bank_name, hire_date, status, base_salary, position_salary, performance_salary, social_security_base, housing_fund_base, special_deduction, remark, created_at, updated_at FROM employees ORDER BY id"
    )?;

    let employees = stmt.query_map([], |row| {
        Ok(Employee {
            id: row.get(0)?,
            employee_no: row.get(1)?,
            name: row.get(2)?,
            department: row.get(3)?,
            position: row.get(4)?,
            id_card: row.get(5)?,
            phone: row.get(6)?,
            bank_account: row.get(7)?,
            bank_name: row.get(8)?,
            hire_date: row.get(9)?,
            status: row.get(10)?,
            base_salary: row.get(11)?,
            position_salary: row.get(12)?,
            performance_salary: row.get(13)?,
            social_security_base: row.get(14)?,
            housing_fund_base: row.get(15)?,
            special_deduction: row.get(16)?,
            remark: row.get(17)?,
            created_at: row.get(18)?,
            updated_at: row.get(19)?,
        })
    })?;

    employees
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)
}

pub fn get_employee(conn: &Connection, id: i64) -> AppResult<Employee> {
    conn.query_row(
        "SELECT id, employee_no, name, department, position, id_card, phone, bank_account, bank_name, hire_date, status, base_salary, position_salary, performance_salary, social_security_base, housing_fund_base, special_deduction, remark, created_at, updated_at FROM employees WHERE id = ?1",
        params![id],
        |row| {
            Ok(Employee {
                id: row.get(0)?,
                employee_no: row.get(1)?,
                name: row.get(2)?,
                department: row.get(3)?,
                position: row.get(4)?,
                id_card: row.get(5)?,
                phone: row.get(6)?,
                bank_account: row.get(7)?,
                bank_name: row.get(8)?,
                hire_date: row.get(9)?,
                status: row.get(10)?,
                base_salary: row.get(11)?,
                position_salary: row.get(12)?,
                performance_salary: row.get(13)?,
                social_security_base: row.get(14)?,
                housing_fund_base: row.get(15)?,
                special_deduction: row.get(16)?,
                remark: row.get(17)?,
                created_at: row.get(18)?,
                updated_at: row.get(19)?,
            })
        },
    ).map_err(|e| AppError::NotFound(format!("员工ID={id}未找到: {e}")))
}

pub fn create_employee(conn: &Connection, data: &EmployeeInput) -> AppResult<Employee> {
    let now = Utc::now().to_rfc3339();
    let employee_no = data.employee_no.trim();
    if employee_no.is_empty() {
        return Err(AppError::InvalidParam("工号必填".to_string()));
    }
    if employee_no_exists(conn, employee_no, None)? {
        return Err(AppError::InvalidParam(format!("工号 {employee_no} 已存在")));
    }
    let status = data.status.clone().unwrap_or_else(|| "active".to_string());
    let base_salary = data.base_salary.unwrap_or(0.0);
    let position_salary = data.position_salary.unwrap_or(0.0);
    let performance_salary = data.performance_salary.unwrap_or(0.0);
    let social_security_base = data.social_security_base.unwrap_or(0.0);
    let housing_fund_base = data.housing_fund_base.unwrap_or(0.0);
    let special_deduction = data.special_deduction.unwrap_or(0.0);

    conn.execute(
        "INSERT INTO employees (employee_no, name, department, position, id_card, phone, bank_account, bank_name, hire_date, status, base_salary, position_salary, performance_salary, social_security_base, housing_fund_base, special_deduction, remark, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        params![
            employee_no, data.name, data.department, data.position,
            data.id_card, data.phone, data.bank_account, data.bank_name,
            data.hire_date, status, base_salary, position_salary,
            performance_salary, social_security_base, housing_fund_base,
            special_deduction, data.remark, now, now
        ],
    )?;

    let id = conn.last_insert_rowid();
    get_employee(conn, id)
}

pub fn update_employee(conn: &Connection, id: i64, data: &EmployeeInput) -> AppResult<bool> {
    let now = Utc::now().to_rfc3339();
    // First get existing employee to merge updates
    let existing = get_employee(conn, id)?;
    let employee_no = data.employee_no.trim();
    if employee_no.is_empty() {
        return Err(AppError::InvalidParam("工号必填".to_string()));
    }
    if employee_no_exists(conn, employee_no, Some(id))? {
        return Err(AppError::InvalidParam(format!("工号 {employee_no} 已存在")));
    }

    let status = data.status.clone().unwrap_or(existing.status);
    let base_salary = data.base_salary.unwrap_or(existing.base_salary);
    let position_salary = data.position_salary.unwrap_or(existing.position_salary);
    let performance_salary = data
        .performance_salary
        .unwrap_or(existing.performance_salary);
    let social_security_base = data
        .social_security_base
        .unwrap_or(existing.social_security_base);
    let housing_fund_base = data.housing_fund_base.unwrap_or(existing.housing_fund_base);
    let special_deduction = data.special_deduction.unwrap_or(existing.special_deduction);

    let updated = conn.execute(
        "UPDATE employees SET employee_no=?1, name=?2, department=?3, position=?4, id_card=?5, phone=?6, bank_account=?7, bank_name=?8, hire_date=?9, status=?10, base_salary=?11, position_salary=?12, performance_salary=?13, social_security_base=?14, housing_fund_base=?15, special_deduction=?16, remark=?17, updated_at=?18 WHERE id=?19",
        params![
            employee_no, data.name, data.department, data.position,
            data.id_card, data.phone, data.bank_account, data.bank_name,
            data.hire_date, status, base_salary, position_salary,
            performance_salary, social_security_base, housing_fund_base,
            special_deduction, data.remark, now, id
        ],
    )?;

    Ok(updated > 0)
}

pub fn delete_employee(conn: &Connection, id: i64) -> AppResult<bool> {
    let deleted = conn.execute("DELETE FROM employees WHERE id=?1", params![id])?;
    Ok(deleted > 0)
}

pub fn search_employees(conn: &Connection, keyword: &str) -> AppResult<Vec<Employee>> {
    let pattern = format!("%{keyword}%");
    let mut stmt = conn.prepare(
        "SELECT id, employee_no, name, department, position, id_card, phone, bank_account, bank_name, hire_date, status, base_salary, position_salary, performance_salary, social_security_base, housing_fund_base, special_deduction, remark, created_at, updated_at FROM employees WHERE name LIKE ?1 OR employee_no LIKE ?1 OR department LIKE ?1 OR phone LIKE ?1 ORDER BY id"
    )?;

    let employees = stmt.query_map(params![pattern], |row| {
        Ok(Employee {
            id: row.get(0)?,
            employee_no: row.get(1)?,
            name: row.get(2)?,
            department: row.get(3)?,
            position: row.get(4)?,
            id_card: row.get(5)?,
            phone: row.get(6)?,
            bank_account: row.get(7)?,
            bank_name: row.get(8)?,
            hire_date: row.get(9)?,
            status: row.get(10)?,
            base_salary: row.get(11)?,
            position_salary: row.get(12)?,
            performance_salary: row.get(13)?,
            social_security_base: row.get(14)?,
            housing_fund_base: row.get(15)?,
            special_deduction: row.get(16)?,
            remark: row.get(17)?,
            created_at: row.get(18)?,
            updated_at: row.get(19)?,
        })
    })?;

    employees
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)
}

// ==================== Attendance CRUD ====================

pub fn get_attendance_records(conn: &Connection, month: &str) -> AppResult<Vec<AttendanceRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, salary_month, employee_no, name, expected_days, actual_days, late_count, early_leave_count, personal_leave_days, sick_leave_days, absent_days, overtime_hours, source_type, ocr_batch_id, remark, created_at, updated_at FROM attendance_records WHERE salary_month = ?1 ORDER BY id"
    )?;

    let records = stmt.query_map(params![month], |row| {
        Ok(AttendanceRecord {
            id: row.get(0)?,
            salary_month: row.get(1)?,
            employee_no: row.get(2)?,
            name: row.get(3)?,
            expected_days: row.get(4)?,
            actual_days: row.get(5)?,
            late_count: row.get(6)?,
            early_leave_count: row.get(7)?,
            personal_leave_days: row.get(8)?,
            sick_leave_days: row.get(9)?,
            absent_days: row.get(10)?,
            overtime_hours: row.get(11)?,
            source_type: row.get(12)?,
            ocr_batch_id: row.get(13)?,
            remark: row.get(14)?,
            created_at: row.get(15)?,
            updated_at: row.get(16)?,
        })
    })?;

    records
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)
}

fn get_attendance_record(conn: &Connection, id: i64) -> AppResult<AttendanceRecord> {
    conn.query_row(
        "SELECT id, salary_month, employee_no, name, expected_days, actual_days, late_count, early_leave_count, personal_leave_days, sick_leave_days, absent_days, overtime_hours, source_type, ocr_batch_id, remark, created_at, updated_at FROM attendance_records WHERE id = ?1",
        params![id],
        |row| {
            Ok(AttendanceRecord {
                id: row.get(0)?,
                salary_month: row.get(1)?,
                employee_no: row.get(2)?,
                name: row.get(3)?,
                expected_days: row.get(4)?,
                actual_days: row.get(5)?,
                late_count: row.get(6)?,
                early_leave_count: row.get(7)?,
                personal_leave_days: row.get(8)?,
                sick_leave_days: row.get(9)?,
                absent_days: row.get(10)?,
                overtime_hours: row.get(11)?,
                source_type: row.get(12)?,
                ocr_batch_id: row.get(13)?,
                remark: row.get(14)?,
                created_at: row.get(15)?,
                updated_at: row.get(16)?,
            })
        },
    )
    .map_err(|e| AppError::NotFound(format!("考勤记录ID={id}未找到: {e}")))
}

pub fn get_attendance_record_month(conn: &Connection, id: i64) -> AppResult<String> {
    Ok(get_attendance_record(conn, id)?.salary_month)
}

pub fn create_attendance_record(
    conn: &Connection,
    data: &AttendanceRecordInput,
) -> AppResult<AttendanceRecord> {
    if data.salary_month.trim().is_empty() {
        return Err(AppError::InvalidParam("考勤月份必填".into()));
    }
    if data.employee_no.trim().is_empty() {
        return Err(AppError::InvalidParam("员工工号必填".into()));
    }
    ensure_month_open(conn, &data.salary_month)?;

    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO attendance_records
         (salary_month, employee_no, name, expected_days, actual_days, late_count, early_leave_count,
          personal_leave_days, sick_leave_days, absent_days, overtime_hours, source_type,
          ocr_batch_id, remark, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            data.salary_month.trim(),
            data.employee_no.trim(),
            data.name,
            data.expected_days.unwrap_or(0.0),
            data.actual_days.unwrap_or(0.0),
            data.late_count.unwrap_or(0),
            data.early_leave_count.unwrap_or(0),
            data.personal_leave_days.unwrap_or(0.0),
            data.sick_leave_days.unwrap_or(0.0),
            data.absent_days.unwrap_or(0.0),
            data.overtime_hours.unwrap_or(0.0),
            data.source_type,
            data.ocr_batch_id,
            data.remark,
            now,
            now
        ],
    )?;

    get_attendance_record(conn, conn.last_insert_rowid())
}

pub fn upsert_attendance_record(
    conn: &Connection,
    data: &AttendanceRecordInput,
) -> AppResult<bool> {
    if let Some(id) = data.id {
        let old_month = get_attendance_record_month(conn, id)?;
        ensure_month_open(conn, &old_month)?;
    }
    ensure_month_open(conn, &data.salary_month)?;

    let now = Utc::now().to_rfc3339();
    let expected_days = data.expected_days.unwrap_or(0.0);
    let actual_days = data.actual_days.unwrap_or(0.0);
    let late_count = data.late_count.unwrap_or(0);
    let early_leave_count = data.early_leave_count.unwrap_or(0);
    let personal_leave_days = data.personal_leave_days.unwrap_or(0.0);
    let sick_leave_days = data.sick_leave_days.unwrap_or(0.0);
    let absent_days = data.absent_days.unwrap_or(0.0);
    let overtime_hours = data.overtime_hours.unwrap_or(0.0);

    if let Some(id) = data.id {
        conn.execute(
            "UPDATE attendance_records
             SET salary_month = CASE WHEN TRIM(?1) = '' THEN salary_month ELSE ?1 END,
                 employee_no = CASE WHEN TRIM(?2) = '' THEN employee_no ELSE ?2 END,
                 name = ?3,
                 expected_days = ?4,
                 actual_days = ?5,
                 late_count = ?6,
                 early_leave_count = ?7,
                 personal_leave_days = ?8,
                 sick_leave_days = ?9,
                 absent_days = ?10,
                 overtime_hours = ?11,
                 source_type = COALESCE(?12, source_type),
                 ocr_batch_id = COALESCE(?13, ocr_batch_id),
                 remark = ?14,
                 updated_at = ?15
             WHERE id = ?16",
            params![
                data.salary_month,
                data.employee_no,
                data.name,
                expected_days,
                actual_days,
                late_count,
                early_leave_count,
                personal_leave_days,
                sick_leave_days,
                absent_days,
                overtime_hours,
                data.source_type,
                data.ocr_batch_id,
                data.remark,
                now,
                id
            ],
        )?;
    } else {
        conn.execute(
            "INSERT INTO attendance_records (salary_month, employee_no, name, expected_days, actual_days, late_count, early_leave_count, personal_leave_days, sick_leave_days, absent_days, overtime_hours, source_type, ocr_batch_id, remark, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                data.salary_month, data.employee_no, data.name, expected_days,
                actual_days, late_count, early_leave_count, personal_leave_days,
                sick_leave_days, absent_days, overtime_hours, data.source_type,
                data.ocr_batch_id, data.remark, now, now
            ],
        )?;
    }

    Ok(true)
}

pub fn update_attendance_record(
    conn: &Connection,
    id: i64,
    data: &AttendanceRecordInput,
) -> AppResult<bool> {
    let old_month = get_attendance_record_month(conn, id)?;
    ensure_month_open(conn, &old_month)?;
    if !data.salary_month.trim().is_empty() {
        ensure_month_open(conn, &data.salary_month)?;
    }

    let now = Utc::now().to_rfc3339();
    let expected_days = data.expected_days.unwrap_or(0.0);
    let actual_days = data.actual_days.unwrap_or(0.0);
    let late_count = data.late_count.unwrap_or(0);
    let early_leave_count = data.early_leave_count.unwrap_or(0);
    let personal_leave_days = data.personal_leave_days.unwrap_or(0.0);
    let sick_leave_days = data.sick_leave_days.unwrap_or(0.0);
    let absent_days = data.absent_days.unwrap_or(0.0);
    let overtime_hours = data.overtime_hours.unwrap_or(0.0);

    let updated = conn.execute(
        "UPDATE attendance_records
         SET salary_month = CASE WHEN TRIM(?1) = '' THEN salary_month ELSE ?1 END,
             employee_no = CASE WHEN TRIM(?2) = '' THEN employee_no ELSE ?2 END,
             name = ?3,
             expected_days = ?4,
             actual_days = ?5,
             late_count = ?6,
             early_leave_count = ?7,
             personal_leave_days = ?8,
             sick_leave_days = ?9,
             absent_days = ?10,
             overtime_hours = ?11,
             source_type = COALESCE(?12, source_type),
             ocr_batch_id = COALESCE(?13, ocr_batch_id),
             remark = ?14,
             updated_at = ?15
         WHERE id = ?16",
        params![
            data.salary_month,
            data.employee_no,
            data.name,
            expected_days,
            actual_days,
            late_count,
            early_leave_count,
            personal_leave_days,
            sick_leave_days,
            absent_days,
            overtime_hours,
            data.source_type,
            data.ocr_batch_id,
            data.remark,
            now,
            id
        ],
    )?;

    Ok(updated > 0)
}

pub fn delete_attendance_record(conn: &Connection, id: i64) -> AppResult<bool> {
    let old_month = match get_attendance_record_month(conn, id) {
        Ok(month) => month,
        Err(AppError::NotFound(_)) => return Ok(false),
        Err(e) => return Err(e),
    };
    ensure_month_open(conn, &old_month)?;
    let updated = conn.execute("DELETE FROM attendance_records WHERE id = ?1", params![id])?;
    Ok(updated > 0)
}

// ==================== Salary Rules CRUD ====================

pub fn get_salary_rules(conn: &Connection) -> AppResult<Vec<SalaryRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, rule_key, rule_name, rule_value, rule_type, enabled, remark FROM salary_rules ORDER BY id"
    )?;

    let rules = stmt.query_map([], |row| {
        Ok(SalaryRule {
            id: row.get(0)?,
            rule_key: row.get(1)?,
            rule_name: row.get(2)?,
            rule_value: row.get(3)?,
            rule_type: row.get(4)?,
            enabled: row.get(5)?,
            remark: row.get(6)?,
        })
    })?;

    rules.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn update_salary_rule(conn: &Connection, id: i64, value: f64) -> AppResult<bool> {
    let updated = conn.execute(
        "UPDATE salary_rules SET rule_value = ?1 WHERE id = ?2",
        params![value, id],
    )?;
    Ok(updated > 0)
}

pub fn get_rule_value(conn: &Connection, key: &str) -> AppResult<f64> {
    let value: f64 = conn.query_row(
        "SELECT rule_value FROM salary_rules WHERE rule_key = ?1 AND enabled = 1",
        params![key],
        |row| row.get(0),
    )?;
    Ok(value)
}

// ==================== Tax Rules CRUD ====================

pub fn get_tax_rules(conn: &Connection) -> AppResult<Vec<TaxRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, min_amount, max_amount, tax_rate, quick_deduction FROM tax_rules ORDER BY min_amount"
    )?;

    let rules = stmt.query_map([], |row| {
        Ok(TaxRule {
            id: row.get(0)?,
            min_amount: row.get(1)?,
            max_amount: row.get(2)?,
            tax_rate: row.get(3)?,
            quick_deduction: row.get(4)?,
        })
    })?;

    rules.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

/// 累计预扣率表（scope='cumulative'），供个税累计预扣法使用
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

pub fn update_tax_rule(conn: &Connection, id: i64, data: &TaxRuleInput) -> AppResult<bool> {
    let existing: TaxRule = conn.query_row(
        "SELECT id, min_amount, max_amount, tax_rate, quick_deduction FROM tax_rules WHERE id = ?1",
        params![id],
        |row| {
            Ok(TaxRule {
                id: row.get(0)?,
                min_amount: row.get(1)?,
                max_amount: row.get(2)?,
                tax_rate: row.get(3)?,
                quick_deduction: row.get(4)?,
            })
        },
    ).map_err(|e| AppError::NotFound(format!("税率规则ID={id}未找到: {e}")))?;

    let min_amount = data.min_amount.unwrap_or(existing.min_amount);
    let max_amount = data.max_amount.or(existing.max_amount);
    let tax_rate = data.tax_rate.unwrap_or(existing.tax_rate);
    let quick_deduction = data.quick_deduction.unwrap_or(existing.quick_deduction);

    let updated = conn.execute(
        "UPDATE tax_rules SET min_amount=?1, max_amount=?2, tax_rate=?3, quick_deduction=?4 WHERE id=?5",
        params![min_amount, max_amount, tax_rate, quick_deduction, id],
    )?;

    Ok(updated > 0)
}

/// 月度税率表计算（备用）：当前工资计算走累计预扣法，此函数保留供月度口径核对。
#[allow(dead_code)]
pub fn calculate_tax(conn: &Connection, taxable_income: f64) -> AppResult<f64> {
    if taxable_income <= 0.0 {
        return Ok(0.0);
    }

    let rules = get_tax_rules(conn)?;
    for rule in &rules {
        let max = rule.max_amount.unwrap_or(f64::MAX);
        if taxable_income > rule.min_amount && taxable_income <= max {
            let tax = taxable_income * rule.tax_rate - rule.quick_deduction;
            return Ok(tax.max(0.0));
        }
    }

    Ok(0.0)
}

// ==================== Salary Results CRUD ====================

/// 个税年度汇总（第六阶段 Task 10）：按员工聚合指定年度非作废工资记录，
/// 用累计预扣率表计算年度应预扣额，差额 = 应预扣 - 已预扣（负数为多缴）。
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

pub fn save_salary_result(conn: &Connection, result: &SalaryResult) -> AppResult<()> {
    ensure_month_open(conn, &result.salary_month)?;
    let now = Utc::now().to_rfc3339();

    // Try update existing, insert if not found
    let existing = conn.query_row(
        "SELECT id FROM salary_monthly_results WHERE salary_month = ?1 AND employee_no = ?2",
        params![result.salary_month, result.employee_no],
        |row| row.get::<_, i64>(0),
    );

    match existing {
        Ok(existing_id) => {
            let locked: i64 = conn.query_row(
                "SELECT locked FROM salary_monthly_results WHERE id = ?1",
                params![existing_id],
                |row| row.get(0),
            )?;
            if locked == 1 {
                return Err(AppError::InvalidParam(
                    "工资结果已锁定，请先解锁再修改".into(),
                ));
            }
            if active_payment_item_exists(conn, "salary_result", existing_id)? {
                return Err(AppError::InvalidParam(
                    "工资结果已纳入付款批次，不能重新计算或覆盖".into(),
                ));
            }
            conn.execute(
                "UPDATE salary_monthly_results SET name=?1, department=?2, base_salary=?3, position_salary=?4, performance_salary=?5, overtime_salary=?6, meal_allowance=?7, transport_allowance=?8, other_allowance=?9, gross_salary=?10, social_security_personal=?11, housing_fund_personal=?12, attendance_deduction=?13, tax_amount=?14, other_deduction=?15, net_salary=?16, status=?17, remark=?18, updated_at=?19, social_security_employer=?20, housing_fund_employer=?21 WHERE id=?22",
                params![
                    result.name, result.department, result.base_salary, result.position_salary,
                    result.performance_salary, result.overtime_salary, result.meal_allowance,
                    result.transport_allowance, result.other_allowance, result.gross_salary,
                    result.social_security_personal, result.housing_fund_personal,
                    result.attendance_deduction, result.tax_amount, result.other_deduction,
                    result.net_salary, result.status, result.remark, now,
                    result.social_security_employer, result.housing_fund_employer, existing_id
                ],
            )?;
        }
        Err(_) => {
            conn.execute(
                "INSERT INTO salary_monthly_results (salary_month, employee_no, name, department, base_salary, position_salary, performance_salary, overtime_salary, meal_allowance, transport_allowance, other_allowance, gross_salary, social_security_personal, housing_fund_personal, attendance_deduction, tax_amount, other_deduction, net_salary, status, locked, remark, created_at, updated_at, social_security_employer, housing_fund_employer)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, 0, ?20, ?21, ?22, ?23, ?24)",
                params![
                    result.salary_month, result.employee_no, result.name, result.department,
                    result.base_salary, result.position_salary, result.performance_salary,
                    result.overtime_salary, result.meal_allowance, result.transport_allowance,
                    result.other_allowance, result.gross_salary, result.social_security_personal,
                    result.housing_fund_personal, result.attendance_deduction, result.tax_amount,
                    result.other_deduction, result.net_salary, result.status, result.remark, now, now,
                    result.social_security_employer, result.housing_fund_employer
                ],
            )?;
        }
    }

    Ok(())
}

pub fn get_salary_results(conn: &Connection, month: &str) -> AppResult<Vec<SalaryResult>> {
    let mut stmt = conn.prepare(
        "SELECT id, salary_month, employee_no, name, department, base_salary, position_salary, performance_salary, overtime_salary, meal_allowance, transport_allowance, other_allowance, gross_salary, social_security_personal, housing_fund_personal, attendance_deduction, tax_amount, other_deduction, net_salary, social_security_employer, housing_fund_employer, status, locked, remark, created_at, updated_at FROM salary_monthly_results WHERE salary_month = ?1 ORDER BY id"
    )?;

    let results = stmt.query_map(params![month], |row| {
        Ok(SalaryResult {
            id: row.get(0)?,
            salary_month: row.get(1)?,
            employee_no: row.get(2)?,
            name: row.get(3)?,
            department: row.get(4)?,
            base_salary: row.get(5)?,
            position_salary: row.get(6)?,
            performance_salary: row.get(7)?,
            overtime_salary: row.get(8)?,
            meal_allowance: row.get(9)?,
            transport_allowance: row.get(10)?,
            other_allowance: row.get(11)?,
            gross_salary: row.get(12)?,
            social_security_personal: row.get(13)?,
            housing_fund_personal: row.get(14)?,
            attendance_deduction: row.get(15)?,
            tax_amount: row.get(16)?,
            other_deduction: row.get(17)?,
            net_salary: row.get(18)?,
            social_security_employer: row.get(19)?,
            housing_fund_employer: row.get(20)?,
            status: row.get(21)?,
            locked: row.get(22)?,
            remark: row.get(23)?,
            created_at: row.get(24)?,
            updated_at: row.get(25)?,
        })
    })?;

    results
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)
}

pub fn get_salary_result_month(conn: &Connection, id: i64) -> AppResult<String> {
    conn.query_row(
        "SELECT salary_month FROM salary_monthly_results WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )
    .map_err(|e| AppError::NotFound(format!("工资结果ID={id}未找到: {e}")))
}

pub fn get_salary_result_by_employee(
    conn: &Connection,
    month: &str,
    employee_no: &str,
) -> AppResult<SalaryResult> {
    conn.query_row(
        "SELECT id, salary_month, employee_no, name, department, base_salary, position_salary, performance_salary, overtime_salary, meal_allowance, transport_allowance, other_allowance, gross_salary, social_security_personal, housing_fund_personal, attendance_deduction, tax_amount, other_deduction, net_salary, social_security_employer, housing_fund_employer, status, locked, remark, created_at, updated_at FROM salary_monthly_results WHERE salary_month = ?1 AND employee_no = ?2",
        params![month, employee_no],
        |row| {
            Ok(SalaryResult {
                id: row.get(0)?,
                salary_month: row.get(1)?,
                employee_no: row.get(2)?,
                name: row.get(3)?,
                department: row.get(4)?,
                base_salary: row.get(5)?,
                position_salary: row.get(6)?,
                performance_salary: row.get(7)?,
                overtime_salary: row.get(8)?,
                meal_allowance: row.get(9)?,
                transport_allowance: row.get(10)?,
                other_allowance: row.get(11)?,
                gross_salary: row.get(12)?,
                social_security_personal: row.get(13)?,
                housing_fund_personal: row.get(14)?,
                attendance_deduction: row.get(15)?,
                tax_amount: row.get(16)?,
                other_deduction: row.get(17)?,
                net_salary: row.get(18)?,
                social_security_employer: row.get(19)?,
            housing_fund_employer: row.get(20)?,
            status: row.get(21)?,
            locked: row.get(22)?,
            remark: row.get(23)?,
            created_at: row.get(24)?,
            updated_at: row.get(25)?,
            })
        },
    ).map_err(|e| AppError::NotFound(format!("工资结果未找到: {e}")))
}

pub fn update_salary_result(
    conn: &Connection,
    id: i64,
    data: &SalaryResultUpdate,
) -> AppResult<bool> {
    let month = get_salary_result_month(conn, id)?;
    ensure_month_open(conn, &month)?;
    let locked: i64 = conn.query_row(
        "SELECT locked FROM salary_monthly_results WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    if locked == 1 {
        return Err(AppError::InvalidParam(
            "工资结果已锁定，请先解锁再修改".into(),
        ));
    }
    if active_payment_item_exists(conn, "salary_result", id)? {
        return Err(AppError::InvalidParam(
            "工资结果已纳入付款批次，不能直接调整".into(),
        ));
    }

    let now = Utc::now().to_rfc3339();
    let mut updates = Vec::new();
    let mut param_idx = 1;
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(v) = data.overtime_salary {
        updates.push(format!("overtime_salary = ?{param_idx}"));
        param_values.push(Box::new(v));
        param_idx += 1;
    }
    if let Some(v) = data.meal_allowance {
        updates.push(format!("meal_allowance = ?{param_idx}"));
        param_values.push(Box::new(v));
        param_idx += 1;
    }
    if let Some(v) = data.transport_allowance {
        updates.push(format!("transport_allowance = ?{param_idx}"));
        param_values.push(Box::new(v));
        param_idx += 1;
    }
    if let Some(v) = data.other_allowance {
        updates.push(format!("other_allowance = ?{param_idx}"));
        param_values.push(Box::new(v));
        param_idx += 1;
    }
    if let Some(v) = data.other_deduction {
        updates.push(format!("other_deduction = ?{param_idx}"));
        param_values.push(Box::new(v));
        param_idx += 1;
    }
    if let Some(v) = &data.remark {
        updates.push(format!("remark = ?{param_idx}"));
        param_values.push(Box::new(v.clone()));
        param_idx += 1;
    }

    if updates.is_empty() {
        return Ok(true);
    }

    // Recalculate gross and net
    updates.push(format!("gross_salary = base_salary + position_salary + performance_salary + overtime_salary + meal_allowance + transport_allowance + other_allowance"));
    updates.push(format!("net_salary = gross_salary - social_security_personal - housing_fund_personal - attendance_deduction - tax_amount - other_deduction"));
    updates.push(format!("updated_at = ?{param_idx}"));
    param_values.push(Box::new(now));
    param_idx += 1;

    updates.push(format!("id = id"));
    param_values.push(Box::new(id));

    let sql = format!(
        "UPDATE salary_monthly_results SET {} WHERE id = ?{}",
        updates.join(", "),
        param_idx
    );

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let updated = conn.execute(&sql, params_refs.as_slice())?;

    Ok(updated > 0)
}

pub fn lock_salary_results(conn: &Connection, month: &str) -> AppResult<bool> {
    ensure_month_open(conn, month)?;
    // 锁定 UPDATE 与计提凭证生成放在同一事务：凭证生成失败时锁定一并回滚
    let tx = conn.unchecked_transaction()?;
    let updated = tx.execute(
        "UPDATE salary_monthly_results SET locked = 1, status = 'locked', updated_at = ?1 WHERE salary_month = ?2 AND locked = 0",
        params![Utc::now().to_rfc3339(), month],
    )?;
    if updated > 0 {
        crate::accounting::generate_salary_accrual_vouchers(&tx, month)?;
    }
    tx.commit()?;
    Ok(updated > 0)
}

pub fn unlock_salary_results(conn: &Connection, month: &str) -> AppResult<usize> {
    ensure_month_open(conn, month)?;
    // 解锁 UPDATE 与计提凭证作废放在同一事务：任一失败整体回滚
    let tx = conn.unchecked_transaction()?;
    let updated = tx.execute(
        "UPDATE salary_monthly_results SET locked = 0, status = 'reviewed', updated_at = ?1 WHERE salary_month = ?2 AND locked = 1",
        params![Utc::now().to_rfc3339(), month],
    )?;
    if updated == 0 {
        return Err(AppError::InvalidParam("该月没有已锁定的工资结果".into()));
    }
    let voided = crate::accounting::void_salary_accrual_vouchers(&tx, month)?;
    tx.commit()?;
    Ok(voided)
}

pub fn review_salary_results(conn: &Connection, month: &str) -> AppResult<bool> {
    ensure_month_open(conn, month)?;
    let updated = conn.execute(
        "UPDATE salary_monthly_results SET status = 'reviewed', updated_at = ?1 WHERE salary_month = ?2 AND locked = 0",
        params![Utc::now().to_rfc3339(), month],
    )?;
    Ok(updated > 0)
}

// ==================== OCR ====================

pub fn save_ocr_batch(conn: &Connection, batch: &OcrBatch) -> AppResult<i64> {
    if let Some(month) = batch.salary_month.as_deref() {
        ensure_month_open(conn, month)?;
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO ocr_batches (batch_name, salary_month, image_path, raw_text, parsed_json, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![batch.batch_name, batch.salary_month, batch.image_path, batch.raw_text, batch.parsed_json, batch.status, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_ocr_batch_status(conn: &Connection, id: i64, status: &str) -> AppResult<bool> {
    let updated = conn.execute(
        "UPDATE ocr_batches SET status = ?1 WHERE id = ?2",
        params![status, id],
    )?;
    Ok(updated > 0)
}

pub fn get_ocr_batches(conn: &Connection, month: &str) -> AppResult<Vec<OcrBatch>> {
    let mut stmt = conn.prepare(
        "SELECT id, batch_name, salary_month, image_path, raw_text, parsed_json, status, created_at FROM ocr_batches WHERE salary_month = ?1 ORDER BY id DESC"
    )?;

    let batches = stmt.query_map(params![month], |row| {
        Ok(OcrBatch {
            id: row.get(0)?,
            batch_name: row.get(1)?,
            salary_month: row.get(2)?,
            image_path: row.get(3)?,
            raw_text: row.get(4)?,
            parsed_json: row.get(5)?,
            status: row.get(6)?,
            created_at: row.get(7)?,
        })
    })?;

    batches
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)
}

// ==================== Operation Logs ====================

pub fn log_operation(
    conn: &Connection,
    op_type: &str,
    description: &str,
    operator: &str,
    detail: Option<&str>,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO operation_logs (operation_type, description, operator, detail, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![op_type, description, operator, detail, now],
    )?;
    Ok(())
}

pub fn query_operation_logs(
    conn: &Connection,
    q: &OperationLogQuery,
) -> AppResult<Vec<OperationLog>> {
    let mut where_clauses: Vec<String> = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(op_type) = q.operation_type.as_ref().filter(|v| !v.trim().is_empty()) {
        where_clauses.push(format!("operation_type = ?{idx}"));
        params_vec.push(Box::new(op_type.clone()));
        idx += 1;
    }
    if let Some(keyword) = q.keyword.as_ref().filter(|v| !v.trim().is_empty()) {
        where_clauses.push(format!(
            "(operation_type LIKE ?{idx} OR description LIKE ?{idx} OR detail LIKE ?{idx} OR operator LIKE ?{idx})"
        ));
        params_vec.push(Box::new(format!("%{keyword}%")));
        idx += 1;
    }
    if let Some(start_date) = q.start_date.as_ref().filter(|v| !v.trim().is_empty()) {
        where_clauses.push(format!("created_at >= ?{idx}"));
        params_vec.push(Box::new(start_date.clone()));
        idx += 1;
    }
    if let Some(end_date) = q.end_date.as_ref().filter(|v| !v.trim().is_empty()) {
        where_clauses.push(format!("created_at <= ?{idx}"));
        params_vec.push(Box::new(end_date.clone()));
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };
    let limit = q.limit.unwrap_or(200).clamp(20, 1000);
    let sql = format!(
        "SELECT id, operation_type, description, operator, detail, created_at \
         FROM operation_logs {where_sql} ORDER BY id DESC LIMIT {limit}"
    );

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(OperationLog {
            id: row.get(0)?,
            operation_type: row.get(1)?,
            description: row.get(2)?,
            operator: row.get(3)?,
            detail: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

// ==================== Dashboard ====================

pub fn get_dashboard_summary(conn: &Connection, month: &str) -> AppResult<DashboardSummary> {
    let employee_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM employees", [], |row| row.get(0))?;

    let active_employee_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM employees WHERE status = 'active'",
        [],
        |row| row.get(0),
    )?;

    let calculated_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month = ?1",
        params![month],
        |row| row.get(0),
    )?;

    let locked_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month = ?1 AND locked = 1",
        params![month],
        |row| row.get(0),
    )?;

    let total_gross_salary: f64 = conn.query_row(
        "SELECT COALESCE(SUM(gross_salary), 0) FROM salary_monthly_results WHERE salary_month = ?1",
        params![month],
        |row| row.get(0),
    ).unwrap_or(0.0);

    let total_net_salary: f64 = conn.query_row(
        "SELECT COALESCE(SUM(net_salary), 0) FROM salary_monthly_results WHERE salary_month = ?1",
        params![month],
        |row| row.get(0),
    ).unwrap_or(0.0);

    let total_social_security: f64 = conn.query_row(
        "SELECT COALESCE(SUM(social_security_personal), 0) FROM salary_monthly_results WHERE salary_month = ?1",
        params![month],
        |row| row.get(0),
    ).unwrap_or(0.0);

    let total_housing_fund: f64 = conn.query_row(
        "SELECT COALESCE(SUM(housing_fund_personal), 0) FROM salary_monthly_results WHERE salary_month = ?1",
        params![month],
        |row| row.get(0),
    ).unwrap_or(0.0);

    let total_tax: f64 = conn.query_row(
        "SELECT COALESCE(SUM(tax_amount), 0) FROM salary_monthly_results WHERE salary_month = ?1",
        params![month],
        |row| row.get(0),
    ).unwrap_or(0.0);

    let attendance_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM attendance_records WHERE salary_month = ?1",
        params![month],
        |row| row.get(0),
    )?;

    Ok(DashboardSummary {
        employee_count: employee_count as i32,
        active_employee_count: active_employee_count as i32,
        calculated_count: calculated_count as i32,
        locked_count: locked_count as i32,
        total_gross_salary,
        total_net_salary,
        total_social_security,
        total_housing_fund,
        total_tax,
        attendance_count: attendance_count as i32,
    })
}

pub fn get_month_close_workbench(conn: &Connection, month: &str) -> AppResult<MonthCloseWorkbench> {
    let active_employee_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM employees WHERE status = 'active'",
        [],
        |row| row.get(0),
    )?;
    let attendance_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM attendance_records WHERE salary_month = ?1",
        params![month],
        |row| row.get(0),
    )?;
    let missing_attendance_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM employees e
         WHERE e.status = 'active'
           AND NOT EXISTS (
             SELECT 1 FROM attendance_records a
             WHERE a.salary_month = ?1 AND a.employee_no = e.employee_no
           )",
        params![month],
        |row| row.get(0),
    )?;
    let abnormal_attendance_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM attendance_records
         WHERE salary_month = ?1
           AND (actual_days < expected_days OR late_count > 0 OR early_leave_count > 0 OR absent_days > 0)",
        params![month],
        |row| row.get(0),
    )?;
    let salary_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month = ?1",
        params![month],
        |row| row.get(0),
    )?;
    let reviewed_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM salary_monthly_results
         WHERE salary_month = ?1 AND (status IN ('reviewed', 'locked') OR locked = 1)",
        params![month],
        |row| row.get(0),
    )?;
    let locked_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month = ?1 AND locked = 1",
        params![month],
        |row| row.get(0),
    )?;
    let missing_bank_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM employees
         WHERE status = 'active'
           AND (bank_account IS NULL OR TRIM(bank_account) = '' OR bank_name IS NULL OR TRIM(bank_name) = '')",
        [],
        |row| row.get(0),
    )?;
    let invoice_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM invoices WHERE belong_month = ?1 AND status != 'void'",
        params![month],
        |row| row.get(0),
    )?;
    let uncategorized_invoice_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM invoices
         WHERE belong_month = ?1 AND status != 'void'
           AND (employee_id IS NULL OR expense_type_code IS NULL OR TRIM(expense_type_code) = '')",
        params![month],
        |row| row.get(0),
    )?;
    let reimbursement_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM reimbursement_claims WHERE belong_month = ?1 AND status != 'void'",
        params![month],
        |row| row.get(0),
    )?;
    let pending_reimbursement_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM reimbursement_claims
         WHERE belong_month = ?1 AND status NOT IN ('approved', 'rejected', 'void')",
        params![month],
        |row| row.get(0),
    )?;
    let unpaid_reimbursement_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM reimbursement_claims
         WHERE belong_month = ?1 AND status = 'approved' AND payment_status != 'paid'",
        params![month],
        |row| row.get(0),
    )?;
    let pending_payment_batch_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM payment_batches
         WHERE belong_month = ?1 AND status IN ('draft', 'exported')",
        params![month],
        |row| row.get(0),
    )?;
    let unmatched_paid_batch_count: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM payment_batches b
         WHERE b.belong_month = ?1 AND b.status = 'paid'
           AND NOT EXISTS (
             SELECT 1 FROM bank_transaction_matches m
             WHERE m.payment_batch_id = b.id AND m.status = 'active'
           )",
        params![month],
        |row| row.get(0),
    )?;
    let duplicate_amount_count: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM (
           SELECT employee_id, total_amount, COUNT(*) AS c
           FROM reimbursement_claims
           WHERE belong_month = ?1 AND status = 'approved'
           GROUP BY employee_id, total_amount
           HAVING c > 1
         )",
        params![month],
        |row| row.get(0),
    )?;
    let over_budget_count = get_budget_executions(conn, month)?
        .into_iter()
        .filter(|item| item.status == "over")
        .count() as i64;
    let total_salary_cost: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(gross_salary), 0) FROM salary_monthly_results WHERE salary_month = ?1",
            params![month],
            |row| row.get(0),
        )
        .unwrap_or(0.0);
    let total_invoice_amount: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(total_amount), 0) FROM invoices WHERE belong_month = ?1 AND status != 'void'",
            params![month],
            |row| row.get(0),
        )
        .unwrap_or(0.0);
    let approved_reimbursement_amount: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(total_amount), 0) FROM reimbursement_claims
             WHERE belong_month = ?1 AND status = 'approved'",
            params![month],
            |row| row.get(0),
        )
        .unwrap_or(0.0);
    let paid_reimbursement_amount: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(total_amount), 0) FROM reimbursement_claims
             WHERE belong_month = ?1 AND status != 'void' AND payment_status = 'paid'",
            params![month],
            |row| row.get(0),
        )
        .unwrap_or(0.0);
    // 记账凭证平衡：active 凭证总额需大于 0 且与借贷分录合计一致（0.005 容差，与 insert_voucher 阈值一致）
    let unbalanced_voucher_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers v WHERE v.status = 'active' AND (
            v.total_amount <= 0
            OR ABS(v.total_amount - (SELECT COALESCE(SUM(debit_amount),0) FROM voucher_lines WHERE voucher_id = v.id)) > 0.005
            OR ABS(v.total_amount - (SELECT COALESCE(SUM(credit_amount),0) FROM voucher_lines WHERE voucher_id = v.id)) > 0.005
        ) AND v.belong_month = ?1",
        params![month],
        |row| row.get(0),
    )?;
    // 受控解锁未重锁：本月工资计提凭证已作废且同源没有 active 凭证，直接月结会把月份冻结在不一致状态
    let unlocked_accrual_voucher_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers
         WHERE source_type='salary_accrual' AND belong_month = ?1
           AND status='void'
           AND NOT EXISTS (
             SELECT 1 FROM vouchers v2
             WHERE v2.source_type='salary_accrual' AND v2.source_id = vouchers.source_id AND v2.status='active'
           )",
        params![month],
        |row| row.get(0),
    )?;

    let summary = MonthCloseSummary {
        month: month.to_string(),
        active_employee_count: active_employee_count as i32,
        attendance_count: attendance_count as i32,
        missing_attendance_count: missing_attendance_count as i32,
        abnormal_attendance_count: abnormal_attendance_count as i32,
        salary_count: salary_count as i32,
        reviewed_count: reviewed_count as i32,
        locked_count: locked_count as i32,
        missing_bank_count: missing_bank_count as i32,
        invoice_count: invoice_count as i32,
        uncategorized_invoice_count: uncategorized_invoice_count as i32,
        reimbursement_count: reimbursement_count as i32,
        pending_reimbursement_count: pending_reimbursement_count as i32,
        unpaid_reimbursement_count: unpaid_reimbursement_count as i32,
        pending_payment_batch_count: pending_payment_batch_count as i32,
        unmatched_paid_batch_count: unmatched_paid_batch_count as i32,
        duplicate_amount_count: duplicate_amount_count as i32,
        over_budget_count: over_budget_count as i32,
        total_salary_cost,
        total_invoice_amount,
        approved_reimbursement_amount,
        paid_reimbursement_amount,
    };
    let mut checks = build_month_close_checks(&summary);
    checks.push(MonthCloseCheckItem {
        key: "voucher_balance".to_string(),
        title: "记账凭证平衡".to_string(),
        status: if unbalanced_voucher_count == 0 {
            "ok"
        } else {
            "blocking"
        }
        .to_string(),
        count: unbalanced_voucher_count as i32,
        description: if unbalanced_voucher_count == 0 {
            "本月记账凭证借贷平衡".to_string()
        } else {
            format!("存在 {unbalanced_voucher_count} 张借贷不平衡凭证")
        },
        action_route: Some("/bank-transactions".to_string()),
    });
    // 受控解锁未重锁：存在已作废且同源无 active 的工资计提凭证时阻塞月结
    checks.push(MonthCloseCheckItem {
        key: "salary_unlocked_accrual".to_string(),
        title: "工资计提凭证完整".to_string(),
        status: if unlocked_accrual_voucher_count == 0 {
            "ok"
        } else {
            "blocking"
        }
        .to_string(),
        count: unlocked_accrual_voucher_count as i32,
        description: if unlocked_accrual_voucher_count == 0 {
            "本月工资计提凭证与锁定状态一致".to_string()
        } else {
            format!(
                "存在 {unlocked_accrual_voucher_count} 条已作废且未重锁的工资计提凭证，请重新锁定工资后再月结"
            )
        },
        action_route: Some("/salary".to_string()),
    });
    // 12 月月结时自动生成年末损益结转凭证（commands::close_month 挂接）
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
    // 部分核销流水/资金分录（spec 8 warning 项；严格模式可配置阻塞归 7D）。
    // 流水侧已核销含 active allocation 与未迁移旧式批次匹配（与核销引擎 bank_tx_allocated 同口径）；
    // 仅统计"部分核销"（已核销>0 且有剩余），完全未核销不提醒。
    let partial_tx_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM (
           SELECT t.id,
             (CASE WHEN t.income_amount > 0.005 THEN t.income_amount ELSE t.expense_amount END)
               AS side_amount,
             (SELECT COALESCE(SUM(a.allocated_amount),0) FROM bank_reconciliation_allocations a
               WHERE a.transaction_id = t.id AND a.status = 'active')
             + (SELECT COALESCE(SUM(b.total_amount),0) FROM bank_transaction_matches m
                JOIN payment_batches b ON b.id = m.payment_batch_id
                WHERE m.transaction_id = t.id AND m.status = 'active'
                  AND NOT EXISTS (SELECT 1 FROM bank_reconciliation_allocations x
                                  WHERE x.legacy_match_id = m.id)) AS allocated
           FROM bank_transactions t
           WHERE t.belong_month = ?1 AND t.status != 'ignored' AND t.fund_account_id IS NOT NULL
         ) WHERE side_amount - allocated > 0.005 AND allocated > 0.005",
        params![month],
        |row| row.get(0),
    )?;
    let partial_line_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM (
           SELECT vl.id,
             (CASE WHEN vl.debit_amount > 0.005 THEN vl.debit_amount ELSE vl.credit_amount END)
               AS side_amount,
             (SELECT COALESCE(SUM(a.allocated_amount),0) FROM bank_reconciliation_allocations a
               WHERE a.voucher_line_id = vl.id AND a.status = 'active') AS allocated
           FROM voucher_lines vl
           JOIN vouchers v ON v.id = vl.voucher_id
           WHERE v.status = 'active' AND vl.fund_account_id IS NOT NULL AND v.belong_month = ?1
         ) WHERE side_amount - allocated > 0.005 AND allocated > 0.005",
        params![month],
        |row| row.get(0),
    )?;
    let partial_allocation_count = partial_tx_count + partial_line_count;
    checks.push(MonthCloseCheckItem {
        key: "bank_partial_allocation".to_string(),
        title: "银行流水核销完整".to_string(),
        status: if partial_allocation_count == 0 { "ok" } else { "warning" }.to_string(),
        count: partial_allocation_count as i32,
        description: if partial_allocation_count == 0 {
            "本月流水与账面分录不存在部分核销".to_string()
        } else {
            format!(
                "存在 {partial_allocation_count} 条部分核销的流水/账面分录，建议到银行对账完成核销或取消重对"
            )
        },
        action_route: Some("/bank-transactions".to_string()),
    });
    // 待归集银行流水（spec 8 阻塞项 / spec 9.6 迁移完成前降级 warning）。
    // 判别口径：历史归集向导执行过（stage7_fund_assignment_last_applied_at 存在）后，
    // 仍出现待归集流水 → blocking；向导未执行（schema 迁移在启动时即自动完成，
    // 不能作为用户确认信号，spec 13 节「严格月结检查只能在历史归集向导完成后启用」）→ warning。
    let unassigned_tx_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM bank_transactions
         WHERE belong_month = ?1 AND status != 'ignored' AND fund_account_id IS NULL",
        params![month],
        |row| row.get(0),
    )?;
    let fund_assignment_applied =
        get_setting(conn, "stage7_fund_assignment_last_applied_at")?.is_some();
    checks.push(MonthCloseCheckItem {
        key: "fund_unassigned_bank_tx".to_string(),
        title: "待归集银行流水".to_string(),
        status: if unassigned_tx_count == 0 {
            "ok"
        } else if fund_assignment_applied {
            "blocking"
        } else {
            "warning"
        }
        .to_string(),
        count: unassigned_tx_count as i32,
        description: if unassigned_tx_count == 0 {
            "本月银行流水均已归集资金账户".to_string()
        } else if fund_assignment_applied {
            format!(
                "归集向导执行后仍存在 {unassigned_tx_count} 条待归集流水，请先完成归集再月结"
            )
        } else {
            format!(
                "存在 {unassigned_tx_count} 条待归集流水（历史迁移期仅提醒）；执行资金归集向导后此项将阻塞月结"
            )
        },
        action_route: Some("/fund-accounts".to_string()),
    });
    let month_close = get_month_close_record(conn, month)?;
    Ok(MonthCloseWorkbench {
        summary,
        checks,
        month_close,
    })
}

fn build_month_close_checks(summary: &MonthCloseSummary) -> Vec<MonthCloseCheckItem> {
    vec![
        MonthCloseCheckItem {
            key: "attendance_imported".to_string(),
            title: "考勤导入完整".to_string(),
            status: if summary.missing_attendance_count == 0 {
                "ok"
            } else {
                "blocking"
            }
            .to_string(),
            count: summary.missing_attendance_count,
            description: if summary.missing_attendance_count == 0 {
                "所有在职员工已有本月考勤记录".to_string()
            } else {
                format!(
                    "{} 名在职员工缺少本月考勤记录",
                    summary.missing_attendance_count
                )
            },
            action_route: Some("/attendance".to_string()),
        },
        MonthCloseCheckItem {
            key: "attendance_abnormal".to_string(),
            title: "异常考勤复核".to_string(),
            status: if summary.abnormal_attendance_count == 0 {
                "ok"
            } else {
                "warning"
            }
            .to_string(),
            count: summary.abnormal_attendance_count,
            description: if summary.abnormal_attendance_count == 0 {
                "未发现迟到、早退、旷工或缺勤异常".to_string()
            } else {
                format!("{} 条考勤记录需要复核", summary.abnormal_attendance_count)
            },
            action_route: Some("/attendance".to_string()),
        },
        MonthCloseCheckItem {
            key: "salary_calculated".to_string(),
            title: "工资已计算".to_string(),
            status: if summary.salary_count >= summary.active_employee_count
                && summary.active_employee_count > 0
            {
                "ok"
            } else {
                "blocking"
            }
            .to_string(),
            count: summary
                .active_employee_count
                .saturating_sub(summary.salary_count),
            description: if summary.salary_count >= summary.active_employee_count
                && summary.active_employee_count > 0
            {
                "本月在职员工工资已生成".to_string()
            } else {
                "工资结果未覆盖全部在职员工".to_string()
            },
            action_route: Some("/salary".to_string()),
        },
        MonthCloseCheckItem {
            key: "salary_reviewed_locked".to_string(),
            title: "工资复核与锁定".to_string(),
            status: if summary.locked_count >= summary.salary_count && summary.salary_count > 0 {
                "ok"
            } else if summary.reviewed_count >= summary.salary_count && summary.salary_count > 0 {
                "warning"
            } else {
                "blocking"
            }
            .to_string(),
            count: summary.salary_count.saturating_sub(summary.locked_count),
            description: if summary.locked_count >= summary.salary_count && summary.salary_count > 0
            {
                "本月工资已锁定".to_string()
            } else if summary.reviewed_count >= summary.salary_count && summary.salary_count > 0 {
                "工资已复核，尚未全部锁定".to_string()
            } else {
                "工资尚未完成复核".to_string()
            },
            action_route: Some("/salary".to_string()),
        },
        MonthCloseCheckItem {
            key: "bank_accounts".to_string(),
            title: "银行信息完整".to_string(),
            status: if summary.missing_bank_count == 0 {
                "ok"
            } else {
                "warning"
            }
            .to_string(),
            count: summary.missing_bank_count,
            description: if summary.missing_bank_count == 0 {
                "在职员工银行账号和开户行完整".to_string()
            } else {
                format!(
                    "{} 名在职员工缺少银行账号或开户行",
                    summary.missing_bank_count
                )
            },
            action_route: Some("/employees".to_string()),
        },
        MonthCloseCheckItem {
            key: "invoices_categorized".to_string(),
            title: "发票归类完整".to_string(),
            status: if summary.uncategorized_invoice_count == 0 {
                "ok"
            } else {
                "warning"
            }
            .to_string(),
            count: summary.uncategorized_invoice_count,
            description: if summary.uncategorized_invoice_count == 0 {
                "本月发票均已关联报销人和费用类型".to_string()
            } else {
                format!(
                    "{} 张发票缺少报销人或费用类型",
                    summary.uncategorized_invoice_count
                )
            },
            action_route: Some("/invoices".to_string()),
        },
        MonthCloseCheckItem {
            key: "reimbursements_paid".to_string(),
            title: "报销审批与付款".to_string(),
            status: if summary.pending_reimbursement_count == 0
                && summary.unpaid_reimbursement_count == 0
            {
                "ok"
            } else {
                "blocking"
            }
            .to_string(),
            count: summary.pending_reimbursement_count + summary.unpaid_reimbursement_count,
            description: if summary.pending_reimbursement_count == 0
                && summary.unpaid_reimbursement_count == 0
            {
                "本月报销单已完成审批付款".to_string()
            } else {
                format!(
                    "{} 张待审批，{} 张已审批未付款",
                    summary.pending_reimbursement_count, summary.unpaid_reimbursement_count
                )
            },
            action_route: Some("/reimbursements".to_string()),
        },
        MonthCloseCheckItem {
            key: "payment_batches_paid".to_string(),
            title: "付款批次完成".to_string(),
            status: if summary.pending_payment_batch_count == 0 {
                "ok"
            } else {
                "blocking"
            }
            .to_string(),
            count: summary.pending_payment_batch_count,
            description: if summary.pending_payment_batch_count == 0 {
                "本月付款批次均已完成付款或作废".to_string()
            } else {
                format!(
                    "{} 个付款批次待导出或待付款",
                    summary.pending_payment_batch_count
                )
            },
            action_route: Some("/payments".to_string()),
        },
        MonthCloseCheckItem {
            key: "bank_transactions_matched".to_string(),
            title: "银行流水匹配".to_string(),
            status: if summary.unmatched_paid_batch_count == 0 {
                "ok"
            } else {
                "blocking"
            }
            .to_string(),
            count: summary.unmatched_paid_batch_count,
            description: if summary.unmatched_paid_batch_count == 0 {
                "已付款批次均已匹配银行流水".to_string()
            } else {
                format!(
                    "{} 个已付款批次尚未匹配银行流水",
                    summary.unmatched_paid_batch_count
                )
            },
            action_route: Some("/bank-transactions".to_string()),
        },
        MonthCloseCheckItem {
            key: "budget_overrun".to_string(),
            title: "预算异常".to_string(),
            status: if summary.over_budget_count == 0 {
                "ok"
            } else {
                "warning"
            }
            .to_string(),
            count: summary.over_budget_count,
            description: if summary.over_budget_count == 0 {
                "预算执行未超出已配置额度".to_string()
            } else {
                format!("{} 项预算已超支", summary.over_budget_count)
            },
            action_route: Some("/financial-analysis".to_string()),
        },
        MonthCloseCheckItem {
            key: "duplicate_amounts".to_string(),
            title: "重复金额检查".to_string(),
            status: if summary.duplicate_amount_count == 0 {
                "ok"
            } else {
                "warning"
            }
            .to_string(),
            count: summary.duplicate_amount_count,
            description: if summary.duplicate_amount_count == 0 {
                "未发现同报销人同金额的重复报销组合".to_string()
            } else {
                format!(
                    "{} 组同报销人同金额报销需要复核",
                    summary.duplicate_amount_count
                )
            },
            action_route: Some("/reimbursements".to_string()),
        },
    ]
}

pub fn get_month_close_record(
    conn: &Connection,
    month: &str,
) -> AppResult<Option<MonthCloseRecord>> {
    let result = conn.query_row(
        "SELECT id, month, status, summary_json, checks_json, closed_at, closed_by,
                reopened_at, reopen_reason, remark, created_at, updated_at
         FROM month_closes WHERE month = ?1",
        params![month],
        |row| {
            Ok(MonthCloseRecord {
                id: row.get(0)?,
                month: row.get(1)?,
                status: row.get(2)?,
                summary_json: row.get(3)?,
                checks_json: row.get(4)?,
                closed_at: row.get(5)?,
                closed_by: row.get(6)?,
                reopened_at: row.get(7)?,
                reopen_reason: row.get(8)?,
                remark: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        },
    );
    match result {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table: month_closes") =>
        {
            Ok(None)
        }
        Err(e) => Err(AppError::from(e)),
    }
}

pub fn is_month_closed(conn: &Connection, month: &str) -> AppResult<bool> {
    Ok(get_month_close_record(conn, month)?
        .map(|record| record.status == "closed")
        .unwrap_or(false))
}

pub fn ensure_month_open(conn: &Connection, month: &str) -> AppResult<()> {
    if month.trim().is_empty() {
        return Ok(());
    }
    if is_month_closed(conn, month)? {
        Err(AppError::InvalidParam(format!(
            "{month} 已正式月结，请先反月结后再修改"
        )))
    } else {
        Ok(())
    }
}

pub fn close_month(
    conn: &Connection,
    month: &str,
    operator: &str,
    remark: Option<&str>,
) -> AppResult<MonthCloseRecord> {
    if month.trim().is_empty() {
        return Err(AppError::InvalidParam("月结月份必填".into()));
    }
    if is_month_closed(conn, month)? {
        return Err(AppError::InvalidParam(format!("{month} 已正式月结")));
    }

    let workbench = get_month_close_workbench(conn, month)?;
    let blockers: Vec<String> = workbench
        .checks
        .iter()
        .filter(|item| item.status == "blocking")
        .map(|item| item.title.clone())
        .collect();
    if !blockers.is_empty() {
        return Err(AppError::InvalidParam(format!(
            "存在阻塞检查项，不能正式月结: {}",
            blockers.join("、")
        )));
    }

    let now = Utc::now().to_rfc3339();
    let summary_json = serde_json::to_string(&workbench.summary)?;
    let checks_json = serde_json::to_string(&workbench.checks)?;
    conn.execute(
        "INSERT INTO month_closes
            (month, status, summary_json, checks_json, closed_at, closed_by, reopened_at,
             reopen_reason, remark, created_at, updated_at)
         VALUES (?1, 'closed', ?2, ?3, ?4, ?5, NULL, NULL, ?6, ?7, ?8)
         ON CONFLICT(month) DO UPDATE SET
            status='closed',
            summary_json=excluded.summary_json,
            checks_json=excluded.checks_json,
            closed_at=excluded.closed_at,
            closed_by=excluded.closed_by,
            remark=excluded.remark,
            updated_at=excluded.updated_at",
        params![
            month.trim(),
            summary_json,
            checks_json,
            now,
            operator,
            remark,
            now,
            now
        ],
    )?;
    get_month_close_record(conn, month)?.ok_or_else(|| AppError::NotFound("月结记录未找到".into()))
}

pub fn reopen_month(conn: &Connection, month: &str, reason: &str) -> AppResult<MonthCloseRecord> {
    if reason.trim().is_empty() {
        return Err(AppError::InvalidParam("反月结原因必填".into()));
    }
    let existing = get_month_close_record(conn, month)?
        .ok_or_else(|| AppError::NotFound(format!("{month} 尚未正式月结")))?;
    if existing.status != "closed" {
        return Err(AppError::InvalidParam(format!(
            "{month} 当前不是已月结状态"
        )));
    }

    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE month_closes
         SET status='reopened', reopened_at=?1, reopen_reason=?2, updated_at=?3
         WHERE month=?4 AND status='closed'",
        params![now, reason.trim(), now, month],
    )?;
    get_month_close_record(conn, month)?.ok_or_else(|| AppError::NotFound("月结记录未找到".into()))
}

// ==================== Payment Batches ====================

/// 批次类型：salary 工资代发 / reimbursement 报销付款 / general 通用付款（第七阶段，spec 5.3）
const PAYMENT_BATCH_TYPES: [&str; 3] = ["salary", "reimbursement", "general"];

/// general 批次可纳入的资金单类型（付款单/员工借款单，与 cashier::BATCHABLE_TYPES 对齐）
const GENERAL_BATCH_SOURCE_TYPES: [&str; 2] = ["payment", "advance"];

/// payment_items 来源类型：fund_document 对应通用资金单（spec 5.3）
pub(crate) const PAYMENT_SOURCE_FUND_DOCUMENT: &str = "fund_document";

fn validate_payment_batch_type(batch_type: &str) -> AppResult<()> {
    if PAYMENT_BATCH_TYPES.contains(&batch_type) {
        Ok(())
    } else {
        Err(AppError::InvalidParam("付款批次类型无效".into()))
    }
}

/// 生成批次号：前缀 + YYYYMM + 纳秒时间戳 + 随机后缀，避免同毫秒并发撞 UNIQUE。
fn payment_batch_no(month: &str, batch_type: &str) -> String {
    let prefix = match batch_type {
        "salary" => "GZ",
        "reimbursement" => "BX",
        _ => "TY",
    };
    let nanos = Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or_else(|| Utc::now().timestamp_millis() as i64);
    let suffix = std::process::id() as i64 ^ nanos;
    format!(
        "{}{}{}{:04X}",
        prefix,
        month.replace('-', ""),
        nanos,
        suffix & 0xFFFF
    )
}

pub(crate) fn row_to_payment_batch(row: &rusqlite::Row<'_>) -> rusqlite::Result<PaymentBatch> {
    Ok(PaymentBatch {
        id: row.get(0)?,
        batch_no: row.get(1)?,
        belong_month: row.get(2)?,
        batch_type: row.get(3)?,
        status: row.get(4)?,
        total_amount: row.get(5)?,
        item_count: row.get(6)?,
        payment_date: row.get(7)?,
        remark: row.get(8)?,
        fund_account_id: row.get(9)?,
        fund_account_name: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn row_to_payment_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<PaymentItem> {
    Ok(PaymentItem {
        id: row.get(0)?,
        batch_id: row.get(1)?,
        source_type: row.get(2)?,
        source_id: row.get(3)?,
        employee_id: row.get(4)?,
        employee_no: row.get(5)?,
        employee_name: row.get(6)?,
        bank_name: row.get(7)?,
        bank_account: row.get(8)?,
        amount: row.get(9)?,
        status: row.get(10)?,
        remark: row.get(11)?,
        created_at: row.get(12)?,
    })
}

pub fn query_payment_batches(
    conn: &Connection,
    query: &PaymentBatchQuery,
) -> AppResult<Vec<PaymentBatch>> {
    let mut where_clauses = vec!["1=1".to_string()];
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(month) = query.belong_month.as_ref().filter(|v| !v.trim().is_empty()) {
        where_clauses.push(format!("belong_month = ?{idx}"));
        params_vec.push(Box::new(month.clone()));
        idx += 1;
    }
    if let Some(batch_type) = query.batch_type.as_ref().filter(|v| !v.trim().is_empty()) {
        where_clauses.push(format!("batch_type = ?{idx}"));
        params_vec.push(Box::new(batch_type.clone()));
        idx += 1;
    }
    if let Some(status) = query.status.as_ref().filter(|v| !v.trim().is_empty()) {
        where_clauses.push(format!("status = ?{idx}"));
        params_vec.push(Box::new(status.clone()));
    }

    let sql = format!(
        "SELECT b.id, b.batch_no, b.belong_month, b.batch_type, b.status, b.total_amount,
                b.item_count, b.payment_date, b.remark, b.fund_account_id, fa.name,
                b.created_at, b.updated_at
         FROM payment_batches b
         LEFT JOIN fund_accounts fa ON fa.id = b.fund_account_id
         WHERE {}
         ORDER BY b.created_at DESC, b.id DESC",
        where_clauses.join(" AND ")
    );
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), row_to_payment_batch)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn get_payment_batch(conn: &Connection, id: i64) -> AppResult<PaymentBatch> {
    conn.query_row(
        "SELECT b.id, b.batch_no, b.belong_month, b.batch_type, b.status, b.total_amount,
                b.item_count, b.payment_date, b.remark, b.fund_account_id, fa.name,
                b.created_at, b.updated_at
         FROM payment_batches b
         LEFT JOIN fund_accounts fa ON fa.id = b.fund_account_id
         WHERE b.id = ?1",
        params![id],
        row_to_payment_batch,
    )
    .map_err(|e| AppError::NotFound(format!("付款批次ID={id}未找到: {e}")))
}

pub fn get_payment_batch_detail(conn: &Connection, id: i64) -> AppResult<PaymentBatchDetail> {
    let batch = get_payment_batch(conn, id)?;
    let mut stmt = conn.prepare(
        "SELECT id, batch_id, source_type, source_id, employee_id, employee_no, employee_name,
                bank_name, bank_account, amount, status, remark, created_at
         FROM payment_items
         WHERE batch_id = ?1
         ORDER BY id",
    )?;
    let rows = stmt.query_map(params![id], row_to_payment_item)?;
    let items = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(PaymentBatchDetail { batch, items })
}

fn active_payment_item_exists(
    conn: &Connection,
    source_type: &str,
    source_id: i64,
) -> AppResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM payment_items i
         JOIN payment_batches b ON b.id = i.batch_id
         WHERE i.source_type = ?1 AND i.source_id = ?2
           AND i.status != 'void' AND b.status != 'void'",
        params![source_type, source_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn selected_source_filter(source_ids: &Option<Vec<i64>>, id: i64) -> bool {
    source_ids
        .as_ref()
        .map(|ids| ids.contains(&id))
        .unwrap_or(true)
}

/// 校验并返回批次付款资金账户（spec 5.3：创建批次必须选择付款资金账户，须存在且启用）。
fn ensure_batch_fund_account(conn: &Connection, account_id: Option<i64>) -> AppResult<i64> {
    let account_id =
        account_id.ok_or_else(|| AppError::InvalidParam("请选择付款资金账户".into()))?;
    let (name, is_active): (String, i64) = conn
        .query_row(
            "SELECT name, is_active FROM fund_accounts WHERE id = ?1",
            params![account_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("资金账户不存在：id={account_id}")))?;
    if is_active == 0 {
        return Err(AppError::InvalidParam(format!(
            "资金账户 {name} 已停用，不能用于付款批次"
        )));
    }
    Ok(account_id)
}

/// 历史批次只读保护（brief：旧批次允许 NULL 只读）：
/// 缺少资金账户的存量批次不允许推进付款流程（作废重建是唯一出路）。
fn ensure_batch_has_fund_account(batch: &PaymentBatch) -> AppResult<()> {
    if batch.fund_account_id.is_none() {
        return Err(AppError::InvalidParam(
            "历史付款批次未指定资金账户（只读），请作废后重新生成批次".into(),
        ));
    }
    Ok(())
}

pub fn create_payment_batch(
    conn: &mut Connection,
    input: &PaymentBatchInput,
    current: &crate::cashier::CurrentOperatorState,
) -> AppResult<PaymentBatchDetail> {
    let month = input.belong_month.trim();
    if month.is_empty() {
        return Err(AppError::InvalidParam("付款月份必填".into()));
    }
    validate_payment_batch_type(&input.batch_type)?;
    ensure_month_open(conn, month)?;

    let tx = conn.unchecked_transaction()?;
    // 资金账户校验在事务内执行（spec 5.3：批次内所有项目使用同一账户）
    let fund_account_id = ensure_batch_fund_account(&tx, input.fund_account_id)?;
    let now = Utc::now().to_rfc3339();
    let batch_no = payment_batch_no(month, &input.batch_type);

    let candidates = match input.batch_type.as_str() {
        "salary" => collect_salary_payment_candidates(&tx, month, &input.source_ids)?,
        "reimbursement" => collect_reimbursement_payment_candidates(&tx, month, &input.source_ids)?,
        // general 批次：从已审批付款/借款单勾选（同账户同月，spec 5.3）
        _ => collect_fund_document_payment_candidates(
            &tx,
            month,
            fund_account_id,
            &input.source_ids,
        )?,
    };
    if candidates.is_empty() {
        return Err(AppError::InvalidParam("没有可生成付款批次的明细".into()));
    }

    let total_amount: f64 = candidates.iter().map(|item| item.amount).sum();
    conn_insert_payment_batch(
        &tx,
        &batch_no,
        month,
        &input.batch_type,
        fund_account_id,
        total_amount,
        candidates.len() as i32,
        input.remark.as_deref(),
        &now,
    )?;
    let batch_id = tx.last_insert_rowid();

    for item in &candidates {
        tx.execute(
            "INSERT INTO payment_items
                (batch_id, source_type, source_id, employee_id, employee_no, employee_name,
                 bank_name, bank_account, amount, status, remark, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', ?10, ?11)",
            params![
                batch_id,
                item.source_type,
                item.source_id,
                item.employee_id,
                item.employee_no,
                item.employee_name,
                item.bank_name,
                item.bank_account,
                item.amount,
                item.remark,
                now
            ],
        )?;
    }

    // general 批次：资金单 approved → batched 走状态机（同事务追加审批事件，spec 5.1/5.3）
    if input.batch_type == "general" {
        for item in &candidates {
            crate::cashier::mark_document_batched_in_tx(&tx, current, item.source_id, batch_id)?;
        }
    }

    tx.commit()?;
    get_payment_batch_detail(conn, batch_id)
}

fn conn_insert_payment_batch(
    conn: &Connection,
    batch_no: &str,
    month: &str,
    batch_type: &str,
    fund_account_id: i64,
    total_amount: f64,
    item_count: i32,
    remark: Option<&str>,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO payment_batches
            (batch_no, belong_month, batch_type, fund_account_id, status, total_amount,
             item_count, payment_date, remark, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6, NULL, ?7, ?8, ?9)",
        params![
            batch_no,
            month,
            batch_type,
            fund_account_id,
            total_amount,
            item_count,
            remark,
            now,
            now
        ],
    )?;
    Ok(())
}

/// general 批次候选：同月已审批的付款单/借款单，且来源账户等于批次账户（spec 5.3）。
/// item 快照往来方（员工或往来单位）编码/名称/开户行/银行账号，备注存单据编号。
fn collect_fund_document_payment_candidates(
    conn: &Connection,
    month: &str,
    fund_account_id: i64,
    source_ids: &Option<Vec<i64>>,
) -> AppResult<Vec<PaymentItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, document_type, document_no, amount, partner_id, employee_id
         FROM fund_documents
         WHERE belong_month = ?1 AND status = 'approved'
           AND document_type IN (?2, ?3)
           AND source_account_id = ?4
         ORDER BY id",
    )?;
    let rows = stmt.query_map(
        params![
            month,
            GENERAL_BATCH_SOURCE_TYPES[0],
            GENERAL_BATCH_SOURCE_TYPES[1],
            fund_account_id
        ],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f64>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        },
    )?;

    let mut candidates = Vec::new();
    for row in rows {
        let (doc_id, doc_type, doc_no, amount, partner_id, employee_id) = row?;
        if !selected_source_filter(source_ids, doc_id) {
            continue;
        }
        if active_payment_item_exists(conn, PAYMENT_SOURCE_FUND_DOCUMENT, doc_id)? {
            continue;
        }
        if amount <= 0.0 {
            return Err(AppError::InvalidParam(format!(
                "单据 {doc_no} 金额必须大于0"
            )));
        }
        // 往来方银行信息快照：员工取员工档案，往来单位取单位档案（spec 5.3）
        let (code, name, bank_name, bank_account) = if let Some(emp_id) = employee_id {
            conn.query_row(
                "SELECT employee_no, name, bank_name, bank_account FROM employees WHERE id = ?1",
                params![emp_id],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("单据 {doc_no} 的员工不存在：id={emp_id}")))?
        } else if let Some(pt_id) = partner_id {
            conn.query_row(
                "SELECT partner_code, name, bank_name, bank_account FROM business_partners WHERE id = ?1",
                params![pt_id],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| {
                AppError::NotFound(format!("单据 {doc_no} 的往来单位不存在：id={pt_id}"))
            })?
        } else {
            return Err(AppError::InvalidParam(format!(
                "单据 {doc_no} 缺少往来对象，不能进入付款批次"
            )));
        };
        let counterparty = name.clone().unwrap_or_else(|| "未命名".into());
        if bank_account
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
            || bank_name.as_deref().map(str::trim).unwrap_or("").is_empty()
        {
            return Err(AppError::InvalidParam(format!(
                "{counterparty} 缺少银行账号或开户行，不能生成通用付款批次（单据 {doc_no}）"
            )));
        }
        candidates.push(PaymentItem {
            id: 0,
            batch_id: 0,
            source_type: PAYMENT_SOURCE_FUND_DOCUMENT.into(),
            source_id: doc_id,
            employee_id,
            employee_no: code,
            employee_name: name,
            bank_name,
            bank_account,
            amount,
            status: "pending".into(),
            remark: Some(format!(
                "{}{}",
                doc_no,
                if doc_type == "advance" {
                    "（借款）"
                } else {
                    ""
                }
            )),
            created_at: None,
        });
    }
    Ok(candidates)
}

fn collect_salary_payment_candidates(
    conn: &Connection,
    month: &str,
    source_ids: &Option<Vec<i64>>,
) -> AppResult<Vec<PaymentItem>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, e.id, s.employee_no, COALESCE(s.name, e.name), e.bank_name, e.bank_account,
                s.net_salary, s.remark
         FROM salary_monthly_results s
         LEFT JOIN employees e ON e.employee_no = s.employee_no
         WHERE s.salary_month = ?1 AND s.locked = 1
           AND COALESCE(s.payment_status, 'unpaid') != 'paid'
         ORDER BY s.id",
    )?;
    let rows = stmt.query_map(params![month], |row| {
        Ok(PaymentItem {
            id: 0,
            batch_id: 0,
            source_type: "salary_result".into(),
            source_id: row.get(0)?,
            employee_id: row.get(1)?,
            employee_no: row.get(2)?,
            employee_name: row.get(3)?,
            bank_name: row.get(4)?,
            bank_account: row.get(5)?,
            amount: row.get(6)?,
            status: "pending".into(),
            remark: row.get(7)?,
            created_at: None,
        })
    })?;
    let mut candidates = Vec::new();
    for item in rows {
        let item = item?;
        if !selected_source_filter(source_ids, item.source_id) {
            continue;
        }
        if active_payment_item_exists(conn, &item.source_type, item.source_id)? {
            continue;
        }
        if item.amount <= 0.0 {
            return Err(AppError::InvalidParam("工资付款金额必须大于0".into()));
        }
        if item
            .bank_account
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
            || item
                .bank_name
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        {
            return Err(AppError::InvalidParam(format!(
                "{} 缺少银行账号或开户行，不能生成工资付款批次",
                item.employee_name.as_deref().unwrap_or("员工")
            )));
        }
        candidates.push(item);
    }
    Ok(candidates)
}

fn collect_reimbursement_payment_candidates(
    conn: &Connection,
    month: &str,
    source_ids: &Option<Vec<i64>>,
) -> AppResult<Vec<PaymentItem>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, e.id, e.employee_no, e.name, e.bank_name, e.bank_account,
                c.total_amount, c.claim_no
         FROM reimbursement_claims c
         LEFT JOIN employees e ON e.id = c.employee_id
         WHERE c.belong_month = ?1 AND c.status = 'approved'
           AND c.payment_status != 'paid'
         ORDER BY c.id",
    )?;
    let rows = stmt.query_map(params![month], |row| {
        let claim_no: String = row.get(7)?;
        Ok(PaymentItem {
            id: 0,
            batch_id: 0,
            source_type: "reimbursement_claim".into(),
            source_id: row.get(0)?,
            employee_id: row.get(1)?,
            employee_no: row.get(2)?,
            employee_name: row.get(3)?,
            bank_name: row.get(4)?,
            bank_account: row.get(5)?,
            amount: row.get(6)?,
            status: "pending".into(),
            remark: Some(claim_no),
            created_at: None,
        })
    })?;
    let mut candidates = Vec::new();
    for item in rows {
        let item = item?;
        if !selected_source_filter(source_ids, item.source_id) {
            continue;
        }
        if active_payment_item_exists(conn, &item.source_type, item.source_id)? {
            continue;
        }
        if item.amount <= 0.0 {
            return Err(AppError::InvalidParam("报销付款金额必须大于0".into()));
        }
        if item
            .bank_account
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
            || item
                .bank_name
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        {
            return Err(AppError::InvalidParam(format!(
                "{} 缺少银行账号或开户行，不能生成报销付款批次",
                item.employee_name.as_deref().unwrap_or("报销人")
            )));
        }
        candidates.push(item);
    }
    Ok(candidates)
}

pub fn update_payment_batch_remark(
    conn: &Connection,
    input: &PaymentBatchRemarkInput,
) -> AppResult<PaymentBatch> {
    let batch = get_payment_batch(conn, input.id)?;
    if batch.status == "void" {
        return Err(AppError::InvalidParam("已作废付款批次不能修改备注".into()));
    }
    ensure_batch_has_fund_account(&batch)?;
    ensure_month_open(conn, &batch.belong_month)?;
    conn.execute(
        "UPDATE payment_batches SET remark=?1, updated_at=?2 WHERE id=?3",
        params![input.remark.as_ref(), Utc::now().to_rfc3339(), input.id],
    )?;
    get_payment_batch(conn, input.id)
}

/// 导出门禁只读校验（供命令层在写导出文件前调用，避免门禁拒绝时留下孤儿文件）。
pub fn ensure_payment_batch_exportable(conn: &Connection, id: i64) -> AppResult<PaymentBatch> {
    let batch = get_payment_batch(conn, id)?;
    ensure_month_open(conn, &batch.belong_month)?;
    if batch.status == "void" {
        return Err(AppError::InvalidParam("已作废付款批次不能导出".into()));
    }
    ensure_batch_has_fund_account(&batch)?;
    Ok(batch)
}

pub fn mark_payment_batch_exported(conn: &Connection, id: i64) -> AppResult<PaymentBatch> {
    let batch = ensure_payment_batch_exportable(conn, id)?;
    if batch.status == "paid" {
        return Ok(batch);
    }
    conn.execute(
        "UPDATE payment_batches SET status='exported', updated_at=?1 WHERE id=?2",
        params![Utc::now().to_rfc3339(), id],
    )?;
    get_payment_batch(conn, id)
}

pub fn mark_payment_batch_paid(
    conn: &mut Connection,
    input: &PaymentBatchPaidInput,
    current: &crate::cashier::CurrentOperatorState,
) -> AppResult<PaymentBatch> {
    if input.payment_date.trim().is_empty() {
        return Err(AppError::InvalidParam("付款日期必填".into()));
    }
    let batch = get_payment_batch(conn, input.id)?;
    ensure_month_open(conn, &batch.belong_month)?;
    if batch.status == "void" {
        return Err(AppError::InvalidParam("已作废付款批次不能标记付款".into()));
    }
    ensure_batch_has_fund_account(&batch)?;
    if batch.status == "paid" {
        return Ok(batch);
    }
    if batch.status != "exported" {
        return Err(AppError::InvalidParam(
            "付款批次必须先导出后才能标记已付款".into(),
        ));
    }

    let tx = conn.unchecked_transaction()?;
    let detail = get_payment_batch_detail(&tx, input.id)?;
    let now = Utc::now().to_rfc3339();
    if batch.batch_type == "general" {
        // general 批次（spec 5.3）：付款后逐张资金单 batched → settled 走状态机，
        // 结算署名、资金辅助凭证与审批事件同事务生成；无批次级凭证（避免与
        // 单据结算凭证重复记账）。凭证失败时整体回滚，付款标记不落库。
        for item in &detail.items {
            if item.source_type == PAYMENT_SOURCE_FUND_DOCUMENT {
                crate::cashier::settle_batched_document_in_tx(&tx, current, item.source_id)?;
            }
        }
    } else {
        for item in &detail.items {
            if item.source_type == "salary_result" {
                tx.execute(
                    "UPDATE salary_monthly_results
                     SET payment_status='paid', payment_date=?1, payment_batch_id=?2, updated_at=?3
                     WHERE id=?4",
                    params![input.payment_date.trim(), input.id, now, item.source_id],
                )?;
            } else if item.source_type == "reimbursement_claim" {
                tx.execute(
                    "UPDATE reimbursement_claims
                     SET payment_status='paid', payment_date=?1, payment_batch_id=?2, updated_at=?3
                     WHERE id=?4",
                    params![input.payment_date.trim(), input.id, now, item.source_id],
                )?;
            }
        }
    }
    tx.execute(
        "UPDATE payment_items SET status='paid' WHERE batch_id=?1 AND status != 'void'",
        params![input.id],
    )?;
    tx.execute(
        "UPDATE payment_batches SET status='paid', payment_date=?1, updated_at=?2 WHERE id=?3",
        params![input.payment_date.trim(), now, input.id],
    )?;
    // salary/reimbursement：状态置 paid 与付款凭证生成同事务（凭证校验 status='paid'，
    // 必须在批次状态更新之后）；general 无批次级凭证（单据结算凭证已生成）
    if batch.batch_type != "general" {
        crate::accounting::generate_payment_voucher(&tx, input.id)?;
    }
    tx.commit()?;
    get_payment_batch(conn, input.id)
}

pub fn void_payment_batch(
    conn: &mut Connection,
    input: &PaymentBatchVoidInput,
    current: &crate::cashier::CurrentOperatorState,
) -> AppResult<PaymentBatch> {
    if input.reason.trim().is_empty() {
        return Err(AppError::InvalidParam("作废原因必填".into()));
    }
    let batch = get_payment_batch(conn, input.id)?;
    ensure_month_open(conn, &batch.belong_month)?;
    if batch.status == "void" {
        return Ok(batch);
    }
    // spec 5.3：已付款批次不可作废，错误通过冲正单处理（general 批次来源单已 settled，
    // 有冲正状态机兜底；salary/reimbursement 保留原纠错路径不变）
    if batch.batch_type == "general" && batch.status == "paid" {
        return Err(AppError::InvalidParam(
            "已付款的通用付款批次不可作废，付款错误请通过资金单冲正处理".into(),
        ));
    }

    let tx = conn.unchecked_transaction()?;
    let now = Utc::now().to_rfc3339();
    if batch.status != "paid" {
        let detail = get_payment_batch_detail(&tx, input.id)?;
        for item in &detail.items {
            if item.source_type == "salary_result" {
                tx.execute(
                    "UPDATE salary_monthly_results
                     SET payment_batch_id=NULL
                     WHERE id=?1 AND COALESCE(payment_status, 'unpaid') != 'paid'",
                    params![item.source_id],
                )?;
            } else if item.source_type == "reimbursement_claim" {
                tx.execute(
                    "UPDATE reimbursement_claims
                     SET payment_batch_id=NULL
                     WHERE id=?1 AND payment_status != 'paid'",
                    params![item.source_id],
                )?;
            } else if item.source_type == PAYMENT_SOURCE_FUND_DOCUMENT {
                // 释放资金单 batched → approved 走状态机（清 payment_batch_id + 审批事件），
                // 释放后可重新进入其他批次（spec 5.3）
                crate::cashier::release_document_batched_in_tx(&tx, current, item.source_id)?;
            }
        }
    }
    tx.execute(
        "UPDATE payment_items SET status='void' WHERE batch_id=?1",
        params![input.id],
    )?;
    tx.execute(
        "UPDATE payment_batches SET status='void', remark=?1, updated_at=?2 WHERE id=?3",
        params![input.reason.trim(), now, input.id],
    )?;
    // 批次作废与付款凭证作废同事务：任一失败整体回滚
    crate::accounting::void_payment_voucher(&tx, input.id)?;
    tx.commit()?;
    get_payment_batch(conn, input.id)
}

// ==================== Social Insurance Profiles ====================

/// 基数按上下限 clamp（min/max <= 0 视为不限制）
pub fn clamp_base(value: f64, min: f64, max: f64) -> f64 {
    let mut v = value;
    if min > 0.0 && v < min {
        v = min;
    }
    if max > 0.0 && v > max {
        v = max;
    }
    v
}

/// 读取社保/公积金缴费基数上下限（ss_min, ss_max, hf_min, hf_max，0 = 不限制）
pub fn get_social_base_limits(conn: &Connection) -> AppResult<(f64, f64, f64, f64)> {
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

/// 保存社保/公积金缴费基数上下限设置
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

/// 查询某年度全部社保公积金台账（按工号排序）
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

/// 新增或更新社保公积金台账（id 命中且员工/年度一致时更新，否则新增；同员工同年度重复新增报错）
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
                params![
                    input.employee_no,
                    input.profile_year,
                    ss_base,
                    hf_base,
                    ss_e,
                    ss_p,
                    hf_e,
                    hf_p,
                    input.remark,
                    now
                ],
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

/// 删除台账记录，返回是否存在
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
        return Err(AppError::InvalidParam(format!(
            "{from_year} 年度无台账可复制"
        )));
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
            params![
                p.employee_no,
                to_year,
                ss,
                hf,
                p.ss_employer_rate,
                p.ss_personal_rate,
                p.hf_employer_rate,
                p.hf_personal_rate,
                p.remark,
                now
            ],
        )?;
        n += 1;
    }
    Ok(n)
}

// ==================== Bank Transactions ====================

fn row_to_bank_transaction(row: &rusqlite::Row<'_>) -> rusqlite::Result<BankTransaction> {
    Ok(BankTransaction {
        id: row.get(0)?,
        transaction_date: row.get(1)?,
        belong_month: row.get(2)?,
        summary: row.get(3)?,
        counterparty_name: row.get(4)?,
        counterparty_account: row.get(5)?,
        income_amount: row.get(6)?,
        expense_amount: row.get(7)?,
        balance: row.get(8)?,
        status: row.get(9)?,
        ignore_reason: row.get(10)?,
        imported_file: row.get(11)?,
        raw_json: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
        matched_batch_id: row.get(15)?,
        matched_batch_no: row.get(16)?,
        matched_batch_type: row.get(17)?,
        matched_amount: row.get(18)?,
        match_score: row.get(19)?,
        match_remark: row.get(20)?,
        fund_account_id: row.get(21)?,
        fund_account_name: row.get(22)?,
        allocated_amount: row.get(23)?,
        remaining_amount: row.get(24)?,
    })
}

fn row_to_bank_match(row: &rusqlite::Row<'_>) -> rusqlite::Result<BankTransactionMatch> {
    Ok(BankTransactionMatch {
        id: row.get(0)?,
        transaction_id: row.get(1)?,
        payment_batch_id: row.get(2)?,
        match_score: row.get(3)?,
        remark: row.get(4)?,
        created_at: row.get(5)?,
    })
}

pub fn query_bank_transactions(
    conn: &Connection,
    query: &BankTransactionQuery,
) -> AppResult<Vec<BankTransaction>> {
    let mut where_clauses = vec!["1=1".to_string()];
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(month) = query
        .belong_month
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        where_clauses.push(format!("t.belong_month = ?{idx}"));
        params_vec.push(Box::new(month.clone()));
        idx += 1;
    }
    if let Some(status) = query
        .status
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        where_clauses.push(format!("t.status = ?{idx}"));
        params_vec.push(Box::new(status.clone()));
        idx += 1;
    }
    if let Some(keyword) = query
        .keyword
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        where_clauses.push(format!(
            "(t.summary LIKE ?{idx} OR t.counterparty_name LIKE ?{idx} OR t.counterparty_account LIKE ?{idx} OR b.batch_no LIKE ?{idx})"
        ));
        params_vec.push(Box::new(format!("%{}%", keyword.trim())));
    }
    if let Some(account_id) = query.fund_account_id {
        where_clauses.push(format!("t.fund_account_id = ?{idx}"));
        params_vec.push(Box::new(account_id));
    }

    let sql = format!(
        "SELECT t.id, t.transaction_date, t.belong_month, t.summary, t.counterparty_name,
                t.counterparty_account, t.income_amount, t.expense_amount, t.balance, t.status,
                t.ignore_reason, t.imported_file, t.raw_json, t.created_at, t.updated_at,
                b.id, b.batch_no, b.batch_type, b.total_amount, m.match_score, m.remark,
                t.fund_account_id, fa.name,
                COALESCE(al.s, 0) + COALESCE(lg.s, 0),
                (CASE WHEN t.income_amount > 0.005 THEN t.income_amount
                      WHEN t.expense_amount > 0.005 THEN t.expense_amount ELSE 0 END)
                    - (COALESCE(al.s, 0) + COALESCE(lg.s, 0))
         FROM bank_transactions t
         LEFT JOIN bank_transaction_matches m ON m.transaction_id = t.id AND m.status = 'active'
         LEFT JOIN payment_batches b ON b.id = m.payment_batch_id
         LEFT JOIN fund_accounts fa ON fa.id = t.fund_account_id
         LEFT JOIN (SELECT transaction_id AS tid, SUM(allocated_amount) AS s
                    FROM bank_reconciliation_allocations WHERE status = 'active'
                    GROUP BY transaction_id) al ON al.tid = t.id
         LEFT JOIN (SELECT m2.transaction_id AS tid, SUM(b2.total_amount) AS s
                    FROM bank_transaction_matches m2
                    JOIN payment_batches b2 ON b2.id = m2.payment_batch_id
                    WHERE m2.status = 'active' AND NOT EXISTS (
                        SELECT 1 FROM bank_reconciliation_allocations x
                        WHERE x.legacy_match_id = m2.id)
                    GROUP BY m2.transaction_id) lg ON lg.tid = t.id
         WHERE {}
         ORDER BY t.transaction_date DESC, t.id DESC",
        where_clauses.join(" AND ")
    );
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|param| param.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), row_to_bank_transaction)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

/// 流水导入目标账户校验（spec 4.8）：必须存在、启用且为 bank/third_party；现金账户不可导入银行流水
pub(crate) fn ensure_bank_import_account(
    conn: &Connection,
    account_id: i64,
) -> AppResult<crate::models::FundAccount> {
    let account = crate::cashier::get_fund_account(conn, account_id)?;
    if account.account_type == "cash" {
        return Err(AppError::InvalidParam(
            "现金账户不能导入银行流水，请选择银行或第三方支付账户".into(),
        ));
    }
    if !account.is_active {
        return Err(AppError::InvalidParam(format!(
            "资金账户 {} 已停用，不能导入银行流水",
            account.account_code
        )));
    }
    Ok(account)
}

pub fn insert_bank_transaction(conn: &Connection, tx: &BankTransaction) -> AppResult<bool> {
    ensure_month_open(conn, &tx.belong_month)?;
    if let Some(account_id) = tx.fund_account_id {
        ensure_bank_import_account(conn, account_id)?;
    }
    let now = Utc::now().to_rfc3339();
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO bank_transactions
            (transaction_date, belong_month, summary, counterparty_name, counterparty_account,
             income_amount, expense_amount, balance, status, ignore_reason, imported_file, raw_json,
             fund_account_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'unmatched', NULL, ?9, ?10, ?11, ?12, ?13)",
        params![
            tx.transaction_date,
            tx.belong_month,
            tx.summary,
            tx.counterparty_name,
            tx.counterparty_account,
            tx.income_amount,
            tx.expense_amount,
            tx.balance,
            tx.imported_file,
            tx.raw_json,
            tx.fund_account_id,
            now,
            now
        ],
    )?;
    Ok(inserted > 0)
}

pub fn get_bank_transaction(conn: &Connection, id: i64) -> AppResult<BankTransaction> {
    query_bank_transactions(
        conn,
        &BankTransactionQuery {
            ..Default::default()
        },
    )?
    .into_iter()
    .find(|tx| tx.id == id)
    .ok_or_else(|| AppError::NotFound(format!("银行流水ID={id}未找到")))
}

/// 银行流水导入预览（spec 4.8/Task 11）：解析结果先行展示，确认后才入库。
/// 账户必选且须为启用的 bank/third_party 账户；逐行给出收支方向、
/// 去重冲突（与库内同账户唯一索引口径一致 + 文件内重复）、月结拦截与余额列衔接校验。
/// 本函数只读，不写库。
pub fn preview_bank_transaction_import(
    conn: &Connection,
    path: &str,
    fund_account_id: i64,
) -> AppResult<BankImportPreview> {
    let account = ensure_bank_import_account(conn, fund_account_id)?;
    let content = crate::excel::read_bank_transactions_file_content(path)?;

    let mut rows: Vec<BankImportPreviewRow> = Vec::new();
    let mut ok_rows = 0i32;
    let mut duplicate_rows = 0i32;
    let mut warning_rows = 0i32;
    let mut error_rows = 0i32;
    let mut income_total = 0.0f64;
    let mut expense_total = 0.0f64;
    let mut seen_in_file: std::collections::HashSet<(
        String,
        String,
        String,
        String,
        i64,
        i64,
        i64,
    )> = std::collections::HashSet::new();

    for (idx, tx) in content.transactions.iter().enumerate() {
        let row_no = (idx + 1) as i32;
        let direction = if tx.income_amount > 0.0 && tx.expense_amount > 0.0 {
            "invalid"
        } else if tx.income_amount > 0.0 {
            "income"
        } else {
            "expense"
        };
        let mut row_status = "ok".to_string();
        let mut messages: Vec<String> = Vec::new();

        // 收支方向校验
        if direction == "invalid" {
            row_status = "error".into();
            messages.push("收入与支出同时有值".into());
        }

        // 月结保护：目标月已结 → 该行不能导入
        if let Err(e) = ensure_month_open(conn, &tx.belong_month) {
            row_status = "error".into();
            messages.push(e.to_string());
        }

        // 去重冲突：与唯一索引 idx_bank_transactions_dedup 同口径（含账户维度，spec 4.8）
        let dup_key = (
            tx.transaction_date.clone(),
            tx.summary.clone().unwrap_or_default(),
            tx.counterparty_name.clone().unwrap_or_default(),
            tx.counterparty_account.clone().unwrap_or_default(),
            (tx.income_amount * 100.0).round() as i64,
            (tx.expense_amount * 100.0).round() as i64,
            (tx.balance.unwrap_or(0.0) * 100.0).round() as i64,
        );
        let db_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM bank_transactions
             WHERE transaction_date=?1 AND COALESCE(summary,'')=COALESCE(?2,'')
               AND COALESCE(counterparty_name,'')=COALESCE(?3,'')
               AND COALESCE(counterparty_account,'')=COALESCE(?4,'')
               AND income_amount=?5 AND expense_amount=?6
               AND COALESCE(balance,0)=?7 AND COALESCE(fund_account_id,0)=?8",
            params![
                tx.transaction_date,
                tx.summary,
                tx.counterparty_name,
                tx.counterparty_account,
                tx.income_amount,
                tx.expense_amount,
                tx.balance.unwrap_or(0.0),
                fund_account_id
            ],
            |r| r.get(0),
        )?;
        let in_file_dup = !seen_in_file.insert(dup_key);
        if db_count > 0 || in_file_dup {
            if row_status == "ok" {
                row_status = "duplicate".into();
            }
            messages.push(if db_count > 0 {
                "与库内同账户流水重复，导入时将跳过".to_string()
            } else {
                "文件内重复行，导入时仅保留一条".to_string()
            });
        }

        // 余额列衔接校验（流水余额为交易后余额：上行余额 + 本行收入 - 本行支出 = 本行余额，容差 0.005）
        if row_status != "error" {
            if let Some(prev) = rows.last() {
                if let (Some(prev_balance), Some(cur_balance)) = (prev.balance, tx.balance) {
                    let expected = prev_balance + tx.income_amount - tx.expense_amount;
                    if (expected - cur_balance).abs() > 0.005 {
                        if row_status == "ok" {
                            row_status = "warning".into();
                        }
                        messages.push(format!(
                            "余额与上一行不衔接（按上行推算应为 {expected:.2}）"
                        ));
                    }
                }
            }
        }

        match row_status.as_str() {
            "ok" => ok_rows += 1,
            "duplicate" => duplicate_rows += 1,
            "warning" => warning_rows += 1,
            _ => error_rows += 1,
        }
        income_total += tx.income_amount;
        expense_total += tx.expense_amount;

        rows.push(BankImportPreviewRow {
            row_no,
            transaction_date: tx.transaction_date.clone(),
            belong_month: tx.belong_month.clone(),
            summary: tx.summary.clone(),
            counterparty_name: tx.counterparty_name.clone(),
            counterparty_account: tx.counterparty_account.clone(),
            income_amount: tx.income_amount,
            expense_amount: tx.expense_amount,
            direction: direction.to_string(),
            balance: tx.balance,
            row_status,
            message: if messages.is_empty() {
                None
            } else {
                Some(messages.join("；"))
            },
        });
    }

    Ok(BankImportPreview {
        fund_account_id: account.id,
        fund_account_name: account.name,
        file_path: path.to_string(),
        headers: content.headers,
        total_rows: rows.len() as i32,
        ok_rows,
        duplicate_rows,
        warning_rows,
        error_rows,
        income_total,
        expense_total,
        rows,
    })
}

pub fn confirm_bank_transaction_match(
    conn: &Connection,
    input: &BankTransactionMatchInput,
    score: i32,
) -> AppResult<BankTransactionMatch> {
    let tx = get_bank_transaction(conn, input.transaction_id)?;
    ensure_month_open(conn, &tx.belong_month)?;
    if tx.status == "ignored" {
        return Err(AppError::InvalidParam("已忽略流水不能匹配".into()));
    }
    let batch = get_payment_batch(conn, input.payment_batch_id)?;
    ensure_month_open(conn, &batch.belong_month)?;
    if batch.status != "paid" {
        return Err(AppError::InvalidParam("只能匹配已付款批次".into()));
    }
    if tx.belong_month != batch.belong_month {
        return Err(AppError::InvalidParam(
            "银行流水月份与付款批次月份不一致".into(),
        ));
    }
    // 账户维度一致（spec 4.8/4.9：不同账户流水不得核销同一次付款）
    if tx.fund_account_id.is_some() && tx.fund_account_id != batch.fund_account_id {
        return Err(AppError::InvalidParam(
            "银行流水与付款批次的资金账户不一致，不能匹配".into(),
        ));
    }
    if (tx.expense_amount - batch.total_amount).abs() > 0.01 {
        return Err(AppError::InvalidParam(format!(
            "流水支出金额{:.2}与付款批次金额{:.2}不一致",
            tx.expense_amount, batch.total_amount
        )));
    }
    // 已生成 active bank_manual 入账凭证的流水不能再匹配付款批次：
    // 流水入账凭证与批次付款凭证会各贷记一次 1002，造成银行存款双重贷记
    let voucher_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers
         WHERE source_type='bank_manual' AND source_id=?1 AND status='active'",
        params![input.transaction_id],
        |r| r.get(0),
    )?;
    if voucher_count > 0 {
        return Err(AppError::General(
            "该流水已生成入账凭证，请先取消凭证或取消流水匹配后再匹配付款批次".into(),
        ));
    }
    // 退役拦截（Task 13，spec 4.9 双路径防重）：流水已在新对账引擎（allocation 或未迁移旧匹配）
    // 占用核销额时，旧批次匹配不得再写入——旧引擎不感知 allocation，双向并存会把流水侧
    // 已核销金额计到 200%。旧匹配表只读保留一个版本周期，写入口统一收敛到银行对账页。
    let tx_allocated = crate::cashier::bank_tx_allocated(conn, input.transaction_id)?;
    if tx_allocated > 0.005 {
        return Err(AppError::General(format!(
            "该流水已通过银行对账核销 {tx_allocated:.2}，请到「银行对账」页处理，不能重复匹配付款批次"
        )));
    }

    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE bank_transaction_matches
         SET status='cancelled'
         WHERE status='active' AND (transaction_id=?1 OR payment_batch_id=?2)",
        params![input.transaction_id, input.payment_batch_id],
    )?;
    conn.execute(
        "INSERT INTO bank_transaction_matches
            (transaction_id, payment_batch_id, match_score, remark, status, created_at)
         VALUES (?1, ?2, ?3, ?4, 'active', ?5)",
        params![
            input.transaction_id,
            input.payment_batch_id,
            score,
            input.remark,
            now
        ],
    )?;
    let match_id = conn.last_insert_rowid();
    conn.execute(
        "UPDATE bank_transactions SET status='matched', updated_at=?1 WHERE id=?2",
        params![now, input.transaction_id],
    )?;

    conn.query_row(
        "SELECT id, transaction_id, payment_batch_id, match_score, remark, created_at
         FROM bank_transaction_matches WHERE id=?1",
        params![match_id],
        row_to_bank_match,
    )
    .map_err(AppError::from)
}

pub fn cancel_bank_transaction_match(conn: &Connection, transaction_id: i64) -> AppResult<bool> {
    let tx = get_bank_transaction(conn, transaction_id)?;
    ensure_month_open(conn, &tx.belong_month)?;
    let now = Utc::now().to_rfc3339();
    let updated = conn.execute(
        "UPDATE bank_transaction_matches SET status='cancelled'
         WHERE transaction_id=?1 AND status='active'",
        params![transaction_id],
    )?;
    conn.execute(
        "UPDATE bank_transactions SET status='unmatched', updated_at=?1 WHERE id=?2",
        params![now, transaction_id],
    )?;
    // 取消匹配后流水回到 unmatched，其 bank_manual 凭证随之作废（spec 3.3）
    // void 有意不检查 updated>0：无论是否存在 active 匹配，都要清理旧 bank_manual 凭证，
    // 否则流水重新手工入账时会被 source 唯一索引（idx_vouchers_source_active）阻塞
    crate::accounting::void_vouchers_for_source(conn, "bank_manual", transaction_id)?;
    Ok(updated > 0)
}

pub fn ignore_bank_transaction(
    conn: &Connection,
    input: &BankTransactionIgnoreInput,
) -> AppResult<bool> {
    if input.reason.trim().is_empty() {
        return Err(AppError::InvalidParam("忽略原因必填".into()));
    }
    let tx = get_bank_transaction(conn, input.transaction_id)?;
    ensure_month_open(conn, &tx.belong_month)?;
    if tx.status == "matched" {
        return Err(AppError::InvalidParam("已匹配流水不能直接忽略".into()));
    }
    let now = Utc::now().to_rfc3339();
    let updated = conn.execute(
        "UPDATE bank_transactions SET status='ignored', ignore_reason=?1, updated_at=?2 WHERE id=?3",
        params![input.reason.trim(), now, input.transaction_id],
    )?;
    // 忽略流水后凭证作废（流水不再构成银行业务）
    crate::accounting::void_vouchers_for_source(conn, "bank_manual", input.transaction_id)?;
    Ok(updated > 0)
}

/// 自动匹配：金额相等的已付款批次与未匹配流水一一对应时匹配。
/// 已生成 active bank_manual 入账凭证的流水被排除——若再匹配批次，
/// 流水凭证与批次付款凭证会各贷记一次 1002（银行存款双重贷记）。
/// 待归集流水（fund_account_id IS NULL，spec 4.8）可查询但不参与自动匹配；
/// 流水与批次资金账户不同也不匹配（同账户为 spec 6.3 硬条件）。
/// 注：spec 6.3 评分升级（金额/单号/户名/尾号/摘要/日期距离 → 匹配账面资金分录）
/// 归 Task 12 多对多核销引擎实现，其硬条件依赖本任务的流水账户维度。
pub fn auto_match_bank_transactions(
    conn: &Connection,
    month: &str,
) -> AppResult<BankAutoMatchResult> {
    ensure_month_open(conn, month)?;
    let transactions = query_bank_transactions(
        conn,
        &BankTransactionQuery {
            belong_month: Some(month.to_string()),
            status: Some("unmatched".to_string()),
            keyword: None,
            fund_account_id: None,
        },
    )?
    .into_iter()
    // 待归集（账户 NULL）不自动匹配
    .filter(|tx| tx.fund_account_id.is_some())
    // 查询失败时保守排除（与下方 active_bank_match_* 的 unwrap_or(true) 口径一致）
    .filter(|tx| !active_bank_manual_voucher_exists(conn, tx.id).unwrap_or(true))
    // 退役拦截（Task 13）：流水已被新对账引擎/旧匹配占用核销额时跳过，
    // 不再进入旧 confirm（其内部同样有 bank_tx_allocated 拦截，双保险）
    .filter(|tx| {
        crate::cashier::bank_tx_allocated(conn, tx.id)
            .map(|allocated| allocated <= 0.005)
            .unwrap_or(true)
    })
    .collect::<Vec<_>>();
    let batches = query_payment_batches(
        conn,
        &PaymentBatchQuery {
            belong_month: Some(month.to_string()),
            status: Some("paid".to_string()),
            batch_type: None,
        },
    )?;

    let mut matched = 0;
    let mut skipped = 0;
    let mut errors = Vec::new();
    for tx in transactions {
        let candidates: Vec<&PaymentBatch> = batches
            .iter()
            .filter(|batch| (tx.expense_amount - batch.total_amount).abs() <= 0.01)
            // 资金账户一致（批次自 Task 9 起必有账户）
            .filter(|batch| batch.fund_account_id == tx.fund_account_id)
            .filter(|batch| {
                !active_bank_match_for_batch_exists(conn, batch.id).unwrap_or(true)
                    && !active_bank_match_for_transaction_exists(conn, tx.id).unwrap_or(true)
            })
            .collect();

        if candidates.len() != 1 {
            skipped += 1;
            continue;
        }
        let batch = candidates[0];
        let score = bank_match_score(&tx, batch);
        match confirm_bank_transaction_match(
            conn,
            &BankTransactionMatchInput {
                transaction_id: tx.id,
                payment_batch_id: batch.id,
                remark: Some("自动匹配".to_string()),
            },
            score,
        ) {
            Ok(_) => matched += 1,
            Err(e) => {
                skipped += 1;
                errors.push(format!("流水ID={}：{e}", tx.id));
            }
        }
    }

    Ok(BankAutoMatchResult {
        success: true,
        matched,
        skipped,
        errors,
    })
}

fn active_bank_match_for_batch_exists(conn: &Connection, batch_id: i64) -> AppResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM bank_transaction_matches
         WHERE payment_batch_id=?1 AND status='active'",
        params![batch_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn active_bank_match_for_transaction_exists(
    conn: &Connection,
    transaction_id: i64,
) -> AppResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM bank_transaction_matches
         WHERE transaction_id=?1 AND status='active'",
        params![transaction_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// 流水是否已有 active bank_manual 入账凭证。有则该流水不能再匹配付款批次（防双重贷记 1002）。
fn active_bank_manual_voucher_exists(conn: &Connection, transaction_id: i64) -> AppResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers
         WHERE source_type='bank_manual' AND source_id=?1 AND status='active'",
        params![transaction_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn bank_match_score(tx: &BankTransaction, batch: &PaymentBatch) -> i32 {
    let mut score = 80;
    let haystack = format!(
        "{} {} {}",
        tx.summary.as_deref().unwrap_or(""),
        tx.counterparty_name.as_deref().unwrap_or(""),
        tx.counterparty_account.as_deref().unwrap_or("")
    );
    if haystack.contains(&batch.batch_no) {
        score += 15;
    }
    if batch.batch_type == "salary" && haystack.contains("工资") {
        score += 5;
    }
    if batch.batch_type == "reimbursement" && haystack.contains("报销") {
        score += 5;
    }
    score.min(100)
}

// ==================== Financial Analysis ====================

pub fn get_financial_analysis(
    conn: &Connection,
    query: &FinancialAnalysisQuery,
) -> AppResult<FinancialAnalysisReport> {
    let months = query.months.unwrap_or(6).clamp(2, 24);
    let trend_months = month_window(&query.month, months as usize);
    let comparison_months = month_window(&query.month, 2);

    Ok(FinancialAnalysisReport {
        month: query.month.clone(),
        months,
        department_costs: get_department_cost_analysis(conn, &query.month)?,
        expense_trends: get_expense_type_trends(conn, &trend_months)?,
        employee_costs: get_employee_cost_views(conn, &query.month)?,
        budget_executions: get_budget_executions(conn, &query.month)?,
        monthly_comparison: get_monthly_comparison(conn, &comparison_months)?,
    })
}

// ==================== Budgets ====================

fn row_to_budget(row: &rusqlite::Row<'_>) -> rusqlite::Result<Budget> {
    Ok(Budget {
        id: row.get(0)?,
        month: row.get(1)?,
        department: row.get(2)?,
        expense_type_code: row.get(3)?,
        expense_type_name: row.get(4)?,
        budget_amount: row.get(5)?,
        remark: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

pub fn query_budgets(conn: &Connection, query: &BudgetQuery) -> AppResult<Vec<Budget>> {
    let mut where_clauses = vec!["1=1".to_string()];
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(month) = query
        .month
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        where_clauses.push(format!("b.month = ?{idx}"));
        params_vec.push(Box::new(month.clone()));
        idx += 1;
    }
    if let Some(department) = query
        .department
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        where_clauses.push(format!("COALESCE(b.department, '') = ?{idx}"));
        params_vec.push(Box::new(department.clone()));
        idx += 1;
    }
    if let Some(expense_type_code) = query
        .expense_type_code
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        where_clauses.push(format!("COALESCE(b.expense_type_code, '') = ?{idx}"));
        params_vec.push(Box::new(expense_type_code.clone()));
    }

    let sql = format!(
        "SELECT b.id, b.month, b.department, b.expense_type_code, t.name,
                b.budget_amount, b.remark, b.created_at, b.updated_at
         FROM budgets b
         LEFT JOIN invoice_expense_types t ON t.code = b.expense_type_code
         WHERE {}
         ORDER BY b.month DESC, COALESCE(b.department, ''), COALESCE(t.name, b.expense_type_code, '')",
        where_clauses.join(" AND ")
    );
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|param| param.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), row_to_budget)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn save_budget(conn: &Connection, input: &BudgetInput) -> AppResult<Budget> {
    let month = input.month.trim();
    if month.is_empty() {
        return Err(AppError::InvalidParam("预算月份必填".into()));
    }
    if input.budget_amount < 0.0 {
        return Err(AppError::InvalidParam("预算金额不能为负数".into()));
    }
    ensure_month_open(conn, month)?;
    let now = Utc::now().to_rfc3339();
    let department = input.department.as_ref().and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });
    let expense_type_code = input.expense_type_code.as_ref().and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });

    if let Some(id) = input.id {
        conn.execute(
            "UPDATE budgets
             SET month=?1, department=?2, expense_type_code=?3, budget_amount=?4, remark=?5, updated_at=?6
             WHERE id=?7",
            params![
                month,
                department,
                expense_type_code,
                input.budget_amount,
                input.remark,
                now,
                id
            ],
        )?;
        get_budget(conn, id)
    } else {
        let existing_id = conn.query_row(
            "SELECT id FROM budgets
             WHERE month=?1 AND COALESCE(department,'')=COALESCE(?2,'')
             AND COALESCE(expense_type_code,'')=COALESCE(?3,'')",
            params![month, department, expense_type_code],
            |row| row.get(0),
        );
        let id = match existing_id {
            Ok(id) => {
                conn.execute(
                    "UPDATE budgets
                     SET budget_amount=?1, remark=?2, updated_at=?3
                     WHERE id=?4",
                    params![input.budget_amount, input.remark, now, id],
                )?;
                id
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                conn.execute(
                    "INSERT INTO budgets
                        (month, department, expense_type_code, budget_amount, remark, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        month,
                        department,
                        expense_type_code,
                        input.budget_amount,
                        input.remark,
                        now,
                        now
                    ],
                )?;
                conn.last_insert_rowid()
            }
            Err(e) => return Err(AppError::from(e)),
        };
        get_budget(conn, id)
    }
}

pub fn delete_budget(conn: &Connection, id: i64) -> AppResult<bool> {
    let budget = get_budget(conn, id)?;
    ensure_month_open(conn, &budget.month)?;
    let updated = conn.execute("DELETE FROM budgets WHERE id=?1", params![id])?;
    Ok(updated > 0)
}

fn get_budget(conn: &Connection, id: i64) -> AppResult<Budget> {
    conn.query_row(
        "SELECT b.id, b.month, b.department, b.expense_type_code, t.name,
                b.budget_amount, b.remark, b.created_at, b.updated_at
         FROM budgets b
         LEFT JOIN invoice_expense_types t ON t.code = b.expense_type_code
         WHERE b.id=?1",
        params![id],
        row_to_budget,
    )
    .map_err(|e| AppError::NotFound(format!("预算ID={id}未找到: {e}")))
}

pub fn get_budget_executions(conn: &Connection, month: &str) -> AppResult<Vec<BudgetExecution>> {
    let budgets = query_budgets(
        conn,
        &BudgetQuery {
            month: Some(month.to_string()),
            ..Default::default()
        },
    )?;
    let department_costs = get_department_cost_analysis(conn, month)?;
    let expense_trends = get_expense_type_trends(conn, &[month.to_string()])?;

    let mut result = Vec::new();
    for budget in budgets {
        let actual_amount = budget_actual_amount(&budget, &department_costs, &expense_trends);
        let usage_percent = if budget.budget_amount <= 0.0 {
            0.0
        } else {
            actual_amount / budget.budget_amount * 100.0
        };
        let over_amount = (actual_amount - budget.budget_amount).max(0.0);
        let status = if budget.budget_amount > 0.0 && actual_amount > budget.budget_amount {
            "over"
        } else {
            "ok"
        };
        result.push(BudgetExecution {
            budget,
            actual_amount,
            usage_percent,
            over_amount,
            status: status.to_string(),
        });
    }
    result.sort_by(|a, b| {
        b.usage_percent
            .partial_cmp(&a.usage_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(result)
}

fn budget_actual_amount(
    budget: &Budget,
    department_costs: &[DepartmentCostAnalysis],
    expense_trends: &[ExpenseTypeTrend],
) -> f64 {
    if let Some(expense_type_code) = budget.expense_type_code.as_deref() {
        return expense_trends
            .iter()
            .filter(|item| item.expense_type_code == expense_type_code)
            .map(|item| item.invoice_amount + item.reimbursement_amount)
            .sum();
    }
    if let Some(department) = budget.department.as_deref() {
        return department_costs
            .iter()
            .find(|item| item.department == department)
            .map(|item| item.total_cost)
            .unwrap_or(0.0);
    }
    department_costs.iter().map(|item| item.total_cost).sum()
}

fn get_department_cost_analysis(
    conn: &Connection,
    month: &str,
) -> AppResult<Vec<DepartmentCostAnalysis>> {
    let mut by_department: HashMap<String, DepartmentCostAnalysis> = HashMap::new();

    {
        let mut stmt = conn.prepare(
            "SELECT
                COALESCE(NULLIF(TRIM(department), ''), '未分配') AS department,
                COUNT(DISTINCT employee_no) AS employee_count,
                COALESCE(SUM(gross_salary), 0),
                COALESCE(SUM(social_security_personal), 0),
                COALESCE(SUM(housing_fund_personal), 0)
             FROM salary_monthly_results
             WHERE salary_month = ?1
             GROUP BY COALESCE(NULLIF(TRIM(department), ''), '未分配')",
        )?;
        let rows = stmt.query_map(params![month], |row| {
            let gross_salary: f64 = row.get(2)?;
            let social_security: f64 = row.get(3)?;
            let housing_fund: f64 = row.get(4)?;
            Ok(DepartmentCostAnalysis {
                department: row.get(0)?,
                employee_count: row.get::<_, i64>(1)? as i32,
                gross_salary,
                social_security,
                housing_fund,
                salary_cost: gross_salary + social_security + housing_fund,
                invoice_amount: 0.0,
                reimbursement_amount: 0.0,
                total_cost: gross_salary + social_security + housing_fund,
            })
        })?;
        for row in rows {
            let item = row?;
            by_department.insert(item.department.clone(), item);
        }
    }

    {
        let mut stmt = conn.prepare(
            "SELECT
                COALESCE(NULLIF(TRIM(e.department), ''), '未分配') AS department,
                COALESCE(SUM(i.total_amount), 0)
             FROM invoices i
             LEFT JOIN employees e ON e.id = i.employee_id
             WHERE i.belong_month = ?1 AND i.status != 'void'
             GROUP BY COALESCE(NULLIF(TRIM(e.department), ''), '未分配')",
        )?;
        let rows = stmt.query_map(params![month], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;
        for row in rows {
            let (department, amount) = row?;
            let item = by_department
                .entry(department.clone())
                .or_insert_with(|| empty_department_cost(&department));
            item.invoice_amount = amount;
            item.total_cost = item.salary_cost + item.invoice_amount + item.reimbursement_amount;
        }
    }

    {
        let mut stmt = conn.prepare(
            "SELECT
                COALESCE(NULLIF(TRIM(e.department), ''), '未分配') AS department,
                COALESCE(SUM(r.total_amount), 0)
             FROM reimbursement_claims r
             LEFT JOIN employees e ON e.id = r.employee_id
             WHERE r.belong_month = ?1 AND r.status = 'approved'
             GROUP BY COALESCE(NULLIF(TRIM(e.department), ''), '未分配')",
        )?;
        let rows = stmt.query_map(params![month], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;
        for row in rows {
            let (department, amount) = row?;
            let item = by_department
                .entry(department.clone())
                .or_insert_with(|| empty_department_cost(&department));
            item.reimbursement_amount = amount;
            item.total_cost = item.salary_cost + item.invoice_amount + item.reimbursement_amount;
        }
    }

    let mut result: Vec<_> = by_department.into_values().collect();
    result.sort_by(|a, b| {
        b.total_cost
            .partial_cmp(&a.total_cost)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.department.cmp(&b.department))
    });
    Ok(result)
}

fn empty_department_cost(department: &str) -> DepartmentCostAnalysis {
    DepartmentCostAnalysis {
        department: department.to_string(),
        employee_count: 0,
        gross_salary: 0.0,
        social_security: 0.0,
        housing_fund: 0.0,
        salary_cost: 0.0,
        invoice_amount: 0.0,
        reimbursement_amount: 0.0,
        total_cost: 0.0,
    }
}

fn get_expense_type_trends(
    conn: &Connection,
    months: &[String],
) -> AppResult<Vec<ExpenseTypeTrend>> {
    let mut expense_types = get_enabled_expense_type_names(conn)?;
    let mut seen_codes: BTreeSet<String> = expense_types.keys().cloned().collect();
    let mut by_key: HashMap<(String, String), ExpenseTypeTrend> = HashMap::new();

    for month in months {
        for (code, name) in &expense_types {
            by_key.insert(
                (month.clone(), code.clone()),
                ExpenseTypeTrend {
                    month: month.clone(),
                    expense_type_code: code.clone(),
                    expense_type_name: name.clone(),
                    invoice_count: 0,
                    invoice_amount: 0.0,
                    reimbursement_amount: 0.0,
                },
            );
        }
    }

    if let (Some(start_month), Some(end_month)) = (months.first(), months.last()) {
        let mut stmt = conn.prepare(
            "SELECT
                i.belong_month,
                COALESCE(NULLIF(TRIM(i.expense_type_code), ''), 'uncategorized') AS code,
                COALESCE(t.name, '未归类') AS name,
                COUNT(*),
                COALESCE(SUM(i.total_amount), 0)
             FROM invoices i
             LEFT JOIN invoice_expense_types t ON t.code = i.expense_type_code
             WHERE i.belong_month >= ?1 AND i.belong_month <= ?2 AND i.status != 'void'
             GROUP BY i.belong_month, COALESCE(NULLIF(TRIM(i.expense_type_code), ''), 'uncategorized'), COALESCE(t.name, '未归类')",
        )?;
        let rows = stmt.query_map(params![start_month, end_month], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)? as i32,
                row.get::<_, f64>(4)?,
            ))
        })?;
        for row in rows {
            let (month, code, name, invoice_count, invoice_amount) = row?;
            seen_codes.insert(code.clone());
            expense_types.entry(code.clone()).or_insert(name.clone());
            let item = by_key
                .entry((month.clone(), code.clone()))
                .or_insert(ExpenseTypeTrend {
                    month,
                    expense_type_code: code,
                    expense_type_name: name,
                    invoice_count: 0,
                    invoice_amount: 0.0,
                    reimbursement_amount: 0.0,
                });
            item.invoice_count = invoice_count;
            item.invoice_amount = invoice_amount;
        }

        let mut stmt = conn.prepare(
            "SELECT
                r.belong_month,
                COALESCE(NULLIF(TRIM(i.expense_type_code), ''), 'uncategorized') AS code,
                COALESCE(t.name, '未归类') AS name,
                COALESCE(SUM(i.total_amount), 0)
             FROM reimbursement_claims r
             JOIN reimbursement_claim_invoices ri ON ri.claim_id = r.id
             JOIN invoices i ON i.id = ri.invoice_id
             LEFT JOIN invoice_expense_types t ON t.code = i.expense_type_code
             WHERE r.belong_month >= ?1 AND r.belong_month <= ?2
               AND r.status = 'approved'
               AND i.status != 'void'
             GROUP BY r.belong_month, COALESCE(NULLIF(TRIM(i.expense_type_code), ''), 'uncategorized'), COALESCE(t.name, '未归类')",
        )?;
        let rows = stmt.query_map(params![start_month, end_month], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?;
        for row in rows {
            let (month, code, name, reimbursement_amount) = row?;
            seen_codes.insert(code.clone());
            expense_types.entry(code.clone()).or_insert(name.clone());
            let item = by_key
                .entry((month.clone(), code.clone()))
                .or_insert(ExpenseTypeTrend {
                    month,
                    expense_type_code: code,
                    expense_type_name: name,
                    invoice_count: 0,
                    invoice_amount: 0.0,
                    reimbursement_amount: 0.0,
                });
            item.reimbursement_amount = reimbursement_amount;
        }
    }

    for month in months {
        for code in seen_codes.iter() {
            let name = expense_types
                .get(code)
                .cloned()
                .unwrap_or_else(|| "未归类".to_string());
            by_key
                .entry((month.clone(), code.clone()))
                .or_insert(ExpenseTypeTrend {
                    month: month.clone(),
                    expense_type_code: code.clone(),
                    expense_type_name: name,
                    invoice_count: 0,
                    invoice_amount: 0.0,
                    reimbursement_amount: 0.0,
                });
        }
    }

    let mut result: Vec<_> = by_key.into_values().collect();
    result.sort_by(|a, b| {
        a.month
            .cmp(&b.month)
            .then_with(|| a.expense_type_name.cmp(&b.expense_type_name))
    });
    Ok(result)
}

fn get_enabled_expense_type_names(conn: &Connection) -> AppResult<HashMap<String, String>> {
    let mut stmt = conn.prepare(
        "SELECT code, name FROM invoice_expense_types WHERE enabled = 1 ORDER BY sort_order, id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut result = HashMap::new();
    for row in rows {
        let (code, name) = row?;
        result.insert(code, name);
    }
    Ok(result)
}

fn get_employee_cost_views(conn: &Connection, month: &str) -> AppResult<Vec<EmployeeCostView>> {
    let mut by_no: HashMap<String, EmployeeCostView> = HashMap::new();
    let mut id_to_no: HashMap<i64, String> = HashMap::new();

    {
        let mut stmt = conn.prepare(
            "SELECT id, employee_no, name, COALESCE(NULLIF(TRIM(department), ''), '未分配')
             FROM employees
             ORDER BY department, name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (id, employee_no, name, department) = row?;
            id_to_no.insert(id, employee_no.clone());
            by_no.insert(
                employee_no.clone(),
                empty_employee_cost(Some(id), &employee_no, &name, &department),
            );
        }
    }

    {
        let mut stmt = conn.prepare(
            "SELECT
                s.employee_no,
                COALESCE(s.name, e.name, ''),
                COALESCE(NULLIF(TRIM(s.department), ''), NULLIF(TRIM(e.department), ''), '未分配'),
                e.id,
                COALESCE(SUM(s.gross_salary), 0),
                COALESCE(SUM(s.net_salary), 0),
                COALESCE(SUM(s.social_security_personal), 0),
                COALESCE(SUM(s.housing_fund_personal), 0),
                COALESCE(SUM(s.attendance_deduction), 0)
             FROM salary_monthly_results s
             LEFT JOIN employees e ON e.employee_no = s.employee_no
             WHERE s.salary_month = ?1
             GROUP BY s.employee_no, COALESCE(s.name, e.name, ''), COALESCE(NULLIF(TRIM(s.department), ''), NULLIF(TRIM(e.department), ''), '未分配'), e.id",
        )?;
        let rows = stmt.query_map(params![month], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, f64>(5)?,
                row.get::<_, f64>(6)?,
                row.get::<_, f64>(7)?,
                row.get::<_, f64>(8)?,
            ))
        })?;
        for row in rows {
            let (
                employee_no,
                name,
                department,
                employee_id,
                gross_salary,
                net_salary,
                social_security,
                housing_fund,
                attendance_deduction,
            ) = row?;
            let item = by_no.entry(employee_no.clone()).or_insert_with(|| {
                empty_employee_cost(employee_id, &employee_no, &name, &department)
            });
            item.name = name;
            item.department = department;
            item.employee_id = employee_id.or(item.employee_id);
            item.gross_salary = gross_salary;
            item.net_salary = net_salary;
            item.social_security = social_security;
            item.housing_fund = housing_fund;
            item.attendance_deduction = attendance_deduction;
            item.total_cost = item.gross_salary
                + item.social_security
                + item.housing_fund
                + item.reimbursement_amount;
        }
    }

    {
        let mut stmt = conn.prepare(
            "SELECT employee_no, COUNT(*)
             FROM attendance_records
             WHERE salary_month = ?1
               AND (actual_days < expected_days OR late_count > 0 OR early_leave_count > 0 OR absent_days > 0)
             GROUP BY employee_no",
        )?;
        let rows = stmt.query_map(params![month], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as i32))
        })?;
        for row in rows {
            let (employee_no, count) = row?;
            if let Some(item) = by_no.get_mut(&employee_no) {
                item.abnormal_attendance_count = count;
            }
        }
    }

    {
        let mut stmt = conn.prepare(
            "SELECT employee_id, COALESCE(SUM(total_amount), 0)
             FROM invoices
             WHERE belong_month = ?1 AND status != 'void' AND employee_id IS NOT NULL
             GROUP BY employee_id",
        )?;
        let rows = stmt.query_map(params![month], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
        })?;
        for row in rows {
            let (employee_id, amount) = row?;
            let employee_no = id_to_no
                .get(&employee_id)
                .cloned()
                .unwrap_or_else(|| format!("ID-{employee_id}"));
            let item = by_no.entry(employee_no.clone()).or_insert_with(|| {
                empty_employee_cost(Some(employee_id), &employee_no, "未知员工", "未分配")
            });
            item.invoice_amount = amount;
        }
    }

    {
        let mut stmt = conn.prepare(
            "SELECT employee_id, COALESCE(SUM(total_amount), 0)
             FROM reimbursement_claims
             WHERE belong_month = ?1 AND status = 'approved' AND employee_id IS NOT NULL
             GROUP BY employee_id",
        )?;
        let rows = stmt.query_map(params![month], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
        })?;
        for row in rows {
            let (employee_id, amount) = row?;
            let employee_no = id_to_no
                .get(&employee_id)
                .cloned()
                .unwrap_or_else(|| format!("ID-{employee_id}"));
            let item = by_no.entry(employee_no.clone()).or_insert_with(|| {
                empty_employee_cost(Some(employee_id), &employee_no, "未知员工", "未分配")
            });
            item.reimbursement_amount = amount;
            item.total_cost = item.gross_salary
                + item.social_security
                + item.housing_fund
                + item.reimbursement_amount;
        }
    }

    let mut result: Vec<_> = by_no
        .into_values()
        .filter(|item| {
            item.gross_salary != 0.0
                || item.invoice_amount != 0.0
                || item.reimbursement_amount != 0.0
                || item.abnormal_attendance_count != 0
        })
        .collect();
    result.sort_by(|a, b| {
        b.total_cost
            .partial_cmp(&a.total_cost)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.department.cmp(&b.department))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(result)
}

fn empty_employee_cost(
    employee_id: Option<i64>,
    employee_no: &str,
    name: &str,
    department: &str,
) -> EmployeeCostView {
    EmployeeCostView {
        employee_id,
        employee_no: employee_no.to_string(),
        name: name.to_string(),
        department: department.to_string(),
        gross_salary: 0.0,
        net_salary: 0.0,
        social_security: 0.0,
        housing_fund: 0.0,
        attendance_deduction: 0.0,
        invoice_amount: 0.0,
        reimbursement_amount: 0.0,
        abnormal_attendance_count: 0,
        total_cost: 0.0,
    }
}

fn get_monthly_comparison(
    conn: &Connection,
    months: &[String],
) -> AppResult<Vec<MonthlyComparison>> {
    let mut result = Vec::new();
    for month in months {
        let gross_salary: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(gross_salary), 0) FROM salary_monthly_results WHERE salary_month = ?1",
                params![month],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        let net_salary: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(net_salary), 0) FROM salary_monthly_results WHERE salary_month = ?1",
                params![month],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        let deduction: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(attendance_deduction + tax_amount + other_deduction), 0)
                 FROM salary_monthly_results WHERE salary_month = ?1",
                params![month],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        let social_security: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(social_security_personal), 0) FROM salary_monthly_results WHERE salary_month = ?1",
                params![month],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        let housing_fund: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(housing_fund_personal), 0) FROM salary_monthly_results WHERE salary_month = ?1",
                params![month],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        let invoice_amount: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(total_amount), 0) FROM invoices WHERE belong_month = ?1 AND status != 'void'",
                params![month],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        let reimbursement_amount: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(total_amount), 0) FROM reimbursement_claims WHERE belong_month = ?1 AND status = 'approved'",
                params![month],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        result.push(MonthlyComparison {
            month: month.clone(),
            gross_salary,
            net_salary,
            deduction,
            social_security,
            housing_fund,
            invoice_amount,
            reimbursement_amount,
            total_cost: gross_salary + social_security + housing_fund + reimbursement_amount,
        });
    }
    Ok(result)
}

fn month_window(end_month: &str, count: usize) -> Vec<String> {
    let (mut year, mut month) = parse_month(end_month);
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        result.push(format!("{year:04}-{month:02}"));
        if month == 1 {
            year -= 1;
            month = 12;
        } else {
            month -= 1;
        }
    }
    result.reverse();
    result
}

fn parse_month(month: &str) -> (i32, u32) {
    let date = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .unwrap_or_else(|_| Utc::now().date_naive());
    (date.year(), date.month())
}

// ==================== App Settings ====================

pub fn get_setting(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM app_settings WHERE key = ?1")?;
    let result = stmt.query_row(params![key], |row| row.get::<_, String>(0));
    match result {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::from(e)),
    }
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO app_settings (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

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

pub fn insert_invoice_expense_type(
    conn: &Connection,
    data: &InvoiceExpenseTypeInput,
) -> AppResult<InvoiceExpenseType> {
    let code = data
        .code
        .as_ref()
        .ok_or_else(|| AppError::InvalidParam("code 必填".into()))?;
    let name = data
        .name
        .as_ref()
        .ok_or_else(|| AppError::InvalidParam("name 必填".into()))?;
    let sort_order = data.sort_order.unwrap_or(99);
    let enabled = data.enabled.unwrap_or(1);
    conn.execute(
        "INSERT INTO invoice_expense_types (code, name, sort_order, enabled, remark) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![code, name, sort_order, enabled, data.remark],
    )?;
    let id = conn.last_insert_rowid();
    Ok(InvoiceExpenseType {
        id,
        code: code.clone(),
        name: name.clone(),
        sort_order,
        enabled,
        remark: data.remark.clone(),
    })
}

pub fn update_invoice_expense_type(
    conn: &Connection,
    id: i64,
    data: &InvoiceExpenseTypeInput,
) -> AppResult<InvoiceExpenseType> {
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
        id,
        code: existing.code,
        name: name.clone(),
        sort_order,
        enabled,
        remark: remark.cloned(),
    })
}

pub fn delete_invoice_expense_type(conn: &Connection, id: i64) -> AppResult<bool> {
    // 不允许删除"other"
    let code: String = conn
        .query_row(
            "SELECT code FROM invoice_expense_types WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|e| AppError::NotFound(format!("费用类型ID={id}未找到: {e}")))?;

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

/// 按去重 key 查找已存在的发票。
/// - 全电票（数电票）没有发票代码，code 为 None 时按空字符串匹配。
/// - 配合 schema 中的 `(COALESCE(invoice_code, ''), invoice_number)` 唯一索引，
///   同号码 + 空代码会被正确去重。
pub fn find_invoice_by_dedup_key(
    conn: &Connection,
    code: Option<&str>,
    number: &str,
) -> AppResult<Option<Invoice>> {
    let code_str = normalize_invoice_code(code);
    let number_str = normalize_invoice_number(number);
    let sql = format!(
        "SELECT {INVOICE_SELECT_FIELDS} FROM invoices \
         WHERE COALESCE(invoice_code, '') = ?1 AND invoice_number = ?2 AND status != 'void' LIMIT 1"
    );
    let result = conn.query_row(&sql, params![code_str, number_str], row_to_invoice);
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

pub fn get_invoice_belong_month(conn: &Connection, id: i64) -> AppResult<Option<String>> {
    Ok(get_invoice(conn, id)?.belong_month)
}

fn normalize_invoice_code(code: Option<&str>) -> String {
    code.map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

fn normalize_invoice_number(number: &str) -> String {
    number.trim().to_lowercase()
}

fn normalized_invoice_input(data: &InvoiceInput) -> InvoiceInput {
    let mut normalized = data.clone();
    normalized.invoice_code = data
        .invoice_code
        .as_deref()
        .map(normalize_invoice_number)
        .filter(|s| !s.is_empty());
    normalized.invoice_number = data
        .invoice_number
        .as_deref()
        .map(normalize_invoice_number)
        .filter(|s| !s.is_empty());
    normalized
}

pub fn insert_invoice(
    conn: &Connection,
    data: &InvoiceInput,
    image_path: &str,
    image_encrypted: i64,
) -> AppResult<Invoice> {
    if let Some(month) = data.belong_month.as_deref() {
        ensure_month_open(conn, month)?;
    }
    let now = Utc::now().to_rfc3339();
    let data = normalized_invoice_input(data);
    // 发票写入与费用凭证生成同事务：凭证失败时发票插入一并回滚
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO invoices (invoice_code, invoice_number, invoice_type, issue_date, check_code, amount, tax_amount, total_amount, seller_name, seller_tax_id, buyer_name, buyer_tax_id, expense_type_code, employee_id, belong_month, status, remark, image_path, raw_ocr_json, image_encrypted, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 'normal', ?16, ?17, ?18, ?19, ?20, ?21)",
        params![
            data.invoice_code, data.invoice_number, data.invoice_type, data.issue_date,
            data.check_code, data.amount.unwrap_or(0.0), data.tax_amount.unwrap_or(0.0),
            data.total_amount.unwrap_or(0.0), data.seller_name, data.seller_tax_id,
            data.buyer_name, data.buyer_tax_id, data.expense_type_code, data.employee_id,
            data.belong_month, data.remark, image_path, data.raw_ocr_json, image_encrypted, now, now
        ],
    )?;
    let id = tx.last_insert_rowid();
    // 未挂报销的发票自动生成费用入账凭证（挂报销的随报销审批入账，见 3.2 防重复口径）
    crate::accounting::maybe_generate_invoice_expense_voucher(&tx, id)?;
    tx.commit()?;
    get_invoice(conn, id)
}

pub fn update_invoice(
    conn: &Connection,
    id: i64,
    data: &InvoiceInput,
    new_image_path: Option<&str>,
) -> AppResult<bool> {
    let now = Utc::now().to_rfc3339();
    let data = normalized_invoice_input(data);
    let existing = get_invoice(conn, id)?;
    if let Some(month) = existing.belong_month.as_deref() {
        ensure_month_open(conn, month)?;
    }
    if let Some(month) = data.belong_month.as_deref() {
        ensure_month_open(conn, month)?;
    }
    let image_path = new_image_path.unwrap_or(existing.image_path.as_deref().unwrap_or(""));
    let existing_code = existing
        .invoice_code
        .as_deref()
        .map(normalize_invoice_number)
        .filter(|s| !s.is_empty());
    let existing_number = existing
        .invoice_number
        .as_deref()
        .map(normalize_invoice_number)
        .filter(|s| !s.is_empty());

    // 若改了 code/number，需校验不撞其他记录（code 可空，支持全电票）
    let new_code = data.invoice_code.as_ref().or(existing_code.as_ref());
    let new_number = data.invoice_number.as_ref().or(existing_number.as_ref());
    if let Some(n) = new_number {
        if let Some(other) = find_invoice_by_dedup_key(conn, new_code.map(|s| s.as_str()), n)? {
            if other.id != id {
                let code_disp = new_code.cloned().unwrap_or_default();
                return Err(AppError::General(format!(
                    "发票代码{code_disp}+号码{n}已被记录ID={}占用",
                    other.id
                )));
            }
        }
    }

    // 发票更新与凭证重生成同事务：凭证失败时更新一并回滚
    let tx = conn.unchecked_transaction()?;
    let updated = tx.execute(
        "UPDATE invoices SET invoice_code=?1, invoice_number=?2, invoice_type=?3, issue_date=?4, check_code=?5, amount=?6, tax_amount=?7, total_amount=?8, seller_name=?9, seller_tax_id=?10, buyer_name=?11, buyer_tax_id=?12, expense_type_code=?13, employee_id=?14, belong_month=?15, remark=?16, image_path=?17, raw_ocr_json=?18, updated_at=?19 WHERE id=?20",
        params![
            new_code,
            new_number,
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
    if updated > 0 {
        // 金额/税额/费用类型/归属月份变化会影响凭证行，简单起见一律 void + 按新值重建（幂等）
        crate::accounting::void_invoice_expense_voucher(&tx, id)?;
        crate::accounting::maybe_generate_invoice_expense_voucher(&tx, id)?;
        // 已挂 approved 报销单的发票金额变化也要重报销计提：void + 重建
        let claim_ids: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT rc.claim_id FROM reimbursement_claim_invoices rc
                 JOIN reimbursement_claims c ON c.id = rc.claim_id
                 WHERE rc.invoice_id = ?1 AND c.status = 'approved'",
            )?;
            let rows = stmt.query_map(params![id], |r| r.get(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for claim_id in claim_ids {
            // 跨月防护：发票月份开放不代表报销单月份开放，计提凭证挂在 claim 的
            // belong_month 上，改动作废重建前必须校验该月未正式月结（fail-fast，放在 void 之前）
            let claim_month: Option<String> = tx.query_row(
                "SELECT belong_month FROM reimbursement_claims WHERE id = ?1",
                params![claim_id],
                |r| r.get(0),
            )?;
            if let Some(month) = claim_month.as_deref() {
                ensure_month_open(&tx, month)?;
            }
            crate::accounting::void_reimbursement_accrual_voucher(&tx, claim_id)?;
            crate::accounting::generate_reimbursement_accrual_voucher(&tx, claim_id)?;
        }
    }
    tx.commit()?;
    Ok(updated > 0)
}

pub fn soft_delete_invoice(conn: &Connection, id: i64) -> AppResult<bool> {
    if let Some(month) = get_invoice_belong_month(conn, id)? {
        ensure_month_open(conn, &month)?;
    }
    let now = Utc::now().to_rfc3339();
    // 发票作废与费用凭证作废同事务
    let tx = conn.unchecked_transaction()?;
    let updated = tx.execute(
        "UPDATE invoices SET status='void', updated_at=?1 WHERE id=?2 AND status != 'void'",
        params![now, id],
    )?;
    if updated > 0 {
        crate::accounting::void_invoice_expense_voucher(&tx, id)?;
        // 作废的发票若挂在 approved 报销单上，其余额仍留在 active 计提凭证中：
        // 仿 update_invoice，对每个关联 approved 报销单 void + 重建计提（同事务）
        // （测试最小 schema 可能没有报销表，此时跳过）
        if crate::accounting::table_exists(&tx, "reimbursement_claim_invoices") {
            let claim_ids: Vec<i64> = {
                let mut stmt = tx.prepare(
                    "SELECT rc.claim_id FROM reimbursement_claim_invoices rc
                     JOIN reimbursement_claims c ON c.id = rc.claim_id
                     WHERE rc.invoice_id = ?1 AND c.status = 'approved'",
                )?;
                let rows = stmt.query_map(params![id], |r| r.get(0))?;
                rows.collect::<Result<Vec<_>, _>>()?
            };
            for claim_id in claim_ids {
                // 跨月防护：与 update_invoice 同口径，作废重建计提前校验 claim 的
                // belong_month 未正式月结（fail-fast，放在 void 之前）
                let claim_month: Option<String> = tx.query_row(
                    "SELECT belong_month FROM reimbursement_claims WHERE id = ?1",
                    params![claim_id],
                    |r| r.get(0),
                )?;
                if let Some(month) = claim_month.as_deref() {
                    ensure_month_open(&tx, month)?;
                }
                crate::accounting::void_reimbursement_accrual_voucher(&tx, claim_id)?;
                crate::accounting::generate_reimbursement_accrual_voucher(&tx, claim_id)?;
            }
        }
        // 注：claim 的 total_amount/invoice_count 滞留旧值（含已作废发票）不在本函数刷新，
        // 由报销模块在重新编辑/保存报销单时重算（save_reimbursement_claim 全量重写这两个字段）
    }
    tx.commit()?;
    Ok(updated > 0)
}

pub fn query_invoices(conn: &Connection, q: &InvoiceQuery) -> AppResult<Vec<Invoice>> {
    let void_filter;
    let mut where_clauses: Vec<String> = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(s) = &q.status {
        where_clauses.push(format!("status = ?{idx}"));
        params_vec.push(Box::new(s.clone()));
        idx += 1;
    } else {
        void_filter = "status != 'void'";
        where_clauses.push(void_filter.to_string());
    }

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
    if let Some(kw) = &q.keyword {
        let pat = format!("%{kw}%");
        where_clauses.push(format!(
            "(seller_name LIKE ?{idx} OR buyer_name LIKE ?{idx} OR remark LIKE ?{idx})"
        ));
        params_vec.push(Box::new(pat));
    }

    let sql = format!(
        "SELECT {INVOICE_SELECT_FIELDS} FROM invoices WHERE {} ORDER BY issue_date DESC, id DESC",
        where_clauses.join(" AND ")
    );

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), row_to_invoice)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

// ==================== Reimbursements ====================

fn row_to_reimbursement_claim(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReimbursementClaim> {
    Ok(ReimbursementClaim {
        id: row.get(0)?,
        claim_no: row.get(1)?,
        employee_id: row.get(2)?,
        employee_name: row.get(3)?,
        department: row.get(4)?,
        belong_month: row.get(5)?,
        title: row.get(6)?,
        total_amount: row.get(7)?,
        invoice_count: row.get(8)?,
        status: row.get(9)?,
        payment_status: row.get(10)?,
        payment_date: row.get(11)?,
        remark: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

pub fn query_reimbursement_claims(
    conn: &Connection,
    q: &ReimbursementQuery,
) -> AppResult<Vec<ReimbursementClaim>> {
    let mut where_clauses: Vec<String> = vec!["c.status != 'void'".to_string()];
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(month) = q.belong_month.as_ref().filter(|v| !v.trim().is_empty()) {
        where_clauses.push(format!("c.belong_month = ?{idx}"));
        params_vec.push(Box::new(month.clone()));
        idx += 1;
    }
    if let Some(employee_id) = q.employee_id {
        where_clauses.push(format!("c.employee_id = ?{idx}"));
        params_vec.push(Box::new(employee_id));
        idx += 1;
    }
    if let Some(status) = q.status.as_ref().filter(|v| !v.trim().is_empty()) {
        where_clauses.push(format!("c.status = ?{idx}"));
        params_vec.push(Box::new(status.clone()));
        idx += 1;
    }
    if let Some(payment_status) = q.payment_status.as_ref().filter(|v| !v.trim().is_empty()) {
        where_clauses.push(format!("c.payment_status = ?{idx}"));
        params_vec.push(Box::new(payment_status.clone()));
        idx += 1;
    }
    if let Some(keyword) = q.keyword.as_ref().filter(|v| !v.trim().is_empty()) {
        where_clauses.push(format!(
            "(c.claim_no LIKE ?{idx} OR c.title LIKE ?{idx} OR c.remark LIKE ?{idx} OR e.name LIKE ?{idx})"
        ));
        params_vec.push(Box::new(format!("%{keyword}%")));
    }

    let sql = format!(
        "SELECT c.id, c.claim_no, c.employee_id, e.name, e.department, c.belong_month,
                c.title, c.total_amount, c.invoice_count, c.status, c.payment_status,
                c.payment_date, c.remark, c.created_at, c.updated_at
         FROM reimbursement_claims c
         LEFT JOIN employees e ON e.id = c.employee_id
         WHERE {}
         ORDER BY c.updated_at DESC, c.id DESC",
        where_clauses.join(" AND ")
    );

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), row_to_reimbursement_claim)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn get_reimbursement_claim(conn: &Connection, id: i64) -> AppResult<ReimbursementClaim> {
    conn.query_row(
        "SELECT c.id, c.claim_no, c.employee_id, e.name, e.department, c.belong_month,
                c.title, c.total_amount, c.invoice_count, c.status, c.payment_status,
                c.payment_date, c.remark, c.created_at, c.updated_at
         FROM reimbursement_claims c
         LEFT JOIN employees e ON e.id = c.employee_id
         WHERE c.id = ?1",
        params![id],
        row_to_reimbursement_claim,
    )
    .map_err(|e| AppError::NotFound(format!("报销单ID={id}未找到: {e}")))
}

pub fn get_reimbursement_claim_month(conn: &Connection, id: i64) -> AppResult<String> {
    Ok(get_reimbursement_claim(conn, id)?.belong_month)
}

pub fn get_reimbursement_invoices(
    conn: &Connection,
    claim_id: i64,
) -> AppResult<Vec<ReimbursementInvoice>> {
    let mut stmt = conn.prepare(
        "SELECT ci.claim_id, i.id, i.invoice_number, i.seller_name, i.expense_type_code,
                i.total_amount, i.issue_date
         FROM reimbursement_claim_invoices ci
         JOIN invoices i ON i.id = ci.invoice_id
         WHERE ci.claim_id = ?1
         ORDER BY i.issue_date DESC, i.id DESC",
    )?;
    let rows = stmt.query_map(params![claim_id], |row| {
        Ok(ReimbursementInvoice {
            claim_id: row.get(0)?,
            invoice_id: row.get(1)?,
            invoice_number: row.get(2)?,
            seller_name: row.get(3)?,
            expense_type_code: row.get(4)?,
            total_amount: row.get(5)?,
            issue_date: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn save_reimbursement_claim(
    conn: &Connection,
    data: &ReimbursementClaimInput,
) -> AppResult<ReimbursementClaim> {
    ensure_month_open(conn, &data.belong_month)?;
    if let Some(id) = data.id {
        let old_month = get_reimbursement_claim_month(conn, id)?;
        ensure_month_open(conn, &old_month)?;
        if active_payment_item_exists(conn, "reimbursement_claim", id)? {
            return Err(AppError::InvalidParam(
                "报销单已纳入付款批次，不能直接编辑".into(),
            ));
        }
    }
    let employee_id = data
        .employee_id
        .ok_or_else(|| AppError::InvalidParam("请选择报销人".into()))?;
    let title = data.title.trim();
    if title.is_empty() {
        return Err(AppError::InvalidParam("报销单标题必填".into()));
    }
    if data.belong_month.trim().is_empty() {
        return Err(AppError::InvalidParam("归属月份必填".into()));
    }
    if data.invoice_ids.is_empty() {
        return Err(AppError::InvalidParam("至少选择一张发票".into()));
    }

    let mut total_amount = 0.0;
    for invoice_id in &data.invoice_ids {
        let invoice = get_invoice(conn, *invoice_id)?;
        if invoice.status.as_deref() == Some("void") {
            return Err(AppError::InvalidParam(format!(
                "发票ID={invoice_id}已作废，不能报销"
            )));
        }
        if invoice.employee_id != Some(employee_id) {
            return Err(AppError::InvalidParam(format!(
                "发票ID={invoice_id}的报销人与报销单不一致"
            )));
        }
        ensure_invoice_not_claimed(conn, *invoice_id, data.id)?;
        total_amount += invoice.total_amount;
    }

    let now = Utc::now().to_rfc3339();
    // 报销单写入 + 关联发票重写 + 凭证补偿放在同一事务：
    // 先重写关联行，再补偿凭证（作废旧计提/发票单独凭证后按新关联重建，幂等）
    let tx = conn.unchecked_transaction()?;
    let mut old_invoice_ids: Vec<i64> = Vec::new();
    let claim_id = if let Some(id) = data.id {
        let updated = tx.execute(
            "UPDATE reimbursement_claims
             SET employee_id=?1, belong_month=?2, title=?3, total_amount=?4, invoice_count=?5,
                 status=?6, payment_status=?7, payment_date=?8, remark=?9, updated_at=?10
             WHERE id=?11 AND status != 'void'",
            params![
                employee_id,
                data.belong_month.trim(),
                title,
                total_amount,
                data.invoice_ids.len() as i32,
                data.status.as_deref().unwrap_or("draft"),
                data.payment_status.as_deref().unwrap_or("unpaid"),
                data.payment_date.as_ref(),
                data.remark.as_ref(),
                now,
                id
            ],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(format!("报销单ID={id}未找到或已作废")));
        }
        // 移除旧关联前记录发票清单，用于补偿其单独入账凭证
        old_invoice_ids = {
            let mut stmt = tx.prepare(
                "SELECT invoice_id FROM reimbursement_claim_invoices WHERE claim_id = ?1",
            )?;
            let rows = stmt.query_map(params![id], |r| r.get(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        tx.execute(
            "DELETE FROM reimbursement_claim_invoices WHERE claim_id = ?1",
            params![id],
        )?;
        id
    } else {
        let claim_no = generate_reimbursement_claim_no(&data.belong_month);
        tx.execute(
            "INSERT INTO reimbursement_claims
             (claim_no, employee_id, belong_month, title, total_amount, invoice_count,
              status, payment_status, payment_date, remark, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                claim_no,
                employee_id,
                data.belong_month.trim(),
                title,
                total_amount,
                data.invoice_ids.len() as i32,
                data.status.as_deref().unwrap_or("draft"),
                data.payment_status.as_deref().unwrap_or("unpaid"),
                data.payment_date.as_ref(),
                data.remark.as_ref(),
                now,
                now
            ],
        )?;
        tx.last_insert_rowid()
    };

    for invoice_id in &data.invoice_ids {
        tx.execute(
            "INSERT INTO reimbursement_claim_invoices (claim_id, invoice_id, created_at)
             VALUES (?1, ?2, ?3)",
            params![claim_id, invoice_id, now],
        )?;
    }

    // 关联行落库后补偿凭证：作废旧计提 + 对全部新旧关联发票重建单独凭证判定 + 按需重生成计提
    compensate_claim_vouchers(&tx, claim_id, &old_invoice_ids)?;
    tx.commit()?;
    get_reimbursement_claim(conn, claim_id)
}

/// 报销单关联/金额/状态变化后的凭证补偿（在关联行已落库后调用）：
/// 1) 作废该报销单计提凭证；2) 对新旧关联发票 void + maybe 重建单独凭证（内部幂等）；
/// 3) 报销单仍为 approved 时重新生成计提。
fn compensate_claim_vouchers(
    conn: &Connection,
    claim_id: i64,
    old_invoice_ids: &[i64],
) -> AppResult<()> {
    crate::accounting::void_reimbursement_accrual_voucher(conn, claim_id)?;
    // 新关联发票：作废其单独入账凭证（若进入 approved 则由计提统一承载，防止重复计费）
    let new_invoice_ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT invoice_id FROM reimbursement_claim_invoices WHERE claim_id = ?1")?;
        let rows = stmt.query_map(params![claim_id], |r| r.get(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for invoice_id in new_invoice_ids.iter().chain(old_invoice_ids.iter()) {
        crate::accounting::void_invoice_expense_voucher(conn, *invoice_id)?;
    }
    for invoice_id in new_invoice_ids.iter().chain(old_invoice_ids.iter()) {
        crate::accounting::maybe_generate_invoice_expense_voucher(conn, *invoice_id)?;
    }
    // 报销单本身若为 approved，重新生成计提
    crate::accounting::generate_reimbursement_accrual_voucher(conn, claim_id)?;
    Ok(())
}

pub fn update_reimbursement_claim_status(
    conn: &Connection,
    id: i64,
    status: Option<String>,
    payment_status: Option<String>,
    payment_date: Option<String>,
) -> AppResult<bool> {
    let existing = get_reimbursement_claim(conn, id)?;
    ensure_month_open(conn, &existing.belong_month)?;
    if payment_status.is_some() && active_payment_item_exists(conn, "reimbursement_claim", id)? {
        return Err(AppError::InvalidParam(
            "报销单已纳入付款批次，请在付款批次中处理付款状态".into(),
        ));
    }
    let old_status = existing.status.clone();
    let new_status = status.unwrap_or(existing.status);
    let new_payment_status = payment_status.unwrap_or(existing.payment_status);
    let now = Utc::now().to_rfc3339();
    // 状态变更与凭证联动同事务：进入 approved 生成计提并作废关联发票单独凭证（防重复计费）；
    // 离开 approved（反审批/驳回）作废计提并恢复仍满足条件的发票单独凭证
    let tx = conn.unchecked_transaction()?;
    let updated = tx.execute(
        "UPDATE reimbursement_claims
         SET status=?1, payment_status=?2, payment_date=?3, updated_at=?4
         WHERE id=?5 AND status != 'void'",
        params![new_status, new_payment_status, payment_date, now, id],
    )?;
    if updated > 0 && new_status != old_status {
        let invoice_ids: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT invoice_id FROM reimbursement_claim_invoices WHERE claim_id = ?1",
            )?;
            let rows = stmt.query_map(params![id], |r| r.get(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        if new_status == "approved" {
            for invoice_id in &invoice_ids {
                crate::accounting::void_invoice_expense_voucher(&tx, *invoice_id)?;
            }
            crate::accounting::generate_reimbursement_accrual_voucher(&tx, id)?;
        } else if old_status == "approved" {
            crate::accounting::void_reimbursement_accrual_voucher(&tx, id)?;
            for invoice_id in &invoice_ids {
                crate::accounting::maybe_generate_invoice_expense_voucher(&tx, *invoice_id)?;
            }
        }
    }
    tx.commit()?;
    Ok(updated > 0)
}

pub fn soft_delete_reimbursement_claim(conn: &Connection, id: i64) -> AppResult<bool> {
    let old_month = get_reimbursement_claim_month(conn, id)?;
    ensure_month_open(conn, &old_month)?;
    if active_payment_item_exists(conn, "reimbursement_claim", id)? {
        return Err(AppError::InvalidParam(
            "报销单已纳入付款批次，不能直接作废".into(),
        ));
    }
    let now = Utc::now().to_rfc3339();
    // 报销单作废与凭证联动同事务：作废计提凭证 + 关联发票恢复单独入账
    let tx = conn.unchecked_transaction()?;
    let updated = tx.execute(
        "UPDATE reimbursement_claims SET status='void', updated_at=?1 WHERE id=?2 AND status != 'void'",
        params![now, id],
    )?;
    if updated > 0 {
        let invoice_ids: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT invoice_id FROM reimbursement_claim_invoices WHERE claim_id = ?1",
            )?;
            let rows = stmt.query_map(params![id], |r| r.get(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        crate::accounting::void_reimbursement_accrual_voucher(&tx, id)?;
        for invoice_id in &invoice_ids {
            crate::accounting::maybe_generate_invoice_expense_voucher(&tx, *invoice_id)?;
        }
    }
    tx.commit()?;
    Ok(updated > 0)
}

fn ensure_invoice_not_claimed(
    conn: &Connection,
    invoice_id: i64,
    current_claim_id: Option<i64>,
) -> AppResult<()> {
    let result = conn.query_row(
        "SELECT c.claim_no
         FROM reimbursement_claim_invoices ci
         JOIN reimbursement_claims c ON c.id = ci.claim_id
         WHERE ci.invoice_id = ?1 AND c.status != 'void'
           AND (?2 IS NULL OR c.id != ?2)
         LIMIT 1",
        params![invoice_id, current_claim_id],
        |row| row.get::<_, String>(0),
    );
    match result {
        Ok(claim_no) => Err(AppError::InvalidParam(format!(
            "发票ID={invoice_id}已关联报销单{claim_no}"
        ))),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(()),
        Err(e) => Err(AppError::from(e)),
    }
}

fn generate_reimbursement_claim_no(month: &str) -> String {
    let month_part = month.replace('-', "");
    format!("BX{}{}", month_part, Utc::now().timestamp_millis())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use rusqlite::Connection;

    pub fn setup_db() -> Connection {
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
                image_encrypted INTEGER NOT NULL DEFAULT 0,
                created_at TEXT, updated_at TEXT
            );
            CREATE UNIQUE INDEX idx_invoices_code_number ON invoices(COALESCE(invoice_code, ''), invoice_number) WHERE status != 'void';
            CREATE TABLE security_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                password_hash TEXT NOT NULL,
                password_kek_salt TEXT NOT NULL,
                wrapped_dek_by_password TEXT NOT NULL,
                wrapped_dek_by_password_nonce TEXT NOT NULL,
                recovery_kek_salt TEXT NOT NULL,
                wrapped_dek_by_recovery TEXT NOT NULL,
                wrapped_dek_by_recovery_nonce TEXT NOT NULL,
                security_question TEXT NOT NULL,
                question_kek_salt TEXT NOT NULL,
                wrapped_dek_by_question TEXT NOT NULL,
                wrapped_dek_by_question_nonce TEXT NOT NULL,
                security_answer_hash TEXT NOT NULL,
                idle_timeout_seconds INTEGER NOT NULL DEFAULT 300,
                idle_lock_enabled INTEGER NOT NULL DEFAULT 1,
                sensitive_reveal_seconds INTEGER NOT NULL DEFAULT 300,
                failed_attempts INTEGER NOT NULL DEFAULT 0,
                lock_until TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE legacy_migration_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                status TEXT NOT NULL DEFAULT 'pending',
                total_invoices INTEGER NOT NULL DEFAULT 0,
                processed_invoices INTEGER NOT NULL DEFAULT 0,
                token_migrated INTEGER NOT NULL DEFAULT 0,
                started_at TEXT,
                completed_at TEXT
            );
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
            amount: Some(100.0),
            tax_amount: Some(6.0),
            total_amount: Some(106.0),
            seller_name: Some("测试销售方".into()),
            seller_tax_id: Some("91XXXX".into()),
            buyer_name: Some("测试购买方".into()),
            buyer_tax_id: Some("92XXXX".into()),
            expense_type_code: Some("office".into()),
            employee_id: Some(1),
            belong_month: Some("2026-08".into()),
            remark: None,
            image_path: Some("/tmp/x.pdf".into()),
            raw_ocr_json: Some("{}".into()),
        }
    }

    pub(crate) fn setup_financial_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        seed_gl_accounts(&conn).unwrap();
        insert_default_data(&conn).unwrap();
        conn.execute_batch(
            "
            INSERT INTO employees
                (id, employee_no, name, department, status, base_salary, created_at, updated_at)
            VALUES
                (1, 'E001', '张三', '销售部', 'active', 10000, '2026-08-01', '2026-08-01'),
                (2, 'E002', '李四', '技术部', 'active', 8000, '2026-08-01', '2026-08-01');

            INSERT INTO salary_monthly_results
                (salary_month, employee_no, name, department, gross_salary, net_salary,
                 social_security_personal, housing_fund_personal, attendance_deduction,
                 tax_amount, other_deduction, status, locked, created_at, updated_at)
            VALUES
                ('2026-07', 'E001', '张三', '销售部', 9000, 7300, 900, 1000, 0, 600, 0, 'reviewed', 0, '2026-07-31', '2026-07-31'),
                ('2026-08', 'E001', '张三', '销售部', 10000, 7800, 1000, 1200, 200, 800, 0, 'reviewed', 0, '2026-08-31', '2026-08-31'),
                ('2026-08', 'E002', '李四', '技术部', 8000, 6600, 800, 900, 0, 500, 0, 'reviewed', 0, '2026-08-31', '2026-08-31');

            INSERT INTO attendance_records
                (salary_month, employee_no, name, expected_days, actual_days, late_count, early_leave_count, absent_days, created_at, updated_at)
            VALUES
                ('2026-08', 'E001', '张三', 22, 21, 1, 0, 0, '2026-08-31', '2026-08-31');

            INSERT INTO invoices
                (id, invoice_code, invoice_number, total_amount, expense_type_code, employee_id, belong_month, status, created_at, updated_at)
            VALUES
                (1, 'A', '001', 300, 'office', 1, '2026-08', 'normal', '2026-08-10', '2026-08-10'),
                (2, 'A', '002', 500, 'travel', 2, '2026-08', 'normal', '2026-08-10', '2026-08-10'),
                (3, 'A', '003', 100, 'office', 1, '2026-07', 'normal', '2026-07-10', '2026-07-10');

            INSERT INTO reimbursement_claims
                (id, claim_no, employee_id, belong_month, title, total_amount, invoice_count, status, payment_status, created_at, updated_at)
            VALUES
                (1, 'BX202608001', 1, '2026-08', '销售报销', 300, 1, 'approved', 'paid', '2026-08-15', '2026-08-15'),
                (2, 'BX202608002', 2, '2026-08', '技术报销', 500, 1, 'approved', 'unpaid', '2026-08-15', '2026-08-15');

            INSERT INTO reimbursement_claim_invoices (claim_id, invoice_id, created_at)
            VALUES (1, 1, '2026-08-15'), (2, 2, '2026-08-15');
            ",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_insert_and_find_invoice() {
        let conn = setup_db();
        let inv =
            insert_invoice(&conn, &sample_input("12345", "67890"), "/stored/x.pdf", 0).unwrap();
        assert_eq!(inv.invoice_code.as_deref(), Some("12345"));
        let found = find_invoice_by_dedup_key(&conn, Some("12345"), "67890").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, inv.id);
    }

    #[test]
    fn test_find_nonexistent_returns_none() {
        let conn = setup_db();
        let found = find_invoice_by_dedup_key(&conn, Some("X"), "Y").unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn test_find_full_electronic_invoice_no_code() {
        // 全电票无发票代码，应能按 number 找到
        let conn = setup_db();
        let mut input = sample_input("", "99999");
        input.invoice_code = None;
        let inv = insert_invoice(&conn, &input, "/e.pdf", 0).unwrap();
        assert!(inv.invoice_code.is_none());
        let found = find_invoice_by_dedup_key(&conn, None, "99999").unwrap();
        assert!(found.is_some(), "None code should match via COALESCE");
        assert_eq!(found.unwrap().id, inv.id);
    }

    #[test]
    fn test_unique_index_blocks_duplicate_no_code() {
        // 两条全电票同号应被拦截（COALESCE 把 NULL 转 '' 后冲突）
        let conn = setup_db();
        let mut a = sample_input("", "88888");
        a.invoice_code = None;
        let mut b = sample_input("", "88888");
        b.invoice_code = None;
        insert_invoice(&conn, &a, "/a.pdf", 0).unwrap();
        let result = insert_invoice(&conn, &b, "/b.pdf", 0);
        assert!(
            result.is_err(),
            "duplicate full-electronic invoice should be blocked by COALESCE index"
        );
    }

    #[test]
    fn test_unique_index_blocks_duplicate() {
        let conn = setup_db();
        insert_invoice(&conn, &sample_input("111", "222"), "/a.pdf", 0).unwrap();
        let result = insert_invoice(&conn, &sample_input("111", "222"), "/b.pdf", 0);
        assert!(result.is_err(), "重复插入应被唯一索引拦截");
    }

    #[test]
    fn test_soft_delete_allows_resubmission() {
        let conn = setup_db();
        let inv = insert_invoice(&conn, &sample_input("111", "222"), "/a.pdf", 0).unwrap();
        assert!(soft_delete_invoice(&conn, inv.id).unwrap());
        // Re-inserting same code/number should now succeed
        let result = insert_invoice(&conn, &sample_input("111", "222"), "/b.pdf", 0);
        assert!(
            result.is_ok(),
            "soft-deleted invoice should allow re-submission"
        );
    }

    #[test]
    fn test_soft_delete_hides_record() {
        let conn = setup_db();
        let inv = insert_invoice(&conn, &sample_input("333", "444"), "/c.pdf", 0).unwrap();
        assert!(soft_delete_invoice(&conn, inv.id).unwrap());
        // find 应该返回 None（因为 status='void' 被过滤）
        assert!(find_invoice_by_dedup_key(&conn, Some("333"), "444")
            .unwrap()
            .is_none());
        // query_invoices 默认也应过滤
        let list = query_invoices(&conn, &InvoiceQuery::default()).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_query_invoices_filters() {
        let conn = setup_db();
        let mut a = sample_input("555", "001");
        a.belong_month = Some("2026-07".into());
        let mut b = sample_input("555", "002");
        b.belong_month = Some("2026-08".into());
        insert_invoice(&conn, &a, "/a.pdf", 0).unwrap();
        insert_invoice(&conn, &b, "/b.pdf", 0).unwrap();

        let july = query_invoices(
            &conn,
            &InvoiceQuery {
                belong_month: Some("2026-07".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(july.len(), 1);
        assert_eq!(july[0].invoice_number.as_deref(), Some("001"));
    }

    #[test]
    fn test_delete_other_expense_type_blocked() {
        let conn = setup_db();
        let other_id: i64 = conn
            .query_row(
                "SELECT id FROM invoice_expense_types WHERE code='other'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let result = delete_invoice_expense_type(&conn, other_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_used_expense_type_blocked() {
        let conn = setup_db();
        insert_invoice(&conn, &sample_input("777", "888"), "/d.pdf", 0).unwrap();
        let office_id: i64 = conn
            .query_row(
                "SELECT id FROM invoice_expense_types WHERE code='office'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let result = delete_invoice_expense_type(&conn, office_id);
        assert!(result.is_err(), "被引用的费用类型不允许删除");
    }

    #[test]
    fn test_financial_analysis_aggregates_department_employee_and_trends() {
        let conn = setup_financial_db();
        let report = get_financial_analysis(
            &conn,
            &FinancialAnalysisQuery {
                month: "2026-08".into(),
                months: Some(3),
            },
        )
        .unwrap();

        let sales = report
            .department_costs
            .iter()
            .find(|row| row.department == "销售部")
            .unwrap();
        assert_eq!(sales.employee_count, 1);
        assert_eq!(sales.gross_salary, 10000.0);
        assert_eq!(sales.social_security, 1000.0);
        assert_eq!(sales.housing_fund, 1200.0);
        assert_eq!(sales.invoice_amount, 300.0);
        assert_eq!(sales.reimbursement_amount, 300.0);

        let employee = report
            .employee_costs
            .iter()
            .find(|row| row.employee_no == "E001")
            .unwrap();
        assert_eq!(employee.attendance_deduction, 200.0);
        assert_eq!(employee.abnormal_attendance_count, 1);
        assert_eq!(employee.invoice_amount, 300.0);
        assert_eq!(employee.reimbursement_amount, 300.0);

        let office_august = report
            .expense_trends
            .iter()
            .find(|row| row.month == "2026-08" && row.expense_type_code == "office")
            .unwrap();
        assert_eq!(office_august.invoice_count, 1);
        assert_eq!(office_august.invoice_amount, 300.0);
        assert_eq!(office_august.reimbursement_amount, 300.0);

        assert_eq!(report.monthly_comparison.len(), 2);
        assert_eq!(report.monthly_comparison[0].month, "2026-07");
        assert_eq!(report.monthly_comparison[1].month, "2026-08");
        assert_eq!(report.monthly_comparison[1].gross_salary, 18000.0);
    }

    #[test]
    fn test_financial_analysis_excludes_unapproved_reimbursements() {
        let conn = setup_financial_db();
        conn.execute_batch(
            "
            INSERT INTO invoices
                (id, invoice_code, invoice_number, total_amount, expense_type_code, employee_id, belong_month, status, created_at, updated_at)
            VALUES
                (4, 'A', '004', 700, 'office', 1, '2026-08', 'normal', '2026-08-10', '2026-08-10');

            INSERT INTO reimbursement_claims
                (id, claim_no, employee_id, belong_month, title, total_amount, invoice_count, status, payment_status, created_at, updated_at)
            VALUES
                (3, 'BX202608003', 1, '2026-08', '驳回报销', 700, 1, 'rejected', 'unpaid', '2026-08-15', '2026-08-15');

            INSERT INTO reimbursement_claim_invoices (claim_id, invoice_id, created_at)
            VALUES (3, 4, '2026-08-15');
            ",
        )
        .unwrap();

        let report = get_financial_analysis(
            &conn,
            &FinancialAnalysisQuery {
                month: "2026-08".into(),
                months: Some(3),
            },
        )
        .unwrap();

        let sales = report
            .department_costs
            .iter()
            .find(|row| row.department == "销售部")
            .unwrap();
        assert_eq!(sales.invoice_amount, 1000.0);
        assert_eq!(sales.reimbursement_amount, 300.0);

        let employee = report
            .employee_costs
            .iter()
            .find(|row| row.employee_no == "E001")
            .unwrap();
        assert_eq!(employee.invoice_amount, 1000.0);
        assert_eq!(employee.reimbursement_amount, 300.0);

        let office_august = report
            .expense_trends
            .iter()
            .find(|row| row.month == "2026-08" && row.expense_type_code == "office")
            .unwrap();
        assert_eq!(office_august.invoice_count, 2);
        assert_eq!(office_august.invoice_amount, 1000.0);
        assert_eq!(office_august.reimbursement_amount, 300.0);

        let august = report
            .monthly_comparison
            .iter()
            .find(|row| row.month == "2026-08")
            .unwrap();
        assert_eq!(august.reimbursement_amount, 800.0);
    }

    #[test]
    fn test_budget_execution_and_month_close_warnings() {
        let conn = setup_financial_db();
        let budget = save_budget(
            &conn,
            &BudgetInput {
                id: None,
                month: "2026-08".into(),
                department: Some("销售部".into()),
                expense_type_code: None,
                budget_amount: 10_000.0,
                remark: Some("销售部预算".into()),
            },
        )
        .unwrap();
        assert_eq!(budget.department.as_deref(), Some("销售部"));

        let updated = save_budget(
            &conn,
            &BudgetInput {
                id: None,
                month: "2026-08".into(),
                department: Some("销售部".into()),
                expense_type_code: None,
                budget_amount: 11_000.0,
                remark: Some("更新预算".into()),
            },
        )
        .unwrap();
        assert_eq!(updated.id, budget.id);
        assert_eq!(updated.budget_amount, 11_000.0);

        let report = get_financial_analysis(
            &conn,
            &FinancialAnalysisQuery {
                month: "2026-08".into(),
                months: Some(3),
            },
        )
        .unwrap();
        assert_eq!(report.budget_executions.len(), 1);
        assert_eq!(report.budget_executions[0].actual_amount, 12_800.0);
        assert_eq!(report.budget_executions[0].over_amount, 1_800.0);
        assert_eq!(report.budget_executions[0].status, "over");

        conn.execute(
            "INSERT INTO reimbursement_claims
                (id, claim_no, employee_id, belong_month, title, total_amount, invoice_count, status, payment_status, created_at, updated_at)
             VALUES (3, 'BX202608003', 1, '2026-08', '重复金额测试', 300, 0, 'approved', 'paid', '2026-08-20', '2026-08-20')",
            [],
        )
        .unwrap();
        let workbench = get_month_close_workbench(&conn, "2026-08").unwrap();
        assert_eq!(workbench.summary.over_budget_count, 1);
        assert_eq!(workbench.summary.duplicate_amount_count, 1);
        assert!(workbench
            .checks
            .iter()
            .any(|item| item.key == "budget_overrun" && item.status == "warning"));
        assert!(workbench
            .checks
            .iter()
            .any(|item| item.key == "duplicate_amounts" && item.status == "warning"));
    }

    #[test]
    fn test_closed_month_blocks_budget_writes() {
        let conn = setup_financial_db();
        let budget = save_budget(
            &conn,
            &BudgetInput {
                id: None,
                month: "2026-08".into(),
                department: None,
                expense_type_code: None,
                budget_amount: 30_000.0,
                remark: None,
            },
        )
        .unwrap();
        make_august_closable(&conn);
        close_month(&conn, "2026-08", "system", None).unwrap();

        let save_err = save_budget(
            &conn,
            &BudgetInput {
                id: Some(budget.id),
                month: "2026-08".into(),
                department: None,
                expense_type_code: None,
                budget_amount: 31_000.0,
                remark: None,
            },
        )
        .unwrap_err();
        assert!(matches!(save_err, AppError::InvalidParam(_)));

        let delete_err = delete_budget(&conn, budget.id).unwrap_err();
        assert!(matches!(delete_err, AppError::InvalidParam(_)));
    }

    #[test]
    fn test_month_close_excludes_void_paid_reimbursements() {
        let conn = setup_financial_db();
        assert!(soft_delete_reimbursement_claim(&conn, 1).unwrap());

        let workbench = get_month_close_workbench(&conn, "2026-08").unwrap();

        assert_eq!(workbench.summary.reimbursement_count, 1);
        assert_eq!(workbench.summary.approved_reimbursement_amount, 500.0);
        assert_eq!(workbench.summary.paid_reimbursement_amount, 0.0);
    }

    #[test]
    fn test_month_close_blocks_unlocked_accrual_vouchers() {
        let conn = setup_financial_db();
        // 锁定前无计提凭证，检查项通过
        let workbench = get_month_close_workbench(&conn, "2026-08").unwrap();
        let item = workbench
            .checks
            .iter()
            .find(|c| c.key == "salary_unlocked_accrual")
            .unwrap();
        assert_eq!(item.status, "ok");
        // 锁定 → 生成计提凭证，检查项仍通过
        lock_salary_results(&conn, "2026-08").unwrap();
        let workbench = get_month_close_workbench(&conn, "2026-08").unwrap();
        let item = workbench
            .checks
            .iter()
            .find(|c| c.key == "salary_unlocked_accrual")
            .unwrap();
        assert_eq!(item.status, "ok");
        // 受控解锁 → 计提凭证作废，检查项阻塞
        unlock_salary_results(&conn, "2026-08").unwrap();
        let workbench = get_month_close_workbench(&conn, "2026-08").unwrap();
        let item = workbench
            .checks
            .iter()
            .find(|c| c.key == "salary_unlocked_accrual")
            .unwrap();
        assert_eq!(item.status, "blocking");
        assert!(item.count > 0);
        assert!(
            item.description.contains("已作废且未重锁") && item.description.contains("重新锁定"),
            "got: {}",
            item.description
        );
        // 阻塞项应让正式月结失败
        let err = close_month(&conn, "2026-08", "system", None).unwrap_err();
        assert!(err.to_string().contains("工资计提凭证完整"), "got: {err:?}");
        // 重新锁定 → active 凭证恢复，检查项回到 ok，月结恢复放行
        lock_salary_results(&conn, "2026-08").unwrap();
        let workbench = get_month_close_workbench(&conn, "2026-08").unwrap();
        let item = workbench
            .checks
            .iter()
            .find(|c| c.key == "salary_unlocked_accrual")
            .unwrap();
        assert_eq!(item.status, "ok");
        // 其他月份的作废计提凭证不影响本月检查：2026-07 存在未重锁的作废计提凭证，2026-08 检查仍通过
        let july_salary_id: i64 = conn
            .query_row(
                "SELECT id FROM salary_monthly_results WHERE salary_month='2026-07' LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status, created_at, updated_at)
             VALUES ('记-202607-002', '2026-07-28', '2026-07', 'salary_accrual', ?1, 7400.0, 'void', '2026-07-31', '2026-07-31')",
            params![july_salary_id],
        )
        .unwrap();
        let workbench = get_month_close_workbench(&conn, "2026-08").unwrap();
        let item = workbench
            .checks
            .iter()
            .find(|c| c.key == "salary_unlocked_accrual")
            .unwrap();
        assert_eq!(item.status, "ok");
        // 而 2026-07 检查应阻塞（确认该凭证确实被检出，只是不影响他月）
        let workbench_july = get_month_close_workbench(&conn, "2026-07").unwrap();
        let item_july = workbench_july
            .checks
            .iter()
            .find(|c| c.key == "salary_unlocked_accrual")
            .unwrap();
        assert_eq!(item_july.status, "blocking");
    }

    #[test]
    fn test_december_workbench_has_period_close_check() {
        let conn = setup_financial_db();
        let wb = get_month_close_workbench(&conn, "2026-12").unwrap();
        assert!(wb.checks.iter().any(|c| c.key == "period_close"));
        let wb_nov = get_month_close_workbench(&conn, "2026-11").unwrap();
        assert!(!wb_nov.checks.iter().any(|c| c.key == "period_close"));
    }

    fn make_august_closable(conn: &Connection) {
        conn.execute(
            "INSERT INTO attendance_records
                (salary_month, employee_no, name, expected_days, actual_days, late_count, early_leave_count, absent_days, created_at, updated_at)
             VALUES
                ('2026-08', 'E002', '李四', 22, 22, 0, 0, 0, '2026-08-31', '2026-08-31')",
            [],
        )
        .unwrap();
        lock_salary_results(conn, "2026-08").unwrap();
        conn.execute(
            "UPDATE reimbursement_claims SET payment_status='paid', payment_date='2026-08-31' WHERE id=2",
            [],
        )
        .unwrap();
    }

    pub(crate) fn fill_employee_bank_info(conn: &Connection) {
        conn.execute_batch(
            "
            UPDATE employees SET bank_account='62220001', bank_name='测试银行' WHERE employee_no='E001';
            UPDATE employees SET bank_account='62220002', bank_name='测试银行' WHERE employee_no='E002';
            ",
        )
        .unwrap();
    }

    /// 批次测试用付款资金账户（银行/1002/启用），返回账户 id
    pub(crate) fn make_batch_fund_account(conn: &Connection, code: &str) -> i64 {
        conn.execute(
            "INSERT INTO fund_accounts
                (account_code, name, account_type, bank_name, account_no, currency,
                 gl_account_code, opening_date, opening_balance, is_default, is_active,
                 created_at, updated_at)
             VALUES (?1, '测试付款户', 'bank', '测试银行', '6222000099', 'CNY',
                     '1002', '2026-08-01', 100000, 0, 1, '2026-08-01', '2026-08-01')",
            params![code],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// 批次测试用操作人会话（general 批次状态机署名必须）
    pub(crate) fn batch_operator() -> crate::cashier::CurrentOperatorState {
        crate::cashier::CurrentOperatorState::default()
    }

    /// general 批次测试环境：带银行信息的往来单位 + 已选操作人会话，返回 (partner_id, operator)
    fn make_general_batch_env(conn: &Connection) -> (i64, crate::cashier::CurrentOperatorState) {
        conn.execute(
            "INSERT INTO business_partners
                (partner_code, name, partner_type, bank_name, bank_account, status,
                 created_at, updated_at)
             VALUES ('GYS-T9', '测试供应商', 'supplier', '测试银行', '6222000111',
                     'active', '2026-08-01', '2026-08-01')",
            [],
        )
        .unwrap();
        let partner_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO operator_profiles (name, role, is_active, created_at, updated_at)
             VALUES ('测试出纳', 'cashier', 1, '2026-08-01', '2026-08-01')",
            [],
        )
        .unwrap();
        let operator_id = conn.last_insert_rowid();
        let operator = crate::cashier::CurrentOperatorState::default();
        crate::cashier::set_current_operator(conn, &operator, operator_id).unwrap();
        (partner_id, operator)
    }

    /// 创建一张已审批的付款单/借款单（source_account_id 指定来源账户）
    fn make_approved_fund_doc(
        conn: &Connection,
        operator: &crate::cashier::CurrentOperatorState,
        document_type: &str,
        account_id: i64,
        partner_id: Option<i64>,
        employee_id: Option<i64>,
        month: &str,
        amount: f64,
    ) -> crate::models::FundDocument {
        let input = crate::models::FundDocumentInput {
            id: None,
            document_type: document_type.into(),
            belong_month: month.into(),
            document_date: format!("{month}-05"),
            amount,
            summary: "测试付款".into(),
            department: None,
            expense_type: None,
            remark: None,
            partner_id,
            employee_id,
            source_account_id: Some(account_id),
            target_account_id: None,
            counter_account_code: Some("2202".into()),
            settlement_mode: None,
            // 借款单必须填写预计归还日（Task 14 spec 4.11）
            due_date: if document_type == "advance" {
                Some(format!("{month}-28"))
            } else {
                None
            },
            advance_allocations: None,
        };
        let doc = crate::cashier::create_fund_document(conn, operator, &input).unwrap();
        crate::cashier::submit_fund_document(conn, operator, doc.id, None).unwrap();
        crate::cashier::approve_fund_document(conn, operator, doc.id, "同意").unwrap()
    }

    fn insert_paid_batch_bank_match(conn: &Connection, batch_id: i64, amount: f64) {
        conn.execute(
            "INSERT INTO bank_transactions
                (transaction_date, belong_month, summary, counterparty_name, counterparty_account,
                 income_amount, expense_amount, balance, status, created_at, updated_at)
             VALUES ('2026-08-31', '2026-08', '测试付款批次匹配', '测试收款人', '62220000',
                     0, ?1, 10000, 'matched', '2026-08-31', '2026-08-31')",
            params![amount],
        )
        .unwrap();
        let transaction_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO bank_transaction_matches
                (transaction_id, payment_batch_id, match_score, remark, status, created_at)
             VALUES (?1, ?2, 100, '测试匹配', 'active', '2026-08-31')",
            params![transaction_id, batch_id],
        )
        .unwrap();
    }

    #[test]
    fn test_close_month_rejects_blocking_checks() {
        let conn = setup_financial_db();
        let err = close_month(&conn, "2026-08", "system", None).unwrap_err();
        assert!(
            matches!(err, AppError::InvalidParam(_)),
            "expected InvalidParam for blocking month close, got {:?}",
            err
        );
    }

    #[test]
    fn test_close_month_success_writes_snapshot() {
        let conn = setup_financial_db();
        make_august_closable(&conn);

        let record = close_month(&conn, "2026-08", "system", Some("月结测试")).unwrap();
        assert_eq!(record.status, "closed");
        assert!(record
            .summary_json
            .as_deref()
            .unwrap_or("")
            .contains("2026-08"));
        assert!(record
            .checks_json
            .as_deref()
            .unwrap_or("")
            .contains("salary_calculated"));
        assert!(record.closed_at.is_some());
        assert_eq!(record.remark.as_deref(), Some("月结测试"));

        assert!(is_month_closed(&conn, "2026-08").unwrap());
    }

    #[test]
    fn test_close_month_is_not_repeated() {
        let conn = setup_financial_db();
        make_august_closable(&conn);
        close_month(&conn, "2026-08", "system", None).unwrap();

        let err = close_month(&conn, "2026-08", "system", None).unwrap_err();
        assert!(matches!(err, AppError::InvalidParam(_)));
    }

    #[test]
    fn test_reopen_month_requires_closed_and_reason() {
        let conn = setup_financial_db();

        let open_err = reopen_month(&conn, "2026-08", "调整").unwrap_err();
        assert!(matches!(open_err, AppError::NotFound(_)));

        make_august_closable(&conn);
        close_month(&conn, "2026-08", "system", None).unwrap();

        let empty_reason = reopen_month(&conn, "2026-08", " ").unwrap_err();
        assert!(matches!(empty_reason, AppError::InvalidParam(_)));

        let reopened = reopen_month(&conn, "2026-08", "补录调整").unwrap();
        assert_eq!(reopened.status, "reopened");
        assert_eq!(reopened.reopen_reason.as_deref(), Some("补录调整"));
        assert!(!is_month_closed(&conn, "2026-08").unwrap());
    }

    #[test]
    fn test_closed_month_blocks_core_writes_and_reopened_allows() {
        let conn = setup_financial_db();
        make_august_closable(&conn);
        close_month(&conn, "2026-08", "system", None).unwrap();

        let attendance_err = create_attendance_record(
            &conn,
            &AttendanceRecordInput {
                id: None,
                salary_month: "2026-08".into(),
                employee_no: "E001".into(),
                name: Some("张三".into()),
                expected_days: Some(22.0),
                actual_days: Some(22.0),
                late_count: Some(0),
                early_leave_count: Some(0),
                personal_leave_days: Some(0.0),
                sick_leave_days: Some(0.0),
                absent_days: Some(0.0),
                overtime_hours: Some(0.0),
                source_type: None,
                ocr_batch_id: None,
                remark: None,
            },
        )
        .unwrap_err();
        assert!(matches!(attendance_err, AppError::InvalidParam(_)));

        let salary_id: i64 = conn
            .query_row(
                "SELECT id FROM salary_monthly_results WHERE salary_month='2026-08' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let salary_err = update_salary_result(
            &conn,
            salary_id,
            &SalaryResultUpdate {
                overtime_salary: Some(10.0),
                meal_allowance: None,
                transport_allowance: None,
                other_allowance: None,
                other_deduction: None,
                remark: None,
            },
        )
        .unwrap_err();
        assert!(matches!(salary_err, AppError::InvalidParam(_)));

        let invoice_err =
            insert_invoice(&conn, &sample_input("LOCK", "001"), "/locked.pdf", 0).unwrap_err();
        assert!(matches!(invoice_err, AppError::InvalidParam(_)));

        let invoice_delete_err = soft_delete_invoice(&conn, 1).unwrap_err();
        assert!(matches!(invoice_delete_err, AppError::InvalidParam(_)));

        let reimbursement_err = update_reimbursement_claim_status(
            &conn,
            1,
            Some("approved".into()),
            Some("paid".into()),
            Some("2026-08-31".into()),
        )
        .unwrap_err();
        assert!(matches!(reimbursement_err, AppError::InvalidParam(_)));

        let reimbursement_delete_err = soft_delete_reimbursement_claim(&conn, 1).unwrap_err();
        assert!(matches!(
            reimbursement_delete_err,
            AppError::InvalidParam(_)
        ));

        let ocr_batch_err = save_ocr_batch(
            &conn,
            &OcrBatch {
                id: 0,
                batch_name: Some("锁账 OCR".into()),
                salary_month: Some("2026-08".into()),
                image_path: None,
                raw_text: None,
                parsed_json: None,
                status: "pending".into(),
                created_at: None,
            },
        )
        .unwrap_err();
        assert!(matches!(ocr_batch_err, AppError::InvalidParam(_)));

        reopen_month(&conn, "2026-08", "测试反月结").unwrap();
        // 月结前 make_august_closable 已锁定工资结果；锁定结果仍需先解锁才能修改
        unlock_salary_results(&conn, "2026-08").unwrap();
        assert!(update_salary_result(
            &conn,
            salary_id,
            &SalaryResultUpdate {
                overtime_salary: Some(10.0),
                meal_allowance: None,
                transport_allowance: None,
                other_allowance: None,
                other_deduction: None,
                remark: None,
            },
        )
        .unwrap());

        assert!(save_ocr_batch(
            &conn,
            &OcrBatch {
                id: 0,
                batch_name: Some("反月结 OCR".into()),
                salary_month: Some("2026-08".into()),
                image_path: None,
                raw_text: None,
                parsed_json: None,
                status: "pending".into(),
                created_at: None,
            },
        )
        .is_ok());
    }

    #[test]
    fn test_locked_salary_result_rejects_save_and_update() {
        let conn = setup_financial_db();
        lock_salary_results(&conn, "2026-08").unwrap();

        // save_salary_result：锁定后覆盖保存应被拒绝
        let existing = get_salary_result_by_employee(&conn, "2026-08", "E001").unwrap();
        let mut overwritten = existing.clone();
        overwritten.id = 0;
        overwritten.overtime_salary = 999.0;
        let save_err = save_salary_result(&conn, &overwritten).unwrap_err();
        assert!(matches!(save_err, AppError::InvalidParam(_)));

        // update_salary_result：锁定后调整应被拒绝
        let update_err = update_salary_result(
            &conn,
            existing.id,
            &SalaryResultUpdate {
                overtime_salary: Some(10.0),
                meal_allowance: None,
                transport_allowance: None,
                other_allowance: None,
                other_deduction: None,
                remark: None,
            },
        )
        .unwrap_err();
        assert!(matches!(update_err, AppError::InvalidParam(_)));

        // 新员工插入不受锁定影响（锁定前该月无记录）
        let fresh = SalaryResult {
            id: 0,
            salary_month: "2026-08".into(),
            employee_no: "E003".into(),
            name: Some("王五".into()),
            department: Some("销售部".into()),
            base_salary: 5000.0,
            position_salary: 0.0,
            performance_salary: 0.0,
            overtime_salary: 0.0,
            meal_allowance: 0.0,
            transport_allowance: 0.0,
            other_allowance: 0.0,
            gross_salary: 5000.0,
            social_security_personal: 0.0,
            housing_fund_personal: 0.0,
            attendance_deduction: 0.0,
            tax_amount: 0.0,
            other_deduction: 0.0,
            net_salary: 5000.0,
            social_security_employer: 0.0,
            housing_fund_employer: 0.0,
            status: "reviewed".into(),
            locked: 0,
            remark: None,
            created_at: None,
            updated_at: None,
        };
        save_salary_result(&conn, &fresh).unwrap();
        // 解锁后更新恢复可用
        unlock_salary_results(&conn, "2026-08").unwrap();
        assert!(update_salary_result(
            &conn,
            existing.id,
            &SalaryResultUpdate {
                overtime_salary: Some(10.0),
                meal_allowance: None,
                transport_allowance: None,
                other_allowance: None,
                other_deduction: None,
                remark: None,
            },
        )
        .unwrap());
    }

    #[test]
    fn test_create_salary_payment_batch_prevents_duplicates_and_void_releases_sources() {
        let mut conn = setup_financial_db();
        fill_employee_bank_info(&conn);
        lock_salary_results(&conn, "2026-08").unwrap();
        let acc = make_batch_fund_account(&conn, "BANK-T1");
        let operator = batch_operator();

        let detail = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "salary".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: Some("工资代发".into()),
            },
            &operator,
        )
        .unwrap();

        assert_eq!(detail.batch.batch_type, "salary");
        assert_eq!(detail.batch.fund_account_id, Some(acc));
        assert_eq!(detail.batch.status, "draft");
        assert_eq!(detail.batch.item_count, 2);
        assert_eq!(detail.batch.total_amount, 14400.0);
        assert!(detail.items.iter().all(|item| item
            .bank_account
            .as_deref()
            .unwrap_or("")
            .starts_with("6222")));

        let duplicate = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "salary".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: None,
            },
            &operator,
        )
        .unwrap_err();
        assert!(matches!(duplicate, AppError::InvalidParam(_)));

        let voided = void_payment_batch(
            &mut conn,
            &PaymentBatchVoidInput {
                id: detail.batch.id,
                reason: "测试作废".into(),
            },
            &operator,
        )
        .unwrap();
        assert_eq!(voided.status, "void");

        let recreated = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "salary".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: None,
            },
            &operator,
        )
        .unwrap();
        assert_eq!(recreated.batch.item_count, 2);
    }

    #[test]
    fn test_reimbursement_payment_batch_paid_syncs_claim_and_blocks_direct_payment() {
        let mut conn = setup_financial_db();
        fill_employee_bank_info(&conn);
        let acc = make_batch_fund_account(&conn, "BANK-T1");
        let operator = batch_operator();

        let detail = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "reimbursement".into(),
                fund_account_id: Some(acc),
                source_ids: Some(vec![2]),
                remark: None,
            },
            &operator,
        )
        .unwrap();
        assert_eq!(detail.batch.item_count, 1);
        assert_eq!(detail.batch.total_amount, 500.0);

        let direct_payment_err = update_reimbursement_claim_status(
            &conn,
            2,
            None,
            Some("paid".into()),
            Some("2026-08-31".into()),
        )
        .unwrap_err();
        assert!(matches!(direct_payment_err, AppError::InvalidParam(_)));

        let direct_void_err = soft_delete_reimbursement_claim(&conn, 2).unwrap_err();
        assert!(matches!(direct_void_err, AppError::InvalidParam(_)));

        let draft_paid_err = mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail.batch.id,
                payment_date: "2026-08-31".into(),
            },
            &operator,
        )
        .unwrap_err();
        assert!(matches!(draft_paid_err, AppError::InvalidParam(_)));

        let exported = mark_payment_batch_exported(&conn, detail.batch.id).unwrap();
        assert_eq!(exported.status, "exported");

        let paid = mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail.batch.id,
                payment_date: "2026-08-31".into(),
            },
            &operator,
        )
        .unwrap();
        assert_eq!(paid.status, "paid");

        let (payment_status, payment_date, payment_batch_id): (String, String, i64) = conn
            .query_row(
                "SELECT payment_status, payment_date, payment_batch_id FROM reimbursement_claims WHERE id=2",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(payment_status, "paid");
        assert_eq!(payment_date, "2026-08-31");
        assert_eq!(payment_batch_id, detail.batch.id);
    }

    #[test]
    fn test_bank_transactions_auto_match_and_month_close_gate() {
        let mut conn = setup_financial_db();
        fill_employee_bank_info(&conn);
        make_august_closable(&conn);
        let acc = make_batch_fund_account(&conn, "BANK-T1");
        let operator = batch_operator();
        let detail = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "salary".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: None,
            },
            &operator,
        )
        .unwrap();
        mark_payment_batch_exported(&conn, detail.batch.id).unwrap();
        mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail.batch.id,
                payment_date: "2026-08-31".into(),
            },
            &operator,
        )
        .unwrap();

        let workbench = get_month_close_workbench(&conn, "2026-08").unwrap();
        assert_eq!(workbench.summary.unmatched_paid_batch_count, 1);
        assert!(matches!(
            close_month(&conn, "2026-08", "system", None).unwrap_err(),
            AppError::InvalidParam(_)
        ));

        assert!(insert_bank_transaction(
            &conn,
            &BankTransaction {
                id: 0,
                transaction_date: "2026-08-31".into(),
                belong_month: "2026-08".into(),
                summary: Some(format!("{} 工资代发", detail.batch.batch_no)),
                counterparty_name: Some("测试银行".into()),
                counterparty_account: Some("62220000".into()),
                income_amount: 0.0,
                expense_amount: detail.batch.total_amount,
                balance: Some(10000.0),
                status: "unmatched".into(),
                ignore_reason: None,
                imported_file: None,
                raw_json: None,
                fund_account_id: Some(acc),
                fund_account_name: None,
                matched_batch_id: None,
                matched_batch_no: None,
                matched_batch_type: None,
                matched_amount: None,
                match_score: None,
                match_remark: None,
                allocated_amount: None,
                remaining_amount: None,
                created_at: None,
                updated_at: None,
            }
        )
        .unwrap());

        let match_result = auto_match_bank_transactions(&conn, "2026-08").unwrap();
        assert_eq!(match_result.matched, 1);
        let transactions = query_bank_transactions(
            &conn,
            &BankTransactionQuery {
                belong_month: Some("2026-08".into()),
                status: Some("matched".into()),
                keyword: None,
                fund_account_id: None,
            },
        )
        .unwrap();
        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0].matched_batch_id, Some(detail.batch.id));

        cancel_bank_transaction_match(&conn, transactions[0].id).unwrap();
        let unmatched = query_bank_transactions(
            &conn,
            &BankTransactionQuery {
                belong_month: Some("2026-08".into()),
                status: Some("unmatched".into()),
                keyword: None,
                fund_account_id: None,
            },
        )
        .unwrap();
        assert_eq!(unmatched.len(), 1);

        confirm_bank_transaction_match(
            &conn,
            &BankTransactionMatchInput {
                transaction_id: unmatched[0].id,
                payment_batch_id: detail.batch.id,
                remark: Some("手工确认".into()),
            },
            100,
        )
        .unwrap();
        let workbench = get_month_close_workbench(&conn, "2026-08").unwrap();
        assert_eq!(workbench.summary.unmatched_paid_batch_count, 0);
        close_month(&conn, "2026-08", "system", None).unwrap();
    }

    #[test]
    fn test_bank_manual_voucher_blocks_batch_match() {
        // F1：已生成 active bank_manual 凭证的流水不能再匹配付款批次（防 1002 双重贷记）
        let mut conn = setup_financial_db();
        fill_employee_bank_info(&conn);
        make_august_closable(&conn);
        let acc = make_batch_fund_account(&conn, "BANK-T1");
        let operator = batch_operator();
        let detail = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "salary".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: None,
            },
            &operator,
        )
        .unwrap();
        mark_payment_batch_exported(&conn, detail.batch.id).unwrap();
        mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail.batch.id,
                payment_date: "2026-08-31".into(),
            },
            &operator,
        )
        .unwrap();

        // 插入一条金额恰好等于批次支出的流水，并生成 bank_manual 凭证
        assert!(insert_bank_transaction(
            &conn,
            &BankTransaction {
                id: 0,
                transaction_date: "2026-08-31".into(),
                belong_month: "2026-08".into(),
                summary: Some("手工入账支出".into()),
                counterparty_name: Some("测试银行".into()),
                counterparty_account: Some("62220000".into()),
                income_amount: 0.0,
                expense_amount: detail.batch.total_amount,
                balance: Some(10000.0),
                status: "unmatched".into(),
                ignore_reason: None,
                imported_file: None,
                raw_json: None,
                fund_account_id: Some(acc),
                fund_account_name: None,
                matched_batch_id: None,
                matched_batch_no: None,
                matched_batch_type: None,
                matched_amount: None,
                match_score: None,
                match_remark: None,
                allocated_amount: None,
                remaining_amount: None,
                created_at: None,
                updated_at: None,
            }
        )
        .unwrap());
        let tx_id: i64 = conn
            .query_row("SELECT MAX(id) FROM bank_transactions", [], |r| r.get(0))
            .unwrap();
        let voucher =
            crate::accounting::create_bank_manual_voucher(&conn, tx_id, "6603", acc, None).unwrap();
        assert_eq!(voucher.source_type, "bank_manual");
        // 资金行带 fund_account_id（spec 4.7）
        let fund_line: Option<i64> = conn
            .query_row(
                "SELECT fund_account_id FROM voucher_lines WHERE voucher_id=?1 AND fund_account_id IS NOT NULL LIMIT 1",
                params![voucher.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fund_line, Some(acc));

        // 人工确认匹配被拦截
        let err = confirm_bank_transaction_match(
            &conn,
            &BankTransactionMatchInput {
                transaction_id: tx_id,
                payment_batch_id: detail.batch.id,
                remark: Some("手工确认".into()),
            },
            100,
        )
        .unwrap_err();
        assert!(err.to_string().contains("已生成入账凭证"), "got: {err:?}");

        // 自动匹配不选中该流水：匹配数不增
        let result = auto_match_bank_transactions(&conn, "2026-08").unwrap();
        assert_eq!(result.matched, 0);

        // 取消匹配入口会 void 该 bank_manual 凭证，随后自动匹配恢复可用（拦截由凭证驱动而非流水状态）
        cancel_bank_transaction_match(&conn, tx_id).unwrap();
        let result = auto_match_bank_transactions(&conn, "2026-08").unwrap();
        assert_eq!(result.matched, 1);
    }

    // ==================== Task 12：旧匹配迁移与多对多核销 ====================

    /// 直插一条旧式 bank_transaction_matches（一对一批次匹配，spec 4.9 迁移对象）
    fn insert_legacy_match(
        conn: &Connection,
        transaction_id: i64,
        batch_id: i64,
        score: i32,
    ) -> i64 {
        conn.execute(
            "INSERT INTO bank_transaction_matches
                (transaction_id, payment_batch_id, match_score, remark, status, created_at)
             VALUES (?1, ?2, ?3, '旧批次匹配', 'active', '2026-08-31')",
            params![transaction_id, batch_id, score],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// 直插一条带/不带账户的银行流水（迁移测试用），返回 id
    fn insert_migration_tx(
        conn: &Connection,
        summary: &str,
        expense: f64,
        account_id: Option<i64>,
    ) -> i64 {
        conn.execute(
            "INSERT INTO bank_transactions (transaction_date, belong_month, summary,
                counterparty_name, counterparty_account, income_amount, expense_amount, balance,
                status, fund_account_id, created_at, updated_at)
             VALUES ('2026-08-31', '2026-08', ?1, '测试银行', '62220000', 0, ?2, 9000,
                     'unmatched', ?3, '2026-08-31', '2026-08-31')",
            params![summary, expense, account_id],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// 旧 bank_transaction_matches 迁移为 allocation（spec 4.9/9.4）：
    /// 能唯一定位付款凭证资金行的迁入并保留 legacy ID；不可定位的进报告不静默丢失；
    /// 重跑幂等；旧表只读保留；迁移后流水侧余额在新引擎中视为已核销。
    #[test]
    fn test_legacy_bank_match_migration_to_allocations() {
        let mut conn = setup_financial_db();
        fill_employee_bank_info(&conn);
        make_august_closable(&conn);
        let acc = make_batch_fund_account(&conn, "BANK-T1");
        let operator = batch_operator();
        let detail = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "salary".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: None,
            },
            &operator,
        )
        .unwrap();
        mark_payment_batch_exported(&conn, detail.batch.id).unwrap();
        mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail.batch.id,
                payment_date: "2026-08-31".into(),
            },
            &operator,
        )
        .unwrap();

        // 批次付款凭证的唯一资金行（贷方、挂账户）
        let (fund_line_id, fund_line_credit): (i64, f64) = conn
            .query_row(
                "SELECT vl.id, vl.credit_amount FROM voucher_lines vl
                 JOIN vouchers v ON v.id = vl.voucher_id
                 WHERE v.source_type='salary_payment' AND v.source_id=?1 AND v.status='active'
                   AND vl.fund_account_id=?2 AND vl.credit_amount > 0",
                params![detail.batch.id, acc],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();

        // 可迁移：带账户流水（金额=批次额）
        let tx_id = insert_migration_tx(
            &conn,
            &format!("{} 工资代发", detail.batch.batch_no),
            detail.batch.total_amount,
            Some(acc),
        );
        insert_legacy_match(&conn, tx_id, detail.batch.id, 95);

        // 不可迁移①：流水未归集账户（待归集，spec 4.8），匹配到第二个已付款批次
        // （旧表 active 匹配一流水/一批次唯一，需独立批次承载第二条旧匹配；
        //   2026-08 工资明细已被批次1占用，批次2 锁定并取 2026-07 既有工资）
        lock_salary_results(&conn, "2026-07").unwrap();
        let detail2 = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-07".into(),
                batch_type: "salary".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: None,
            },
            &operator,
        )
        .unwrap();
        mark_payment_batch_exported(&conn, detail2.batch.id).unwrap();
        mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail2.batch.id,
                payment_date: "2026-07-31".into(),
            },
            &operator,
        )
        .unwrap();
        assert!(insert_bank_transaction(
            &conn,
            &BankTransaction {
                id: 0,
                transaction_date: "2026-08-30".into(),
                belong_month: "2026-08".into(),
                summary: Some("待归集旧流水".into()),
                counterparty_name: Some("测试银行".into()),
                counterparty_account: Some("62220000".into()),
                income_amount: 0.0,
                expense_amount: 10.0,
                balance: Some(9000.0),
                status: "matched".into(),
                ignore_reason: None,
                imported_file: None,
                raw_json: None,
                fund_account_id: None,
                fund_account_name: None,
                matched_batch_id: Some(detail2.batch.id),
                matched_batch_no: Some(detail2.batch.batch_no.clone()),
                matched_batch_type: Some("salary".into()),
                matched_amount: Some(10.0),
                match_score: Some(50),
                match_remark: Some("旧批次匹配".into()),
                allocated_amount: None,
                remaining_amount: None,
                created_at: None,
                updated_at: None,
            }
        )
        .unwrap());
        let tx2_id: i64 = conn
            .query_row("SELECT MAX(id) FROM bank_transactions", [], |r| r.get(0))
            .unwrap();
        insert_legacy_match(&conn, tx2_id, detail2.batch.id, 50);

        // 历史噪音：cancelled 旧匹配不迁移（旧表 active 匹配一流水/一批次唯一，
        // cancelled 行不受唯一索引约束，指向批次1 即可）
        conn.execute(
            "INSERT INTO bank_transaction_matches
                (transaction_id, payment_batch_id, match_score, remark, status, created_at)
             VALUES (?1, ?2, 10, '旧批次匹配', 'cancelled', '2026-08-31')",
            params![tx_id, detail.batch.id],
        )
        .unwrap();

        let before: i64 = conn
            .query_row("SELECT COUNT(*) FROM bank_transaction_matches", [], |r| {
                r.get(0)
            })
            .unwrap();

        let report = migrate_legacy_bank_matches(&conn).unwrap();
        assert_eq!(report.total as i64, before, "报告 total 应覆盖旧表全部行");
        assert_eq!(report.active_total, 2, "cancelled 旧行不计入 active");
        assert_eq!(
            report.migrated, 1,
            "可唯一定位的旧匹配应迁移：{:?}",
            report.unconverted
        );
        assert_eq!(
            report.unconverted.len(),
            1,
            "不可定位项必须进报告不静默丢失：{:?}",
            report.unconverted
        );
        assert!(report.unconverted.iter().any(|u| u.reason.contains("归集")));

        // allocation 落库且可追溯 legacy ID、方向/金额与付款凭证资金行一致
        let (alloc_line, alloc_amount, method, score, legacy_id): (i64, f64, String, i32, i64) =
            conn.query_row(
                "SELECT voucher_line_id, allocated_amount, match_method, score, legacy_match_id
                 FROM bank_reconciliation_allocations",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get::<_, Option<i32>>(3)?.unwrap_or(0),
                        r.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(alloc_line, fund_line_id);
        assert!((alloc_amount - fund_line_credit).abs() < 0.005);
        assert_eq!(method, "migrated");
        assert_eq!(score, 95, "迁移应保留旧匹配 score");
        assert!(legacy_id > 0);

        // 重跑幂等：已迁移项不再重复写入
        let report2 = migrate_legacy_bank_matches(&conn).unwrap();
        assert_eq!(report2.migrated, 0);
        assert_eq!(report2.already_migrated, 1);
        let alloc_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bank_reconciliation_allocations",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(alloc_count, 1, "重跑不得产生重复 allocation");

        // 旧表只读保留一个版本周期（行数与内容不因迁移改变）
        let after: i64 = conn
            .query_row("SELECT COUNT(*) FROM bank_transaction_matches", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(before, after);

        // 迁移后流水侧余额在新引擎中视为已核销：无剩余候选
        let item = crate::cashier::preview_bank_allocation_candidates(&conn, tx_id).unwrap();
        assert!(item.candidates.is_empty(), "已核销完成的流水不应再有候选");
    }

    /// 直插一条新引擎核销 allocation（模拟历史脏数据：迁移前流水/分录侧已有占用）
    fn insert_dirty_allocation(conn: &Connection, tx_id: i64, line_id: i64, amount: f64) -> i64 {
        conn.execute(
            "INSERT INTO bank_reconciliation_allocations
                (transaction_id, voucher_line_id, allocated_amount, status, match_method,
                 operator_name, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'active', 'manual', '测试', '2026-08-31', '2026-08-31')",
            params![tx_id, line_id, amount],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// 迁移金额必须取流水侧/分录侧剩余的较小值（Fix Round 1 回归）：
    /// 历史脏数据下流水已被新引擎部分核销时，旧匹配不得按"只减分录侧"的全额迁出；
    /// 流水侧耗尽时跳过进不可定位报告而非写出超额核销。
    #[test]
    fn test_legacy_migration_amount_capped_by_tx_side_allocated() {
        let mut conn = setup_financial_db();
        fill_employee_bank_info(&conn);
        make_august_closable(&conn);
        let acc = make_batch_fund_account(&conn, "BANK-T12");
        let operator = batch_operator();

        // 批次1（2026-08 工资）已付款 → 唯一资金行 L1
        let detail1 = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "salary".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: None,
            },
            &operator,
        )
        .unwrap();
        mark_payment_batch_exported(&conn, detail1.batch.id).unwrap();
        mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail1.batch.id,
                payment_date: "2026-08-31".into(),
            },
            &operator,
        )
        .unwrap();
        let (line1_id, line1_credit): (i64, f64) = conn
            .query_row(
                "SELECT vl.id, vl.credit_amount FROM voucher_lines vl
                 JOIN vouchers v ON v.id = vl.voucher_id
                 WHERE v.source_type='salary_payment' AND v.source_id=?1 AND v.status='active'
                   AND vl.fund_account_id=?2 AND vl.credit_amount > 0",
                params![detail1.batch.id, acc],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(
            line1_credit >= 1000.0,
            "fixture 假设批次额大于测试金额，got {line1_credit}"
        );

        // 批次2（2026-07 工资）已付款 → 资金行 L2，仅作场景①脏数据核销目标
        lock_salary_results(&conn, "2026-07").unwrap();
        let detail2 = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-07".into(),
                batch_type: "salary".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: None,
            },
            &operator,
        )
        .unwrap();
        mark_payment_batch_exported(&conn, detail2.batch.id).unwrap();
        mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail2.batch.id,
                payment_date: "2026-07-31".into(),
            },
            &operator,
        )
        .unwrap();
        let line2_id: i64 = conn
            .query_row(
                "SELECT vl.id FROM voucher_lines vl
                 JOIN vouchers v ON v.id = vl.voucher_id
                 WHERE v.source_type='salary_payment' AND v.source_id=?1 AND v.status='active'
                   AND vl.fund_account_id=?2 AND vl.credit_amount > 0",
                params![detail2.batch.id, acc],
                |r| r.get(0),
            )
            .unwrap();

        // 场景①：流水支出 1000，其中 600 已被新引擎核销到 L2（直改库模拟历史脏数据）。
        // 迁移额应取两侧剩余较小值 min(1000-600, L1-0) = 400，
        // 而非只减分录侧得到的 min(1000-0, L1-0) = 1000。
        let tx1 = insert_migration_tx(&conn, "脏数据流水1", 1000.0, Some(acc));
        insert_dirty_allocation(&conn, tx1, line2_id, 600.0);
        let m1 = insert_legacy_match(&conn, tx1, detail1.batch.id, 90);

        // 场景②：流水支出 400 已被新引擎全额核销到 L1 → 迁移必须跳过进报告
        // （旧表 active 匹配按批次唯一，场景②改挂批次2；其脏数据核销 L1
        //   同时占掉批次1 资金行部分分录侧余额，场景①不受影响）
        let tx2 = insert_migration_tx(&conn, "脏数据流水2", 400.0, Some(acc));
        insert_dirty_allocation(&conn, tx2, line1_id, 400.0);
        insert_legacy_match(&conn, tx2, detail2.batch.id, 80);

        let report = migrate_legacy_bank_matches(&conn).unwrap();
        assert_eq!(report.active_total, 2);
        assert_eq!(report.migrated, 1, "场景①应迁移：{:?}", report.unconverted);
        assert_eq!(
            report.unconverted.len(),
            1,
            "场景②流水侧耗尽必须进报告不静默丢失：{:?}",
            report.unconverted
        );
        assert_eq!(report.unconverted[0].transaction_id, tx2);
        assert!(
            report.unconverted[0].reason.contains("余额不足"),
            "got: {}",
            report.unconverted[0].reason
        );

        // 场景①迁移额 = 400（按 legacy ID 定位迁移行），落在 L1 且保留旧匹配 ID
        let (m_amount, m_line, m_legacy): (f64, i64, i64) = conn
            .query_row(
                "SELECT allocated_amount, voucher_line_id, legacy_match_id
                 FROM bank_reconciliation_allocations WHERE legacy_match_id=?1",
                params![m1],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(m_legacy, m1);
        assert_eq!(m_line, line1_id, "迁移应落在旧匹配批次的资金行");
        assert!(
            (m_amount - 400.0).abs() < 0.005,
            "迁移额应取两侧剩余的较小值 400，got {m_amount}"
        );
        // 守恒：tx1 流水侧全部 active 核销合计 == 支出，未超核销
        let tx1_total: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(allocated_amount),0) FROM bank_reconciliation_allocations
                 WHERE transaction_id=?1 AND status='active'",
                params![tx1],
                |r| r.get(0),
            )
            .unwrap();
        assert!((tx1_total - 1000.0).abs() < 0.005, "got {tx1_total}");
        // 场景②不新增迁移行（仅原有脏数据 1 条）
        let tx2_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bank_reconciliation_allocations WHERE transaction_id=?1",
                params![tx2],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tx2_rows, 1, "耗尽流水不得再写迁移行");

        // 重跑幂等：场景①不重复迁移，场景②仍进报告
        let report2 = migrate_legacy_bank_matches(&conn).unwrap();
        assert_eq!(report2.migrated, 0);
        assert_eq!(report2.already_migrated, 1);
        assert_eq!(report2.unconverted.len(), 1);
    }

    // ==================== Task 11：流水账户化导入与预览 ====================

    /// 现金账户（流水导入禁止目标，spec 4.8）
    fn make_cash_fund_account(conn: &Connection, code: &str) -> i64 {
        conn.execute(
            "INSERT INTO fund_accounts
                (account_code, name, account_type, bank_name, account_no, currency,
                 gl_account_code, opening_date, opening_balance, is_default, is_active,
                 created_at, updated_at)
             VALUES (?1, '测试现金户', 'cash', NULL, NULL, 'CNY',
                     '1001', '2026-08-01', 5000, 0, 1, '2026-08-01', '2026-08-01')",
            params![code],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn bank_tx_fixture(
        summary: &str,
        income: f64,
        expense: f64,
        balance: f64,
        account: Option<i64>,
    ) -> BankTransaction {
        BankTransaction {
            id: 0,
            transaction_date: "2026-08-11".into(),
            belong_month: "2026-08".into(),
            summary: Some(summary.into()),
            counterparty_name: Some("测试对方".into()),
            counterparty_account: Some("6222000001".into()),
            income_amount: income,
            expense_amount: expense,
            balance: Some(balance),
            status: "unmatched".into(),
            ignore_reason: None,
            imported_file: None,
            raw_json: None,
            fund_account_id: account,
            fund_account_name: None,
            matched_batch_id: None,
            matched_batch_no: None,
            matched_batch_type: None,
            matched_amount: None,
            match_score: None,
            match_remark: None,
            allocated_amount: None,
            remaining_amount: None,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn test_bank_transaction_import_with_account_and_cross_account_dedup() {
        let conn = setup_financial_db();
        let acc1 = make_batch_fund_account(&conn, "BANK-T11A");
        let acc2 = make_batch_fund_account(&conn, "BANK-T11B");

        // 带账户落库，查询回显账户 id 与名称
        assert!(insert_bank_transaction(
            &conn,
            &bank_tx_fixture("手续费A", 0.0, 100.0, 9900.0, Some(acc1))
        )
        .unwrap());
        let rows = query_bank_transactions(&conn, &BankTransactionQuery::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fund_account_id, Some(acc1));
        assert_eq!(rows[0].fund_account_name.as_deref(), Some("测试付款户"));

        // 同账户重复导入 → 去重跳过
        assert!(!insert_bank_transaction(
            &conn,
            &bank_tx_fixture("手续费A", 0.0, 100.0, 9900.0, Some(acc1))
        )
        .unwrap());

        // 不同账户同号流水允许（唯一索引含账户维度，spec 4.8）
        assert!(insert_bank_transaction(
            &conn,
            &bank_tx_fixture("手续费A", 0.0, 100.0, 9900.0, Some(acc2))
        )
        .unwrap());
        let acc2_rows = query_bank_transactions(
            &conn,
            &BankTransactionQuery {
                fund_account_id: Some(acc2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(acc2_rows.len(), 1);
        assert_eq!(acc2_rows[0].fund_account_id, Some(acc2));
    }

    #[test]
    fn test_bank_import_rejects_cash_account_and_closed_month() {
        let conn = setup_financial_db();
        let acc = make_batch_fund_account(&conn, "BANK-T11A");
        let cash = make_cash_fund_account(&conn, "CASH-T11");

        // 现金账户不能导入银行流水（spec 4.8：导入必须指定 bank/third_party 账户）
        let err = insert_bank_transaction(
            &conn,
            &bank_tx_fixture("手续费B", 0.0, 50.0, 9950.0, Some(cash)),
        )
        .unwrap_err();
        assert!(err.to_string().contains("现金"), "got: {err:?}");

        // 月结保护：目标月已结 → ensure_month_open 拦截
        conn.execute(
            "INSERT INTO month_closes (month, status, created_at, updated_at)
             VALUES ('2026-08', 'closed', '2026-09-01', '2026-09-01')",
            [],
        )
        .unwrap();
        let err = insert_bank_transaction(
            &conn,
            &bank_tx_fixture("手续费C", 0.0, 50.0, 9950.0, Some(acc)),
        )
        .unwrap_err();
        assert!(err.to_string().contains("月结"), "got: {err:?}");

        // 预览同样拦截：已结月份的行标记 error 且提示月结
        let path =
            std::env::temp_dir().join(format!("bank-t11-closed-{}.csv", conn.last_insert_rowid()));
        std::fs::write(
            &path,
            "交易日期,摘要,对方户名,收入,支出,余额\n2026-08-12,手续费D,测试对方,0,60,9890\n",
        )
        .unwrap();
        let preview = preview_bank_transaction_import(&conn, &path.to_string_lossy(), acc).unwrap();
        assert_eq!(preview.total_rows, 1);
        assert_eq!(preview.error_rows, 1);
        assert!(preview.rows[0]
            .message
            .as_deref()
            .unwrap_or("")
            .contains("月结"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_bank_transaction_import_preview_does_not_write() {
        let conn = setup_financial_db();
        let acc1 = make_batch_fund_account(&conn, "BANK-T11A");
        let acc2 = make_batch_fund_account(&conn, "BANK-T11B");
        let path = std::env::temp_dir().join("bank-t11-preview.csv");
        std::fs::write(
            &path,
            "交易日期,摘要,对方户名,收入,支出,余额\n\
             2026-08-01,货款,客户甲,100.50,0,1200.50\n\
             2026-08-02,手续费,,0,30,1170.50\n\
             2026-08-02,手续费,,0,30,1170.50\n\
             2026-08-03,异常余额,,0,20,9999.00\n",
        )
        .unwrap();
        let path_str = path.to_string_lossy().to_string();

        // 预览：4 行 = 2 正常 + 1 文件内重复 + 1 余额不衔接（warning），不落库
        let preview = preview_bank_transaction_import(&conn, &path_str, acc1).unwrap();
        assert_eq!(preview.fund_account_id, acc1);
        assert_eq!(preview.fund_account_name.as_str(), "测试付款户");
        assert_eq!(preview.total_rows, 4);
        assert_eq!(preview.ok_rows, 2);
        assert_eq!(preview.duplicate_rows, 1);
        assert_eq!(preview.warning_rows, 1);
        assert_eq!(preview.error_rows, 0);
        assert!((preview.income_total - 100.50).abs() < 0.005);
        assert!((preview.expense_total - 80.0).abs() < 0.005);
        assert_eq!(preview.rows[0].direction, "income");
        assert_eq!(preview.rows[1].direction, "expense");
        assert_eq!(preview.rows[2].row_status, "duplicate");
        assert_eq!(preview.rows[3].row_status, "warning");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bank_transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "预览不得写库");

        // 换账户预览：库内无该账户流水，仅文件内重复被标记
        let preview_acc2 = preview_bank_transaction_import(&conn, &path_str, acc2).unwrap();
        assert_eq!(preview_acc2.duplicate_rows, 1);

        // 确认后入库：重复行被唯一索引跳过
        for row in &preview.rows {
            let _ = insert_bank_transaction(
                &conn,
                &BankTransaction {
                    fund_account_id: Some(acc1),
                    imported_file: Some(path_str.clone()),
                    income_amount: row.income_amount,
                    expense_amount: row.expense_amount,
                    balance: row.balance,
                    transaction_date: row.transaction_date.clone(),
                    belong_month: row.belong_month.clone(),
                    summary: row.summary.clone(),
                    counterparty_name: row.counterparty_name.clone(),
                    counterparty_account: row.counterparty_account.clone(),
                    ..bank_tx_fixture("", 0.0, 0.0, 0.0, Some(acc1))
                },
            );
        }
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bank_transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3, "重复行应被去重");

        // 再次预览：全部行与库内同账户流水重复
        let preview_again = preview_bank_transaction_import(&conn, &path_str, acc1).unwrap();
        assert_eq!(preview_again.duplicate_rows, 4);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_auto_match_excludes_unassigned_and_cross_account_transactions() {
        let mut conn = setup_financial_db();
        fill_employee_bank_info(&conn);
        make_august_closable(&conn);
        let acc = make_batch_fund_account(&conn, "BANK-T1");
        let other = make_batch_fund_account(&conn, "BANK-T11C");
        let operator = batch_operator();
        let detail = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "salary".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: None,
            },
            &operator,
        )
        .unwrap();
        mark_payment_batch_exported(&conn, detail.batch.id).unwrap();
        mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail.batch.id,
                payment_date: "2026-08-31".into(),
            },
            &operator,
        )
        .unwrap();

        // 待归集流水（fund_account_id 为 NULL）可查询但不参与自动匹配（spec 4.8）
        assert!(insert_bank_transaction(
            &conn,
            &bank_tx_fixture("工资代发", 0.0, detail.batch.total_amount, 9000.0, None)
        )
        .unwrap());
        let result = auto_match_bank_transactions(&conn, "2026-08").unwrap();
        assert_eq!(result.matched, 0, "待归集流水不能自动匹配");
        assert_eq!(
            query_bank_transactions(&conn, &BankTransactionQuery::default())
                .unwrap()
                .len(),
            1,
            "待归集流水仍可查询"
        );

        // 跨账户流水不匹配批次（批次资金账户为 acc）
        conn.execute(
            "UPDATE bank_transactions SET fund_account_id = ?1",
            params![other],
        )
        .unwrap();
        let result = auto_match_bank_transactions(&conn, "2026-08").unwrap();
        assert_eq!(result.matched, 0, "跨账户流水不能自动匹配");

        // 同账户流水可自动匹配
        conn.execute(
            "UPDATE bank_transactions SET fund_account_id = ?1",
            params![acc],
        )
        .unwrap();
        let result = auto_match_bank_transactions(&conn, "2026-08").unwrap();
        assert_eq!(result.matched, 1);
    }

    #[test]
    fn test_ignore_bank_transaction_requires_open_unmatched_transaction() {
        let conn = setup_financial_db();
        assert!(insert_bank_transaction(
            &conn,
            &BankTransaction {
                id: 0,
                transaction_date: "2026-08-20".into(),
                belong_month: "2026-08".into(),
                summary: Some("利息收入".into()),
                counterparty_name: None,
                counterparty_account: None,
                income_amount: 1.0,
                expense_amount: 0.0,
                balance: Some(1.0),
                status: "unmatched".into(),
                ignore_reason: None,
                imported_file: None,
                raw_json: None,
                fund_account_id: None,
                fund_account_name: None,
                matched_batch_id: None,
                matched_batch_no: None,
                matched_batch_type: None,
                matched_amount: None,
                match_score: None,
                match_remark: None,
                allocated_amount: None,
                remaining_amount: None,
                created_at: None,
                updated_at: None,
            }
        )
        .unwrap());
        let tx = query_bank_transactions(
            &conn,
            &BankTransactionQuery {
                belong_month: Some("2026-08".into()),
                status: Some("unmatched".into()),
                keyword: None,
                fund_account_id: None,
            },
        )
        .unwrap()
        .remove(0);

        ignore_bank_transaction(
            &conn,
            &BankTransactionIgnoreInput {
                transaction_id: tx.id,
                reason: "非付款流水".into(),
            },
        )
        .unwrap();
        let ignored = query_bank_transactions(
            &conn,
            &BankTransactionQuery {
                belong_month: Some("2026-08".into()),
                status: Some("ignored".into()),
                keyword: None,
                fund_account_id: None,
            },
        )
        .unwrap();
        assert_eq!(ignored[0].ignore_reason.as_deref(), Some("非付款流水"));
    }

    #[test]
    fn test_closed_month_blocks_payment_batch_writes() {
        let mut conn = setup_financial_db();
        fill_employee_bank_info(&conn);
        lock_salary_results(&conn, "2026-08").unwrap();
        let acc = make_batch_fund_account(&conn, "BANK-T1");
        let operator = batch_operator();
        let detail = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "salary".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: None,
            },
            &operator,
        )
        .unwrap();
        make_august_closable(&conn);
        mark_payment_batch_exported(&conn, detail.batch.id).unwrap();
        mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail.batch.id,
                payment_date: "2026-08-31".into(),
            },
            &operator,
        )
        .unwrap();
        insert_paid_batch_bank_match(&conn, detail.batch.id, detail.batch.total_amount);
        close_month(&conn, "2026-08", "system", None).unwrap();

        let create_err = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "reimbursement".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: None,
            },
            &operator,
        )
        .unwrap_err();
        assert!(matches!(create_err, AppError::InvalidParam(_)));

        let void_err = void_payment_batch(
            &mut conn,
            &PaymentBatchVoidInput {
                id: detail.batch.id,
                reason: "锁账测试".into(),
            },
            &operator,
        )
        .unwrap_err();
        assert!(matches!(void_err, AppError::InvalidParam(_)));

        let remark_err = update_payment_batch_remark(
            &conn,
            &PaymentBatchRemarkInput {
                id: detail.batch.id,
                remark: Some("锁账测试".into()),
            },
        )
        .unwrap_err();
        assert!(matches!(remark_err, AppError::InvalidParam(_)));
    }

    // ==================== general 通用付款批次（第七阶段 Task 9，spec 5.3） ====================

    #[test]
    fn test_general_payment_batch_create_paid_and_settle_flow() {
        let mut conn = setup_financial_db();
        fill_employee_bank_info(&conn);
        let acc = make_batch_fund_account(&conn, "BANK-G1");
        let (partner_id, operator) = make_general_batch_env(&conn);
        let payment_doc = make_approved_fund_doc(
            &conn,
            &operator,
            "payment",
            acc,
            Some(partner_id),
            None,
            "2026-08",
            800.0,
        );
        let advance_doc = make_approved_fund_doc(
            &conn,
            &operator,
            "advance",
            acc,
            None,
            Some(1),
            "2026-08",
            300.0,
        );

        // 创建批次：勾选两张单，账户必选；单据 approved → batched 走状态机
        let detail = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "general".into(),
                fund_account_id: Some(acc),
                source_ids: Some(vec![payment_doc.id, advance_doc.id]),
                remark: Some("通用付款".into()),
            },
            &operator,
        )
        .unwrap();
        assert_eq!(detail.batch.batch_type, "general");
        assert_eq!(detail.batch.fund_account_id, Some(acc));
        assert_eq!(detail.batch.item_count, 2);
        assert_eq!(detail.batch.total_amount, 1100.0);
        // item 快照：往来方银行信息 + 单号
        assert!(detail
            .items
            .iter()
            .all(|item| item.source_type == "fund_document"));
        let pay_item = detail
            .items
            .iter()
            .find(|i| i.source_id == payment_doc.id)
            .unwrap();
        assert_eq!(pay_item.employee_name.as_deref(), Some("测试供应商"));
        assert_eq!(pay_item.bank_account.as_deref(), Some("6222000111"));
        assert_eq!(
            pay_item.remark.as_deref(),
            Some(payment_doc.document_no.as_str())
        );

        // 单据已 batched 且回写批次 id；batch 事件落审批轨迹
        for doc_id in [payment_doc.id, advance_doc.id] {
            let (status, batch_id): (String, i64) = conn
                .query_row(
                    "SELECT status, payment_batch_id FROM fund_documents WHERE id=?1",
                    params![doc_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(status, "batched");
            assert_eq!(batch_id, detail.batch.id);
            let action: String = conn
                .query_row(
                    "SELECT action FROM approval_events WHERE entity_type='fund_document' AND entity_id=?1 ORDER BY id DESC LIMIT 1",
                    params![doc_id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(action, "batch");
        }

        // 导出后标记付款：单据 batched → settled 走状态机，逐单生成资金凭证
        mark_payment_batch_exported(&conn, detail.batch.id).unwrap();
        let paid = mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail.batch.id,
                payment_date: "2026-08-31".into(),
            },
            &operator,
        )
        .unwrap();
        assert_eq!(paid.status, "paid");

        for doc_id in [payment_doc.id, advance_doc.id] {
            let (status, settled_by): (String, Option<i64>) = conn
                .query_row(
                    "SELECT status, settled_by FROM fund_documents WHERE id=?1",
                    params![doc_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(status, "settled");
            assert!(settled_by.is_some());
            // 单据结算凭证 active 且资金行带 fund_account_id（spec 4.7）
            let (voucher_id, credit_account): (i64, String) = conn
                .query_row(
                    "SELECT v.id, vl.account_code
                     FROM vouchers v JOIN voucher_lines vl ON vl.voucher_id = v.id
                     WHERE v.source_type='fund_document' AND v.source_id=?1 AND v.status='active'
                       AND vl.fund_account_id IS NOT NULL",
                    params![doc_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(credit_account, "1002");
            let _ = voucher_id;
        }
        // 无批次级付款凭证：general 批次以单据结算凭证入账，避免重复记账
        let batch_voucher_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vouchers WHERE source_type IN
                    ('salary_payment','reimbursement_payment') AND source_id=?1",
                params![detail.batch.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(batch_voucher_count, 0);
    }

    #[test]
    fn test_general_payment_batch_paid_rolls_back_when_doc_no_longer_batched() {
        let mut conn = setup_financial_db();
        let acc = make_batch_fund_account(&conn, "BANK-G4");
        let (partner_id, operator) = make_general_batch_env(&conn);
        let doc_a = make_approved_fund_doc(
            &conn,
            &operator,
            "payment",
            acc,
            Some(partner_id),
            None,
            "2026-08",
            400.0,
        );
        let doc_b = make_approved_fund_doc(
            &conn,
            &operator,
            "payment",
            acc,
            Some(partner_id),
            None,
            "2026-08",
            600.0,
        );

        let detail = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "general".into(),
                fund_account_id: Some(acc),
                source_ids: Some(vec![doc_a.id, doc_b.id]),
                remark: None,
            },
            &operator,
        )
        .unwrap();
        mark_payment_batch_exported(&conn, detail.batch.id).unwrap();

        // 同批一张单被单独结算（batched → settled，非 batched 状态）
        crate::cashier::settle_fund_document(&conn, &operator, doc_a.id).unwrap();

        // 失败前快照：active 凭证数（doc_a 结算凭证已存在）
        let count_active_vouchers = |conn: &Connection| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM vouchers WHERE status='active'",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        let before_vouchers = count_active_vouchers(&conn);

        let err = mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail.batch.id,
                payment_date: "2026-08-31".into(),
            },
            &operator,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains(&doc_a.document_no),
            "错误应指向已单独结算的单据: {err:?}"
        );

        // 整体回滚：批次仍 exported 且无付款日期
        let (batch_status, batch_date): (String, Option<String>) = conn
            .query_row(
                "SELECT status, payment_date FROM payment_batches WHERE id=?1",
                params![detail.batch.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(batch_status, "exported");
        assert_eq!(batch_date, None);

        // 其余单据仍 batched 且挂原批次；批次明细均未置 paid
        let (doc_b_status, doc_b_batch): (String, Option<i64>) = conn
            .query_row(
                "SELECT status, payment_batch_id FROM fund_documents WHERE id=?1",
                params![doc_b.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(doc_b_status, "batched");
        assert_eq!(doc_b_batch, Some(detail.batch.id));
        let paid_items: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM payment_items WHERE batch_id=?1 AND status='paid'",
                params![detail.batch.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(paid_items, 0);

        // 凭证数不变：无 doc_b 结算凭证，也无批次级凭证
        assert_eq!(count_active_vouchers(&conn), before_vouchers);
    }

    #[test]
    fn test_general_payment_batch_void_releases_documents_back_to_approved() {
        let mut conn = setup_financial_db();
        let acc = make_batch_fund_account(&conn, "BANK-G1");
        let (partner_id, operator) = make_general_batch_env(&conn);
        let doc = make_approved_fund_doc(
            &conn,
            &operator,
            "payment",
            acc,
            Some(partner_id),
            None,
            "2026-08",
            500.0,
        );

        let detail = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "general".into(),
                fund_account_id: Some(acc),
                source_ids: Some(vec![doc.id]),
                remark: None,
            },
            &operator,
        )
        .unwrap();

        // 未付款作废：单据 batched → approved 走状态机释放，可重新入批次
        let voided = void_payment_batch(
            &mut conn,
            &PaymentBatchVoidInput {
                id: detail.batch.id,
                reason: "勾选错误".into(),
            },
            &operator,
        )
        .unwrap();
        assert_eq!(voided.status, "void");
        let (status, batch_id): (String, Option<i64>) = conn
            .query_row(
                "SELECT status, payment_batch_id FROM fund_documents WHERE id=?1",
                params![doc.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "approved");
        assert_eq!(batch_id, None);
        let action: String = conn
            .query_row(
                "SELECT action FROM approval_events WHERE entity_type='fund_document' AND entity_id=?1 ORDER BY id DESC LIMIT 1",
                params![doc.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(action, "unbatch");

        // 释放后可重新进入新批次
        let recreated = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "general".into(),
                fund_account_id: Some(acc),
                source_ids: Some(vec![doc.id]),
                remark: None,
            },
            &operator,
        )
        .unwrap();
        assert_eq!(recreated.batch.item_count, 1);
    }

    #[test]
    fn test_general_payment_batch_same_account_and_paid_void_guard() {
        let mut conn = setup_financial_db();
        let acc_a = make_batch_fund_account(&conn, "BANK-GA");
        let acc_b = make_batch_fund_account(&conn, "BANK-GB");
        let (partner_id, operator) = make_general_batch_env(&conn);
        // doc_a 在账户 A；doc_b 在账户 B；doc_last 属于上月
        let doc_a = make_approved_fund_doc(
            &conn,
            &operator,
            "payment",
            acc_a,
            Some(partner_id),
            None,
            "2026-08",
            100.0,
        );
        let doc_b = make_approved_fund_doc(
            &conn,
            &operator,
            "payment",
            acc_b,
            Some(partner_id),
            None,
            "2026-08",
            200.0,
        );
        let doc_other_month = make_approved_fund_doc(
            &conn,
            &operator,
            "payment",
            acc_a,
            Some(partner_id),
            None,
            "2026-07",
            400.0,
        );

        // 账户 A 的批次只纳入 doc_a（同账户 + 同月，spec 5.3）
        let detail = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "general".into(),
                fund_account_id: Some(acc_a),
                source_ids: Some(vec![doc_a.id, doc_b.id, doc_other_month.id]),
                remark: None,
            },
            &operator,
        )
        .unwrap();
        assert_eq!(detail.batch.item_count, 1);
        assert_eq!(detail.items[0].source_id, doc_a.id);

        // 勾选空集（候选不在该账户下）→ 报错
        let empty_err = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "general".into(),
                fund_account_id: Some(acc_a),
                source_ids: Some(vec![doc_b.id]),
                remark: None,
            },
            &operator,
        )
        .unwrap_err();
        assert!(matches!(empty_err, AppError::InvalidParam(_)));

        // 未选资金账户 → 报错（三种批次一致，spec 5.3）
        let no_account_err = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "general".into(),
                fund_account_id: None,
                source_ids: Some(vec![doc_b.id]),
                remark: None,
            },
            &operator,
        )
        .unwrap_err();
        assert!(matches!(no_account_err, AppError::InvalidParam(_)));

        // 导出并付款后禁止作废（spec 5.3：错误通过冲正单处理）
        mark_payment_batch_exported(&conn, detail.batch.id).unwrap();
        mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: detail.batch.id,
                payment_date: "2026-08-31".into(),
            },
            &operator,
        )
        .unwrap();
        let paid_void_err = void_payment_batch(
            &mut conn,
            &PaymentBatchVoidInput {
                id: detail.batch.id,
                reason: "不应成功".into(),
            },
            &operator,
        )
        .unwrap_err();
        assert!(
            paid_void_err.to_string().contains("不可作废"),
            "got: {paid_void_err:?}"
        );
    }

    #[test]
    fn test_general_payment_batch_requires_counterparty_bank_info() {
        let mut conn = setup_financial_db();
        let acc = make_batch_fund_account(&conn, "BANK-G1");
        let (_partner_id, operator) = make_general_batch_env(&conn);
        // 无银行信息的往来单位
        conn.execute(
            "INSERT INTO business_partners
                (partner_code, name, partner_type, status, created_at, updated_at)
             VALUES ('GYS-T8', '无银行供应商', 'supplier', 'active', '2026-08-01', '2026-08-01')",
            [],
        )
        .unwrap();
        let poor_partner: i64 = conn.last_insert_rowid();
        let doc = make_approved_fund_doc(
            &conn,
            &operator,
            "payment",
            acc,
            Some(poor_partner),
            None,
            "2026-08",
            100.0,
        );
        let err = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "general".into(),
                fund_account_id: Some(acc),
                source_ids: Some(vec![doc.id]),
                remark: None,
            },
            &operator,
        )
        .unwrap_err();
        assert!(err.to_string().contains("银行账号"), "got: {err:?}");
    }

    #[test]
    fn test_legacy_batch_without_fund_account_is_read_only() {
        let mut conn = setup_financial_db();
        fill_employee_bank_info(&conn);
        lock_salary_results(&conn, "2026-08").unwrap();
        let operator = batch_operator();
        // 直插一条历史批次（fund_account_id 为 NULL，模拟第七阶段前的旧数据）
        conn.execute(
            "INSERT INTO payment_batches
                (batch_no, belong_month, batch_type, status, total_amount, item_count,
                 payment_date, remark, created_at, updated_at)
             VALUES ('GZ202608LEGACY', '2026-08', 'salary', 'draft', 14400, 2,
                     NULL, '历史批次', '2026-08-01', '2026-08-01')",
            [],
        )
        .unwrap();
        let legacy_id: i64 = conn
            .query_row(
                "SELECT id FROM payment_batches WHERE batch_no='GZ202608LEGACY'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        // 只读：导出 / 标记付款 / 备注均拒绝
        let export_err = mark_payment_batch_exported(&conn, legacy_id).unwrap_err();
        assert!(
            export_err.to_string().contains("只读"),
            "got: {export_err:?}"
        );
        let paid_err = mark_payment_batch_paid(
            &mut conn,
            &PaymentBatchPaidInput {
                id: legacy_id,
                payment_date: "2026-08-31".into(),
            },
            &operator,
        )
        .unwrap_err();
        assert!(paid_err.to_string().contains("只读"), "got: {paid_err:?}");
        let remark_err = update_payment_batch_remark(
            &conn,
            &PaymentBatchRemarkInput {
                id: legacy_id,
                remark: Some("试试备注".into()),
            },
        )
        .unwrap_err();
        assert!(
            remark_err.to_string().contains("只读"),
            "got: {remark_err:?}"
        );

        // 作废保留为唯一出路（旧批次纠错路径），作废后可重建带账户的新批次
        let voided = void_payment_batch(
            &mut conn,
            &PaymentBatchVoidInput {
                id: legacy_id,
                reason: "历史批次重建".into(),
            },
            &operator,
        )
        .unwrap();
        assert_eq!(voided.status, "void");

        let acc = make_batch_fund_account(&conn, "BANK-T1");
        let recreated = create_payment_batch(
            &mut conn,
            &PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "salary".into(),
                fund_account_id: Some(acc),
                source_ids: None,
                remark: None,
            },
            &operator,
        )
        .unwrap();
        assert_eq!(recreated.batch.item_count, 2);
    }

    #[test]
    fn test_update_void_reimbursement_claim_is_rejected_without_relinking() {
        let conn = setup_financial_db();
        assert!(soft_delete_reimbursement_claim(&conn, 1).unwrap());

        let err = save_reimbursement_claim(
            &conn,
            &ReimbursementClaimInput {
                id: Some(1),
                employee_id: Some(1),
                belong_month: "2026-08".into(),
                title: "作废后编辑".into(),
                invoice_ids: vec![1],
                status: Some("draft".into()),
                payment_status: Some("unpaid".into()),
                payment_date: None,
                remark: None,
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, AppError::NotFound(_)),
            "expected NotFound for void claim update, got {:?}",
            err
        );

        let link_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reimbursement_claim_invoices WHERE claim_id = 1 AND invoice_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(link_count, 1);
    }

    #[test]
    fn test_update_attendance_keeps_identity_when_input_is_blank() {
        let conn = setup_financial_db();

        let record = get_attendance_records(&conn, "2026-08")
            .unwrap()
            .into_iter()
            .find(|row| row.employee_no == "E001")
            .unwrap();

        let updated = update_attendance_record(
            &conn,
            record.id,
            &AttendanceRecordInput {
                id: None,
                salary_month: "".into(),
                employee_no: "".into(),
                name: record.name.clone(),
                expected_days: Some(record.expected_days),
                actual_days: Some(22.0),
                late_count: Some(record.late_count),
                early_leave_count: Some(record.early_leave_count),
                personal_leave_days: Some(record.personal_leave_days),
                sick_leave_days: Some(record.sick_leave_days),
                absent_days: Some(record.absent_days),
                overtime_hours: Some(record.overtime_hours),
                source_type: None,
                ocr_batch_id: None,
                remark: record.remark.clone(),
            },
        )
        .unwrap();

        assert!(updated);

        let records = get_attendance_records(&conn, "2026-08").unwrap();
        let saved = records
            .iter()
            .find(|row| row.employee_no == "E001")
            .unwrap();
        assert_eq!(saved.salary_month, "2026-08");
        assert_eq!(saved.employee_no, "E001");
        assert_eq!(saved.actual_days, 22.0);
    }

    #[test]
    fn test_create_and_delete_attendance_record() {
        let conn = setup_financial_db();
        let created = create_attendance_record(
            &conn,
            &AttendanceRecordInput {
                id: None,
                salary_month: "2026-09".into(),
                employee_no: "E002".into(),
                name: Some("李四".into()),
                expected_days: Some(22.0),
                actual_days: Some(22.0),
                late_count: Some(0),
                early_leave_count: Some(0),
                personal_leave_days: Some(0.0),
                sick_leave_days: Some(0.0),
                absent_days: Some(0.0),
                overtime_hours: Some(1.5),
                source_type: Some("manual".into()),
                ocr_batch_id: None,
                remark: Some("手工新增".into()),
            },
        )
        .unwrap();

        assert_eq!(created.salary_month, "2026-09");
        assert_eq!(created.employee_no, "E002");
        assert_eq!(created.overtime_hours, 1.5);
        assert!(delete_attendance_record(&conn, created.id).unwrap());
        assert!(!delete_attendance_record(&conn, created.id).unwrap());
    }

    #[test]
    fn test_create_employee_rejects_duplicate_employee_no() {
        let conn = setup_financial_db();
        let err = create_employee(
            &conn,
            &EmployeeInput {
                employee_no: " e001 ".into(),
                name: "重复工号".into(),
                department: None,
                position: None,
                id_card: None,
                phone: None,
                bank_account: None,
                bank_name: None,
                hire_date: None,
                status: Some("active".into()),
                base_salary: Some(0.0),
                position_salary: Some(0.0),
                performance_salary: Some(0.0),
                social_security_base: Some(0.0),
                housing_fund_base: Some(0.0),
                special_deduction: Some(0.0),
                remark: None,
            },
        )
        .unwrap_err();

        assert!(
            matches!(err, AppError::InvalidParam(_)),
            "expected duplicate employee no to return InvalidParam, got {:?}",
            err
        );
    }

    #[test]
    fn security_state_table_exists() {
        let conn = setup_db();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM security_state", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn invoices_has_image_encrypted_column() {
        let conn = setup_db();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(invoices)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(cols.iter().any(|c| c == "image_encrypted"));
    }

    #[test]
    fn legacy_migration_state_table_exists() {
        let conn = setup_db();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM legacy_migration_state", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_gl_tables_and_seed() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        seed_gl_accounts(&conn).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM gl_accounts WHERE is_system=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // 《小企业会计准则》官方一级科目共 62 个（资产30+负债12+权益5+成本3+损益12）
        assert!(count >= 62, "预置科目不足: {count}");
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

    /// 跨月发票编辑/软删绕过报销单月份锁账的防护：
    /// approved 报销单 2026-08 已正式月结，挂在其上的发票归属开放月 2026-09，
    /// 编辑/软删该发票时应以 claim 的 belong_month 锁账报错，且 2026-08 计提凭证不被改动。
    fn setup_cross_month_claim_scenario() -> (Connection, i64, String) {
        let conn = setup_financial_db();
        make_august_closable(&conn);
        close_month(&conn, "2026-08", "system", None).unwrap();
        // 生成 claim 1 的报销计提凭证（approved 计提入账，模拟审批链路已走完）
        crate::accounting::generate_reimbursement_accrual_voucher(&conn, 1).unwrap();
        // 新增一张归属 2026-09（开放月）的发票并挂到已月结的 claim 1 上
        let inv = insert_invoice(
            &conn,
            &InvoiceInput {
                invoice_code: Some("X".into()),
                invoice_number: Some("X20260901".into()),
                invoice_type: Some("普通发票".into()),
                issue_date: Some("2026-09-05".into()),
                check_code: None,
                amount: Some(200.0),
                tax_amount: Some(12.0),
                total_amount: Some(212.0),
                seller_name: Some("跨月销售方".into()),
                seller_tax_id: None,
                buyer_name: None,
                buyer_tax_id: None,
                expense_type_code: Some("office".into()),
                employee_id: Some(1),
                belong_month: Some("2026-09".into()),
                remark: None,
                image_path: Some("/tmp/cross.pdf".into()),
                raw_ocr_json: Some("{}".into()),
            },
            "/stored/cross.pdf",
            0,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reimbursement_claim_invoices (claim_id, invoice_id, created_at)
             VALUES (1, ?1, '2026-09-06')",
            params![inv.id],
        )
        .unwrap();
        (conn, inv.id, format!("X20260901"))
    }

    fn active_accrual_voucher_id(conn: &Connection, claim_id: i64) -> Option<i64> {
        conn.query_row(
            "SELECT id FROM vouchers WHERE source_type='reimbursement_accrual'
             AND source_id=?1 AND status='active'",
            params![claim_id],
            |r| r.get(0),
        )
        .ok()
    }

    #[test]
    fn test_update_invoice_blocked_by_closed_claim_month() {
        let (conn, inv_id, number) = setup_cross_month_claim_scenario();
        let voucher_before = active_accrual_voucher_id(&conn, 1);
        assert!(voucher_before.is_some(), "计提凭证应已生成");

        // 编辑发票（改金额，发票自身归属 2026-09 开放）：claim 1 月份 2026-08 已月结，应报错
        let mut input = sample_input("X", &number);
        input.belong_month = Some("2026-09".into());
        input.amount = Some(300.0);
        input.tax_amount = Some(18.0);
        input.total_amount = Some(318.0);
        let err = update_invoice(&conn, inv_id, &input, None).unwrap_err();
        assert!(
            matches!(err, AppError::InvalidParam(ref msg) if msg.contains("2026-08")),
            "expected closed-month InvalidParam, got {:?}",
            err
        );

        // 事务回滚：发票未被改动，2026-08 计提凭证保持原样
        let (amount, updated_count): (f64, i64) = conn
            .query_row(
                "SELECT amount, (SELECT COUNT(*) FROM vouchers WHERE source_type='reimbursement_accrual' AND source_id=1) FROM invoices WHERE id=?1",
                params![inv_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(amount, 200.0, "发票金额不应被改动");
        assert_eq!(updated_count, 1, "计提凭证不应被 void");
        assert_eq!(active_accrual_voucher_id(&conn, 1), voucher_before);
    }

    #[test]
    fn test_soft_delete_invoice_blocked_by_closed_claim_month() {
        let (conn, inv_id, _number) = setup_cross_month_claim_scenario();
        let voucher_before = active_accrual_voucher_id(&conn, 1);
        assert!(voucher_before.is_some(), "计提凭证应已生成");

        // 软删发票（发票自身归属 2026-09 开放）：claim 1 月份 2026-08 已月结，应报错
        let err = soft_delete_invoice(&conn, inv_id).unwrap_err();
        assert!(
            matches!(err, AppError::InvalidParam(ref msg) if msg.contains("2026-08")),
            "expected closed-month InvalidParam, got {:?}",
            err
        );

        // 事务回滚：发票仍 normal，2026-08 计提凭证保持原样
        let (status, updated_count): (String, i64) = conn
            .query_row(
                "SELECT status, (SELECT COUNT(*) FROM vouchers WHERE source_type='reimbursement_accrual' AND source_id=1) FROM invoices WHERE id=?1",
                params![inv_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "normal", "发票不应被作废");
        assert_eq!(updated_count, 1, "计提凭证不应被 void");
        assert_eq!(active_accrual_voucher_id(&conn, 1), voucher_before);
    }
}

#[cfg(test)]
mod social_tests {
    use super::*;

    #[test]
    fn test_social_profile_crud_and_copy() {
        let conn = crate::db::tests::setup_financial_db();
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
        assert_eq!(
            get_social_base_limits(&conn).unwrap(),
            (4590.0, 22950.0, 0.0, 0.0)
        );
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

    #[test]
    fn test_get_annual_tax_summary() {
        // E001：3 个月 gross 15000、社保 1500、公积金 1200/月，专项附加 1000/月，已预扣 300/月
        // 累计应税 = 45000 - 4500 - 3600 - 5000*3 - 1000*3 = 18900 → 3% 档 567 应预扣
        // 已预扣 900 > 567 → 差额 -333（多缴）
        // E002：同口径但已预扣 100/月 → 差额 567 - 300 = 267（少缴）
        let conn = crate::db::tests::setup_financial_db();
        conn.execute_batch(
            "
            UPDATE employees SET special_deduction = 1000 WHERE employee_no IN ('E001','E002');

            DELETE FROM salary_monthly_results;

            INSERT INTO salary_monthly_results
                (salary_month, employee_no, name, department, gross_salary, net_salary,
                 social_security_personal, housing_fund_personal, attendance_deduction,
                 tax_amount, other_deduction, status, locked, created_at, updated_at)
            VALUES
                ('2026-01', 'E001', '张三', '销售部', 15000, 12000, 1500, 1200, 0, 300, 0, 'reviewed', 0, '2026-01-31', '2026-01-31'),
                ('2026-02', 'E001', '张三', '销售部', 15000, 12000, 1500, 1200, 0, 300, 0, 'reviewed', 0, '2026-02-28', '2026-02-28'),
                ('2026-03', 'E001', '张三', '销售部', 15000, 12000, 1500, 1200, 0, 300, 0, 'reviewed', 0, '2026-03-31', '2026-03-31'),
                ('2026-01', 'E002', '李四', '技术部', 15000, 12200, 1500, 1200, 0, 100, 0, 'reviewed', 0, '2026-01-31', '2026-01-31'),
                ('2026-02', 'E002', '李四', '技术部', 15000, 12200, 1500, 1200, 0, 100, 0, 'reviewed', 0, '2026-02-28', '2026-02-28'),
                ('2026-03', 'E002', '李四', '技术部', 15000, 12200, 1500, 1200, 0, 100, 0, 'reviewed', 0, '2026-03-31', '2026-03-31'),
                ('2025-12', 'E001', '张三', '销售部', 15000, 12000, 1500, 1200, 0, 300, 0, 'reviewed', 0, '2025-12-31', '2025-12-31'),
                ('2026-04', 'E001', '张三', '销售部', 15000, 12000, 1500, 1200, 0, 300, 0, 'void', 0, '2026-04-30', '2026-04-30');
            ",
        )
        .unwrap();

        let rows = get_annual_tax_summary(&conn, 2026).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].employee_no, "E001");
        assert_eq!(rows[0].month_count, 3);
        assert_eq!(rows[0].total_gross, 45000.0);
        assert_eq!(rows[0].total_ss_personal, 4500.0);
        assert_eq!(rows[0].total_hf_personal, 3600.0);
        assert_eq!(rows[0].total_special_deduction, 3000.0);
        assert_eq!(rows[0].total_tax_withheld, 900.0);
        assert_eq!(rows[0].annual_tax_due, 567.0);
        assert_eq!(rows[0].difference, -333.0); // 多缴为负

        assert_eq!(rows[1].employee_no, "E002");
        assert_eq!(rows[1].month_count, 3);
        assert_eq!(rows[1].annual_tax_due, 567.0);
        assert_eq!(rows[1].total_tax_withheld, 300.0);
        assert_eq!(rows[1].difference, 267.0); // 少缴为正

        // 其他年份无数据
        assert!(get_annual_tax_summary(&conn, 2027).unwrap().is_empty());
    }
}

/// 第七阶段（7A）DDL 与迁移测试：独立测试模块，复用 db::tests 的建库 helper
#[cfg(test)]
mod stage7_tests {
    use super::tests::setup_financial_db;
    use super::*;

    // ==================== 第七阶段（7A）DDL 与迁移测试 ====================

    /// 模拟 v0.6.1 旧库：仅建第七阶段迁移涉及的老表（老结构，无资金账户表/列/账户维度索引），
    /// 并预置一条资金科目分录、一个付款批次、一条银行流水作为待归集样本。
    pub(crate) fn setup_stage7_legacy_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT);
            CREATE TABLE gl_accounts (
                code TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                category TEXT NOT NULL,
                direction TEXT NOT NULL,
                cash_flow_category TEXT NOT NULL,
                is_system INTEGER DEFAULT 0,
                is_active INTEGER DEFAULT 1,
                remark TEXT,
                created_at TEXT,
                updated_at TEXT
            );
            CREATE TABLE vouchers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                voucher_no TEXT UNIQUE NOT NULL,
                voucher_date TEXT NOT NULL,
                belong_month TEXT NOT NULL,
                source_type TEXT NOT NULL CHECK (source_type IN (
                    'salary_accrual','salary_payment','reimbursement_accrual',
                    'reimbursement_payment','invoice_expense','bank_manual','period_close')),
                source_id INTEGER NOT NULL,
                total_amount REAL NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','void')),
                remark TEXT,
                created_at TEXT,
                updated_at TEXT
            );
            CREATE TABLE voucher_lines (
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
            CREATE TABLE payment_batches (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                batch_no TEXT UNIQUE NOT NULL,
                belong_month TEXT NOT NULL,
                batch_type TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'draft',
                total_amount REAL DEFAULT 0,
                item_count INTEGER DEFAULT 0,
                payment_date TEXT,
                remark TEXT,
                created_at TEXT,
                updated_at TEXT
            );
            CREATE TABLE bank_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                transaction_date TEXT NOT NULL,
                belong_month TEXT NOT NULL,
                summary TEXT,
                counterparty_name TEXT,
                counterparty_account TEXT,
                income_amount REAL DEFAULT 0,
                expense_amount REAL DEFAULT 0,
                balance REAL,
                status TEXT NOT NULL DEFAULT 'unmatched',
                imported_file TEXT,
                raw_json TEXT,
                created_at TEXT,
                updated_at TEXT
            );
            CREATE UNIQUE INDEX idx_bank_transactions_dedup
                ON bank_transactions(transaction_date, COALESCE(summary,''), COALESCE(counterparty_name,''),
                                     COALESCE(counterparty_account,''), income_amount, expense_amount, COALESCE(balance,0));
            INSERT INTO gl_accounts (code, name, category, direction, cash_flow_category) VALUES
                ('1001', '库存现金', 'asset', 'debit', 'none'),
                ('1002', '银行存款', 'asset', 'debit', 'none'),
                ('1012', '其他货币资金', 'asset', 'debit', 'none'),
                ('6602', '管理费用', 'profit_loss', 'debit', 'operating');
            INSERT INTO vouchers (id, voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status)
                VALUES (1, 'V20260801001', '2026-08-01', '2026-08', 'bank_manual', 1, 500, 'active');
            INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount, line_order)
                VALUES (1, '1002', 500, 0, 1), (1, '6602', 0, 500, 2);
            INSERT INTO payment_batches (id, batch_no, belong_month, batch_type, status)
                VALUES (1, 'GZ202608001', '2026-08', 'salary', 'paid');
            INSERT INTO bank_transactions (id, transaction_date, belong_month, summary, income_amount, expense_amount)
                VALUES (1, '2026-08-01', '2026-08', '银行付款', 0, 500);
            ",
        )
        .unwrap();
        conn
    }

    fn stage7_table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            params![name],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false)
    }

    fn stage7_column_exists(conn: &Connection, table: &str, column: &str) -> bool {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|c| c.ok())
            .collect();
        names.iter().any(|n| n == column)
    }

    /// 显式执行 PRAGMA foreign_key_check 并返回违规行数
    fn stage7_fk_violation_count(conn: &Connection) -> i64 {
        let mut stmt = conn.prepare("PRAGMA foreign_key_check").unwrap();
        let mut rows = stmt.query([]).unwrap();
        let mut count = 0i64;
        while rows.next().unwrap().is_some() {
            count += 1;
        }
        count
    }

    fn stage7_setting(conn: &Connection, key: &str) -> Option<String> {
        get_setting(conn, key).unwrap()
    }

    const STAGE7_TEST_TABLES: &[&str] = &[
        "fund_accounts",
        "business_partners",
        "operator_profiles",
        "approval_events",
        "business_attachments",
    ];

    /// Task 10 口径修正（Task 2 挂账承接）：待归集计数必须排除 void 凭证资金分录与 void 付款批次。
    /// 已作废数据无资金意义且永远无法被归集，若计入则待归集计数永远无法清零（spec 9：无法
    /// 确定归属的"有效"数据才进待归集）。
    #[test]
    fn test_stage7_report_excludes_void_vouchers_and_batches() {
        let conn = setup_financial_db();
        conn.execute_batch(
            "
            -- 有效数据：1 流水 + 1 批次 + 1 资金分录（全部待归集）
            INSERT INTO bank_transactions
                (transaction_date, belong_month, summary, expense_amount, status, created_at, updated_at)
                VALUES ('2026-07-01', '2026-07', '历史支出', 500, 'unmatched', '2026-07-01', '2026-07-01');
            INSERT INTO payment_batches (batch_no, belong_month, batch_type, status, created_at, updated_at)
                VALUES ('GZ202607001', '2026-07', 'salary', 'paid', '2026-07-01', '2026-07-01');
            INSERT INTO vouchers
                (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status, created_at, updated_at)
                VALUES ('V1', '2026-07-10', '2026-07', 'bank_manual', 999, 500, 'active', '2026-07-10', '2026-07-10');
            INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount, line_order)
                VALUES (1, '1002', 500, 0, 1);
            -- 作废数据：void 凭证分录 + void 批次（不应计入待归集）
            INSERT INTO vouchers
                (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status, created_at, updated_at)
                VALUES ('V2', '2026-07-11', '2026-07', 'bank_manual', 998, 300, 'void', '2026-07-11', '2026-07-11');
            INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount, line_order)
                VALUES (2, '1002', 300, 0, 1);
            INSERT INTO payment_batches (batch_no, belong_month, batch_type, status, created_at, updated_at)
                VALUES ('GZ202607002', '2026-07', 'salary', 'void', '2026-07-02', '2026-07-02');
            ",
        )
        .unwrap();
        let report = build_stage7_report(&conn).unwrap();
        assert_eq!(report.unassigned_bank_transactions, 1);
        assert_eq!(
            report.unassigned_payment_batches, 1,
            "void 批次不应计入待归集"
        );
        assert_eq!(
            report.unassigned_voucher_lines, 1,
            "void 凭证分录不应计入待归集"
        );
        assert_eq!(report.pending_count, 3);
    }

    #[test]
    fn test_stage7_fresh_db_initializes_fund_tables() {
        // 空库初始化：五张新表、三处可空 fund_account_id 列、迁移状态键齐全
        let conn = setup_financial_db();
        for table in STAGE7_TEST_TABLES {
            assert!(stage7_table_exists(&conn, table), "空库初始化缺表 {table}");
        }
        assert!(stage7_column_exists(
            &conn,
            "voucher_lines",
            "fund_account_id"
        ));
        assert!(stage7_column_exists(
            &conn,
            "payment_batches",
            "fund_account_id"
        ));
        assert!(stage7_column_exists(
            &conn,
            "bank_transactions",
            "fund_account_id"
        ));
        assert_eq!(
            stage7_setting(&conn, "stage7_migration_status").as_deref(),
            Some("done")
        );
        assert_eq!(
            stage7_setting(&conn, "stage7_migration_pending_count").as_deref(),
            Some("0")
        );
        assert!(stage7_setting(&conn, "stage7_migration_completed_at").is_some());
        // 空库不得伪造默认账户（归集向导确认后再建）
        let accounts: i64 = conn
            .query_row("SELECT COUNT(*) FROM fund_accounts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(accounts, 0);
        assert_eq!(stage7_fk_violation_count(&conn), 0);
        // 同一类型最多一个默认账户：第二个默认银行账户应被部分唯一索引拒绝
        conn.execute(
            "INSERT INTO fund_accounts
             (account_code, name, account_type, gl_account_code, is_default)
             VALUES ('BANK-001', '基本户', 'bank', '1002', 1)",
            [],
        )
        .unwrap();
        let dup = conn.execute(
            "INSERT INTO fund_accounts
             (account_code, name, account_type, gl_account_code, is_default)
             VALUES ('BANK-002', '备用户', 'bank', '1002', 1)",
            [],
        );
        assert!(dup.is_err(), "同类型第二个默认账户应违反部分唯一索引");
    }

    #[test]
    fn test_stage7_migration_upgrades_legacy_db() {
        let conn = setup_stage7_legacy_db();
        let report = migrate_stage7_schema(&conn).unwrap();

        for table in STAGE7_TEST_TABLES {
            assert!(stage7_table_exists(&conn, table), "旧库升级缺表 {table}");
        }
        assert!(stage7_column_exists(
            &conn,
            "voucher_lines",
            "fund_account_id"
        ));
        assert!(stage7_column_exists(
            &conn,
            "payment_batches",
            "fund_account_id"
        ));
        assert!(stage7_column_exists(
            &conn,
            "bank_transactions",
            "fund_account_id"
        ));

        // 待归集：1 条银行流水 + 1 个付款批次 + 1 条资金科目分录（6602 分录不计）
        assert_eq!(report.pending_count, 3);
        assert_eq!(report.unassigned_bank_transactions, 1);
        assert_eq!(report.unassigned_payment_batches, 1);
        assert_eq!(report.unassigned_voucher_lines, 1);
        assert_eq!(
            stage7_setting(&conn, "stage7_migration_pending_count").as_deref(),
            Some("3")
        );
        assert_eq!(
            stage7_setting(&conn, "stage7_migration_status").as_deref(),
            Some("done")
        );
        assert!(stage7_setting(&conn, "stage7_migration_completed_at").is_some());

        // 银行流水去重唯一索引已加入账户维度
        let idx_sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='index' AND name='idx_bank_transactions_dedup'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            idx_sql.contains("fund_account_id"),
            "去重唯一索引应包含 fund_account_id 维度：{idx_sql}"
        );

        // 历史数据不猜测归属：保持 NULL 进待归集
        let unassigned: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bank_transactions WHERE fund_account_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(unassigned, 1);
        assert_eq!(stage7_fk_violation_count(&conn), 0);
    }

    #[test]
    fn test_stage7_migration_idempotent() {
        let conn = setup_stage7_legacy_db();
        let first = migrate_stage7_schema(&conn).unwrap();
        let ts_first = stage7_setting(&conn, "stage7_migration_completed_at").unwrap();

        let second = migrate_stage7_schema(&conn).unwrap();
        assert_eq!(first.pending_count, second.pending_count);
        // 重跑迁移不覆盖首次完成时间戳
        assert_eq!(
            stage7_setting(&conn, "stage7_migration_completed_at").as_deref(),
            Some(ts_first.as_str())
        );
        // 迁移本身不写默认账户，重跑不产生重复数据
        let accounts: i64 = conn
            .query_row("SELECT COUNT(*) FROM fund_accounts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(accounts, 0);

        // 已存在默认账户时重跑迁移，不得重复插入
        conn.execute(
            "INSERT INTO fund_accounts
             (account_code, name, account_type, gl_account_code, is_default)
             VALUES ('BANK-001', '基本户', 'bank', '1002', 1)",
            [],
        )
        .unwrap();
        migrate_stage7_schema(&conn).unwrap();
        let accounts: i64 = conn
            .query_row("SELECT COUNT(*) FROM fund_accounts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(accounts, 1);
    }

    /// v0.6.1：vouchers 表 CHECK 白名单扩展 'fund_document'（spec 4.7）。
    /// 旧库须在事务中安全重建 vouchers 表：断言原 id、分录外键关系、索引与旧查询不变，
    /// 新类型可写入、非法类型仍被拦截；重建后外键开关恢复原状态、迁移幂等。
    #[test]
    fn test_stage7_migration_rebuilds_vouchers_check_whitelist() {
        let conn = setup_stage7_legacy_db();
        // 模拟真实应用的连接状态（init_db 会开启外键）
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        let fk_before: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk_before, 1);
        migrate_stage7_schema(&conn).unwrap();

        // 表定义已扩展且保留原白名单类型
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='vouchers'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            sql.contains("fund_document"),
            "vouchers CHECK 应包含 fund_document：{sql}"
        );
        assert!(sql.contains("period_close"), "原类型应保留：{sql}");

        // 原 id 与分录外键关系不变（分录可经 voucher_id 关联到重建后的表）
        let voucher_no: String = conn
            .query_row("SELECT voucher_no FROM vouchers WHERE id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(voucher_no, "V20260801001");
        let joined: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vouchers v JOIN voucher_lines vl ON vl.voucher_id = v.id
                 WHERE v.id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(joined, 2, "原凭证 id=1 的两条分录应完整保留");

        // 三条索引（含同源 active 部分唯一索引）在重建后齐全
        for idx in [
            "idx_vouchers_month",
            "idx_vouchers_source",
            "idx_vouchers_source_active",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
                    params![idx],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "索引 {idx} 应在重建后存在");
        }

        // 新白名单可写入；非法 source_type 与非法 status 仍被 CHECK 拦截（旧查询语义不变）
        conn.execute(
            "INSERT INTO vouchers
                (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status)
             VALUES ('V20260901001', '2026-09-01', '2026-09', 'fund_document', 7, 100, 'active')",
            [],
        )
        .unwrap();
        let bad = conn.execute(
            "INSERT INTO vouchers
                (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status)
             VALUES ('V20260901002', '2026-09-01', '2026-09', 'bogus_type', 7, 100, 'active')",
            [],
        );
        assert!(bad.is_err(), "非法 source_type 仍应被 CHECK 拦截");
        let bad_status = conn.execute(
            "INSERT INTO vouchers
                (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status)
             VALUES ('V20260901003', '2026-09-01', '2026-09', 'fund_document', 7, 100, 'posted')",
            [],
        );
        assert!(bad_status.is_err(), "非法 status 仍应被 CHECK 拦截");
    }

    #[test]
    fn test_stage7_migration_rebuilds_approval_events_action_whitelist() {
        // 模拟旧版第七阶段库：approval_events 还是 8 动作白名单（无 unbatch）
        let conn = setup_financial_db();
        conn.execute(
            "INSERT INTO operator_profiles (name, role, is_active, created_at, updated_at)
             VALUES ('旧操作人', 'cashier', 1, '2026-08-01', '2026-08-01')",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "
            ALTER TABLE approval_events RENAME TO approval_events_old;
            CREATE TABLE approval_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_type TEXT NOT NULL CHECK (entity_type IN ('reimbursement_claim','fund_document')),
                entity_id INTEGER NOT NULL,
                action TEXT NOT NULL CHECK (action IN
                    ('submit','approve','reject','settle','void','reverse','withdraw','batch')),
                from_status TEXT,
                to_status TEXT,
                operator_id INTEGER,
                comment TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (operator_id) REFERENCES operator_profiles(id)
            );
            INSERT INTO approval_events
                (id, entity_type, entity_id, action, from_status, to_status,
                 operator_id, comment, created_at)
            SELECT id, entity_type, entity_id, action, from_status, to_status,
                   operator_id, comment, created_at
            FROM approval_events_old;
            DROP TABLE approval_events_old;
            INSERT INTO approval_events (entity_type, entity_id, action, operator_id, created_at)
             VALUES ('fund_document', 7, 'batch', 1, '2026-08-01');
            ",
        )
        .unwrap();
        // 旧白名单下 unbatch 被拒
        let blocked = conn.execute(
            "INSERT INTO approval_events (entity_type, entity_id, action, operator_id, created_at)
             VALUES ('fund_document', 7, 'unbatch', 1, '2026-08-01')",
            [],
        );
        assert!(blocked.is_err(), "旧白名单应拦截 unbatch");

        migrate_stage7_schema(&conn).unwrap();

        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='approval_events'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            sql.contains("unbatch"),
            "action CHECK 应包含 unbatch：{sql}"
        );
        // 历史轨迹保留，且新动作可写入
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM approval_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 1);
        conn.execute(
            "INSERT INTO approval_events (entity_type, entity_id, action, operator_id, created_at)
             VALUES ('fund_document', 7, 'unbatch', 1, '2026-08-02')",
            [],
        )
        .unwrap();
        let bad = conn.execute(
            "INSERT INTO approval_events (entity_type, entity_id, action, operator_id, created_at)
             VALUES ('fund_document', 7, 'bogus', 1, '2026-08-02')",
            [],
        );
        assert!(bad.is_err(), "非法 action 仍应被 CHECK 拦截");
    }

    #[test]
    fn test_stage7_migration_vouchers_rebuild_idempotent_and_fk_restored() {
        let conn = setup_stage7_legacy_db();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        let fk_before: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk_before, 1);
        migrate_stage7_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO vouchers
                (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status)
             VALUES ('V20260901001', '2026-09-01', '2026-09', 'fund_document', 7, 100, 'active')",
            [],
        )
        .unwrap();

        // 幂等：重跑迁移不重复复制、不丢数据
        migrate_stage7_schema(&conn).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vouchers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);

        // 重建前后连接的外键开关状态保持一致（重建期间临时关闭应被恢复）
        let fk_after: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk_after, fk_before);
        assert_eq!(stage7_fk_violation_count(&conn), 0);
    }

    #[test]
    fn test_stage7_duplicate_default_account_rolls_back() {
        let conn = setup_stage7_legacy_db();
        // 模拟归集向导在同一事务写入默认账户时出现重复默认账户
        let result = run_migration_in_transaction(&conn, |c| {
            create_stage7_tables(c)?;
            c.execute(
                "INSERT INTO fund_accounts
                 (account_code, name, account_type, gl_account_code, is_default)
                 VALUES ('BANK-001', '基本户', 'bank', '1002', 1)",
                [],
            )?;
            // 同类型第二个默认账户 → 部分唯一索引冲突
            c.execute(
                "INSERT INTO fund_accounts
                 (account_code, name, account_type, gl_account_code, is_default)
                 VALUES ('BANK-002', '备用户', 'bank', '1002', 1)",
                [],
            )?;
            Ok(())
        });
        assert!(result.is_err(), "重复默认账户应导致迁移失败");
        // 回滚后建表与第一条 INSERT 均不残留
        assert!(
            !stage7_table_exists(&conn, "fund_accounts"),
            "失败回滚后不应残留 fund_accounts 表"
        );
        // 库仍可用：老数据可正常查询，无外键悬空
        let batches: i64 = conn
            .query_row("SELECT COUNT(*) FROM payment_batches", [], |r| r.get(0))
            .unwrap();
        assert_eq!(batches, 1);
        assert_eq!(stage7_fk_violation_count(&conn), 0);
    }

    #[test]
    fn test_stage7_migration_rejects_orphan_foreign_keys() {
        let conn = setup_stage7_legacy_db();
        // 模拟历史脏数据：分录科目在科目表中不存在（外键悬空）。
        // 旧版本/外部工具可能在关闭外键校验时写入，这里先关闭 pragma 复现该数据状态。
        conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        conn.execute(
            "INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount, line_order)
             VALUES (1, 'ZZZ9', 1, 0, 9)",
            [],
        )
        .unwrap();
        let result = migrate_stage7_schema(&conn);
        assert!(result.is_err(), "存在外键悬空引用时迁移应中止");
        // 回滚：新增列、新表与状态键均不残留
        assert!(!stage7_column_exists(
            &conn,
            "voucher_lines",
            "fund_account_id"
        ));
        assert!(!stage7_column_exists(
            &conn,
            "bank_transactions",
            "fund_account_id"
        ));
        assert!(!stage7_table_exists(&conn, "fund_accounts"));
        assert!(stage7_setting(&conn, "stage7_migration_status").is_none());
        // 库仍可用：分录数据完整
        let lines: i64 = conn
            .query_row("SELECT COUNT(*) FROM voucher_lines", [], |r| r.get(0))
            .unwrap();
        assert_eq!(lines, 3);
    }

    #[test]
    fn test_stage7_partial_ddl_rolls_back() {
        let conn = setup_stage7_legacy_db();
        // 模拟迁移中途失败（部分建表）
        let result: AppResult<()> = run_migration_in_transaction(&conn, |c| {
            c.execute_batch(
                "CREATE TABLE stage7_probe_tmp (id INTEGER PRIMARY KEY);
                 INSERT INTO stage7_probe_tmp VALUES (1);",
            )?;
            Err(AppError::General("模拟迁移中途失败".into()))
        });
        assert!(result.is_err());
        // 建表已回滚，库保持可用
        assert!(!stage7_table_exists(&conn, "stage7_probe_tmp"));
        let txs: i64 = conn
            .query_row("SELECT COUNT(*) FROM bank_transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(txs, 1);
        assert_eq!(stage7_fk_violation_count(&conn), 0);
    }
}
