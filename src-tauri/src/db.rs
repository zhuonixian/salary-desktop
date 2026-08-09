use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::{Datelike, NaiveDate, Utc};
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
        ",
    )?;

    Ok(())
}

fn insert_default_data(conn: &Connection) -> AppResult<()> {
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

    results
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)
}

pub fn get_salary_result_by_employee(
    conn: &Connection,
    month: &str,
    employee_no: &str,
) -> AppResult<SalaryResult> {
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

pub fn update_salary_result(
    conn: &Connection,
    id: i64,
    data: &SalaryResultUpdate,
) -> AppResult<bool> {
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
        total_salary_cost,
        total_invoice_amount,
        approved_reimbursement_amount,
        paid_reimbursement_amount,
    };
    let checks = build_month_close_checks(&summary);
    Ok(MonthCloseWorkbench { summary, checks })
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
                "warning"
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
    ]
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
        monthly_comparison: get_monthly_comparison(conn, &comparison_months)?,
    })
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
) -> AppResult<Invoice> {
    let now = Utc::now().to_rfc3339();
    let data = normalized_invoice_input(data);
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

pub fn update_invoice(
    conn: &Connection,
    id: i64,
    data: &InvoiceInput,
    new_image_path: Option<&str>,
) -> AppResult<bool> {
    let now = Utc::now().to_rfc3339();
    let data = normalized_invoice_input(data);
    let existing = get_invoice(conn, id)?;
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

    let updated = conn.execute(
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
    let claim_id = if let Some(id) = data.id {
        let updated = conn.execute(
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
        conn.execute(
            "DELETE FROM reimbursement_claim_invoices WHERE claim_id = ?1",
            params![id],
        )?;
        id
    } else {
        let claim_no = generate_reimbursement_claim_no(&data.belong_month);
        conn.execute(
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
        conn.last_insert_rowid()
    };

    for invoice_id in &data.invoice_ids {
        conn.execute(
            "INSERT INTO reimbursement_claim_invoices (claim_id, invoice_id, created_at)
             VALUES (?1, ?2, ?3)",
            params![claim_id, invoice_id, now],
        )?;
    }

    get_reimbursement_claim(conn, claim_id)
}

pub fn update_reimbursement_claim_status(
    conn: &Connection,
    id: i64,
    status: Option<String>,
    payment_status: Option<String>,
    payment_date: Option<String>,
) -> AppResult<bool> {
    let existing = get_reimbursement_claim(conn, id)?;
    let new_status = status.unwrap_or(existing.status);
    let new_payment_status = payment_status.unwrap_or(existing.payment_status);
    let now = Utc::now().to_rfc3339();
    let updated = conn.execute(
        "UPDATE reimbursement_claims
         SET status=?1, payment_status=?2, payment_date=?3, updated_at=?4
         WHERE id=?5 AND status != 'void'",
        params![new_status, new_payment_status, payment_date, now, id],
    )?;
    Ok(updated > 0)
}

pub fn soft_delete_reimbursement_claim(conn: &Connection, id: i64) -> AppResult<bool> {
    let now = Utc::now().to_rfc3339();
    let updated = conn.execute(
        "UPDATE reimbursement_claims SET status='void', updated_at=?1 WHERE id=?2 AND status != 'void'",
        params![now, id],
    )?;
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

    fn setup_financial_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
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
        let mut a = sample_input("", "88888");
        a.invoice_code = None;
        let mut b = sample_input("", "88888");
        b.invoice_code = None;
        insert_invoice(&conn, &a, "/a.pdf").unwrap();
        let result = insert_invoice(&conn, &b, "/b.pdf");
        assert!(
            result.is_err(),
            "duplicate full-electronic invoice should be blocked by COALESCE index"
        );
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
        assert!(
            result.is_ok(),
            "soft-deleted invoice should allow re-submission"
        );
    }

    #[test]
    fn test_soft_delete_hides_record() {
        let conn = setup_db();
        let inv = insert_invoice(&conn, &sample_input("333", "444"), "/c.pdf").unwrap();
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
        insert_invoice(&conn, &a, "/a.pdf").unwrap();
        insert_invoice(&conn, &b, "/b.pdf").unwrap();

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
        insert_invoice(&conn, &sample_input("777", "888"), "/d.pdf").unwrap();
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
    fn test_month_close_excludes_void_paid_reimbursements() {
        let conn = setup_financial_db();
        assert!(soft_delete_reimbursement_claim(&conn, 1).unwrap());

        let workbench = get_month_close_workbench(&conn, "2026-08").unwrap();

        assert_eq!(workbench.summary.reimbursement_count, 1);
        assert_eq!(workbench.summary.approved_reimbursement_amount, 500.0);
        assert_eq!(workbench.summary.paid_reimbursement_amount, 0.0);
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
}
