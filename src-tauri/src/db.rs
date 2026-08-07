use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection};

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
    insert_default_data(&conn)?;

    Ok(conn)
}

fn create_tables(conn: &Connection) -> AppResult<()> {
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
        ",
    )?;

    Ok(())
}

fn insert_default_data(conn: &Connection) -> AppResult<()> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM salary_rules",
        [],
        |row| row.get(0),
    )?;

    if count == 0 {
        let default_rules = vec![
            ("late_penalty", "迟到扣款（每次）", 20.0, "attendance"),
            ("early_leave_penalty", "早退扣款（每次）", 20.0, "attendance"),
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

    let tax_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tax_rules",
        [],
        |row| row.get(0),
    )?;

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

    Ok(())
}

// ==================== Employee CRUD ====================

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

    employees.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
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
            data.employee_no, data.name, data.department, data.position,
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

    let status = data.status.clone().unwrap_or(existing.status);
    let base_salary = data.base_salary.unwrap_or(existing.base_salary);
    let position_salary = data.position_salary.unwrap_or(existing.position_salary);
    let performance_salary = data.performance_salary.unwrap_or(existing.performance_salary);
    let social_security_base = data.social_security_base.unwrap_or(existing.social_security_base);
    let housing_fund_base = data.housing_fund_base.unwrap_or(existing.housing_fund_base);
    let special_deduction = data.special_deduction.unwrap_or(existing.special_deduction);

    let updated = conn.execute(
        "UPDATE employees SET employee_no=?1, name=?2, department=?3, position=?4, id_card=?5, phone=?6, bank_account=?7, bank_name=?8, hire_date=?9, status=?10, base_salary=?11, position_salary=?12, performance_salary=?13, social_security_base=?14, housing_fund_base=?15, special_deduction=?16, remark=?17, updated_at=?18 WHERE id=?19",
        params![
            data.employee_no, data.name, data.department, data.position,
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

    employees.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
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

    records.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn upsert_attendance_record(conn: &Connection, data: &AttendanceRecordInput) -> AppResult<bool> {
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
            "UPDATE attendance_records SET salary_month=?1, employee_no=?2, name=?3, expected_days=?4, actual_days=?5, late_count=?6, early_leave_count=?7, personal_leave_days=?8, sick_leave_days=?9, absent_days=?10, overtime_hours=?11, source_type=?12, ocr_batch_id=?13, remark=?14, updated_at=?15 WHERE id=?16",
            params![
                data.salary_month, data.employee_no, data.name, expected_days,
                actual_days, late_count, early_leave_count, personal_leave_days,
                sick_leave_days, absent_days, overtime_hours, data.source_type,
                data.ocr_batch_id, data.remark, now, id
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

pub fn update_attendance_record(conn: &Connection, id: i64, data: &AttendanceRecordInput) -> AppResult<bool> {
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
        "UPDATE attendance_records SET salary_month=?1, employee_no=?2, name=?3, expected_days=?4, actual_days=?5, late_count=?6, early_leave_count=?7, personal_leave_days=?8, sick_leave_days=?9, absent_days=?10, overtime_hours=?11, source_type=?12, ocr_batch_id=?13, remark=?14, updated_at=?15 WHERE id=?16",
        params![
            data.salary_month, data.employee_no, data.name, expected_days,
            actual_days, late_count, early_leave_count, personal_leave_days,
            sick_leave_days, absent_days, overtime_hours, data.source_type,
            data.ocr_batch_id, data.remark, now, id
        ],
    )?;

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

pub fn save_salary_result(conn: &Connection, result: &SalaryResult) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();

    // Try update existing, insert if not found
    let existing = conn.query_row(
        "SELECT id FROM salary_monthly_results WHERE salary_month = ?1 AND employee_no = ?2",
        params![result.salary_month, result.employee_no],
        |row| row.get::<_, i64>(0),
    );

    match existing {
        Ok(existing_id) => {
            conn.execute(
                "UPDATE salary_monthly_results SET name=?1, department=?2, base_salary=?3, position_salary=?4, performance_salary=?5, overtime_salary=?6, meal_allowance=?7, transport_allowance=?8, other_allowance=?9, gross_salary=?10, social_security_personal=?11, housing_fund_personal=?12, attendance_deduction=?13, tax_amount=?14, other_deduction=?15, net_salary=?16, status=?17, remark=?18, updated_at=?19 WHERE id=?20",
                params![
                    result.name, result.department, result.base_salary, result.position_salary,
                    result.performance_salary, result.overtime_salary, result.meal_allowance,
                    result.transport_allowance, result.other_allowance, result.gross_salary,
                    result.social_security_personal, result.housing_fund_personal,
                    result.attendance_deduction, result.tax_amount, result.other_deduction,
                    result.net_salary, result.status, result.remark, now, existing_id
                ],
            )?;
        }
        Err(_) => {
            conn.execute(
                "INSERT INTO salary_monthly_results (salary_month, employee_no, name, department, base_salary, position_salary, performance_salary, overtime_salary, meal_allowance, transport_allowance, other_allowance, gross_salary, social_security_personal, housing_fund_personal, attendance_deduction, tax_amount, other_deduction, net_salary, status, locked, remark, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, 0, ?20, ?21, ?22)",
                params![
                    result.salary_month, result.employee_no, result.name, result.department,
                    result.base_salary, result.position_salary, result.performance_salary,
                    result.overtime_salary, result.meal_allowance, result.transport_allowance,
                    result.other_allowance, result.gross_salary, result.social_security_personal,
                    result.housing_fund_personal, result.attendance_deduction, result.tax_amount,
                    result.other_deduction, result.net_salary, result.status, result.remark, now, now
                ],
            )?;
        }
    }

    Ok(())
}

pub fn get_salary_results(conn: &Connection, month: &str) -> AppResult<Vec<SalaryResult>> {
    let mut stmt = conn.prepare(
        "SELECT id, salary_month, employee_no, name, department, base_salary, position_salary, performance_salary, overtime_salary, meal_allowance, transport_allowance, other_allowance, gross_salary, social_security_personal, housing_fund_personal, attendance_deduction, tax_amount, other_deduction, net_salary, status, locked, remark, created_at, updated_at FROM salary_monthly_results WHERE salary_month = ?1 ORDER BY id"
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
            status: row.get(19)?,
            locked: row.get(20)?,
            remark: row.get(21)?,
            created_at: row.get(22)?,
            updated_at: row.get(23)?,
        })
    })?;

    results.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn get_salary_result_by_employee(conn: &Connection, month: &str, employee_no: &str) -> AppResult<SalaryResult> {
    conn.query_row(
        "SELECT id, salary_month, employee_no, name, department, base_salary, position_salary, performance_salary, overtime_salary, meal_allowance, transport_allowance, other_allowance, gross_salary, social_security_personal, housing_fund_personal, attendance_deduction, tax_amount, other_deduction, net_salary, status, locked, remark, created_at, updated_at FROM salary_monthly_results WHERE salary_month = ?1 AND employee_no = ?2",
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
                status: row.get(19)?,
                locked: row.get(20)?,
                remark: row.get(21)?,
                created_at: row.get(22)?,
                updated_at: row.get(23)?,
            })
        },
    ).map_err(|e| AppError::NotFound(format!("工资结果未找到: {e}")))
}

pub fn update_salary_result(conn: &Connection, id: i64, data: &SalaryResultUpdate) -> AppResult<bool> {
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

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let updated = conn.execute(&sql, params_refs.as_slice())?;

    Ok(updated > 0)
}

pub fn lock_salary_results(conn: &Connection, month: &str) -> AppResult<bool> {
    let updated = conn.execute(
        "UPDATE salary_monthly_results SET locked = 1, status = 'locked', updated_at = ?1 WHERE salary_month = ?2 AND locked = 0",
        params![Utc::now().to_rfc3339(), month],
    )?;
    Ok(updated > 0)
}

pub fn review_salary_results(conn: &Connection, month: &str) -> AppResult<bool> {
    let updated = conn.execute(
        "UPDATE salary_monthly_results SET status = 'reviewed', updated_at = ?1 WHERE salary_month = ?2 AND locked = 0",
        params![Utc::now().to_rfc3339(), month],
    )?;
    Ok(updated > 0)
}

// ==================== OCR ====================

pub fn save_ocr_batch(conn: &Connection, batch: &OcrBatch) -> AppResult<i64> {
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

    batches.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

// ==================== Operation Logs ====================

pub fn log_operation(conn: &Connection, op_type: &str, description: &str, operator: &str, detail: Option<&str>) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO operation_logs (operation_type, description, operator, detail, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![op_type, description, operator, detail, now],
    )?;
    Ok(())
}

// ==================== Dashboard ====================

pub fn get_dashboard_summary(conn: &Connection, month: &str) -> AppResult<DashboardSummary> {
    let employee_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM employees",
        [],
        |row| row.get(0),
    )?;

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

/// 按去重 key 查找已存在的发票。
/// - 全电票（数电票）没有发票代码，code 为 None 时按空字符串匹配。
/// - 配合 schema 中的 `(COALESCE(invoice_code, ''), invoice_number)` 唯一索引，
///   同号码 + 空代码会被正确去重。
pub fn find_invoice_by_dedup_key(conn: &Connection, code: Option<&str>, number: &str) -> AppResult<Option<Invoice>> {
    let code_str = code.unwrap_or("");
    let sql = format!(
        "SELECT {INVOICE_SELECT_FIELDS} FROM invoices \
         WHERE COALESCE(invoice_code, '') = ?1 AND invoice_number = ?2 AND status != 'void' LIMIT 1"
    );
    let result = conn.query_row(&sql, params![code_str, number], row_to_invoice);
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

    // 若改了 code/number，需校验不撞其他记录（code 可空，支持全电票）
    let new_code = data.invoice_code.as_ref().or(existing.invoice_code.as_ref());
    let new_number = data.invoice_number.as_ref().or(existing.invoice_number.as_ref());
    if let Some(n) = new_number {
        if let Some(other) = find_invoice_by_dedup_key(conn, new_code.map(|s| s.as_str()), n)? {
            if other.id != id {
                let code_disp = new_code.cloned().unwrap_or_default();
                return Err(AppError::General(format!(
                    "发票代码{code_disp}+号码{n}已被记录ID={}占用", other.id
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
            CREATE UNIQUE INDEX idx_invoices_code_number ON invoices(COALESCE(invoice_code, ''), invoice_number) WHERE status != 'void';
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
        let inv = insert_invoice(&conn, &input, "/e.pdf").unwrap();
        assert!(inv.invoice_code.is_none());
        let found = find_invoice_by_dedup_key(&conn, None, "99999").unwrap();
        assert!(found.is_some(), "None code should match via COALESCE");
        assert_eq!(found.unwrap().id, inv.id);
    }

    #[test]
    fn test_unique_index_blocks_duplicate_no_code() {
        // 两条全电票同号应被拦截（COALESCE 把 NULL 转 '' 后冲突）
        let conn = setup_db();
        let mut a = sample_input("", "88888"); a.invoice_code = None;
        let mut b = sample_input("", "88888"); b.invoice_code = None;
        insert_invoice(&conn, &a, "/a.pdf").unwrap();
        let result = insert_invoice(&conn, &b, "/b.pdf");
        assert!(result.is_err(), "duplicate full-electronic invoice should be blocked by COALESCE index");
    }

    #[test]
    fn test_unique_index_blocks_duplicate() {
        let conn = setup_db();
        insert_invoice(&conn, &sample_input("111", "222"), "/a.pdf").unwrap();
        let result = insert_invoice(&conn, &sample_input("111", "222"), "/b.pdf");
        assert!(result.is_err(), "重复插入应被唯一索引拦截");
    }

    #[test]
    fn test_soft_delete_allows_resubmission() {
        let conn = setup_db();
        let inv = insert_invoice(&conn, &sample_input("111", "222"), "/a.pdf").unwrap();
        assert!(soft_delete_invoice(&conn, inv.id).unwrap());
        // Re-inserting same code/number should now succeed
        let result = insert_invoice(&conn, &sample_input("111", "222"), "/b.pdf");
        assert!(result.is_ok(), "soft-deleted invoice should allow re-submission");
    }

    #[test]
    fn test_soft_delete_hides_record() {
        let conn = setup_db();
        let inv = insert_invoice(&conn, &sample_input("333", "444"), "/c.pdf").unwrap();
        assert!(soft_delete_invoice(&conn, inv.id).unwrap());
        // find 应该返回 None（因为 status='void' 被过滤）
        assert!(find_invoice_by_dedup_key(&conn, Some("333"), "444").unwrap().is_none());
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
