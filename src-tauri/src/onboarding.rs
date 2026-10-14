//! 第十阶段：上手引导与办税落地辅助（spec 2026-10-11-stage10）
//!
//! 三块职责：
//! 1. bootstrap_status —— 向导与首页检查清单共用的账套状态聚合
//! 2. 示例账套 seed_demo_data / clear_demo_data —— 强隔离双门禁，全程走既有领域函数
//! 3. 办税征期日历 —— 内置规则常量（不入库）+ tax_filing_records 完成留痕
//!
//! 公司信息（company_name / taxpayer_id）存 app_settings，供申报模板表头消费。

use std::sync::Mutex;

use chrono::Datelike;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::cashier::{self, CurrentOperatorState};
use crate::errors::{AppError, AppResult};
use crate::models::{
    AttendanceRecordInput, EmployeeInput, FundAccountInput, FundDocumentInput, InvoiceInput,
    OperatorProfileInput, PaymentBatchInput, ReimbursementClaimInput, SocialInsuranceProfileInput,
};
use crate::security::SecurityState;

// ==================== bootstrap_status ====================

/// 示例账套与 clear 的"空账套"判定表：任一非 0 即视为有业务数据。
/// 覆盖种子会写入的全部业务表（比 spec 的 7 表更宽，防误判）。
const EMPTY_LEDGER_TABLES: &[&str] = &[
    "employees",
    "attendance_records",
    "salary_monthly_results",
    "invoices",
    "reimbursement_claims",
    "payment_batches",
    "bank_transactions",
    "fund_accounts",
    "fund_documents",
    "vouchers",
    "opening_balances",
    "social_insurance_profiles",
    "month_closes",
    "business_partners",
    "negotiable_instruments",
    "cash_count_sheets",
];

fn table_count(conn: &Connection, table: &str) -> AppResult<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    Ok(conn.query_row(&sql, [], |r| r.get(0))?)
}

/// 示例账套/清除门禁：核心业务表全 0 行。
pub fn is_empty_ledger(conn: &Connection) -> AppResult<bool> {
    for table in EMPTY_LEDGER_TABLES {
        if table_count(conn, table)? > 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(Debug, Serialize)]
pub struct BootstrapStatus {
    pub has_password: bool,
    pub has_employees: bool,
    pub has_salary_rules: bool,
    pub has_social_profiles: bool,
    pub has_opening_balance: bool,
    pub has_locked_salary: bool,
    pub is_empty_ledger: bool,
    pub setup_wizard_done: bool,
    pub setup_checklist_dismissed: bool,
    pub has_demo_data: bool,
    pub company_name: Option<String>,
    pub taxpayer_id: Option<String>,
}

pub fn get_bootstrap_status(conn: &Connection) -> AppResult<BootstrapStatus> {
    let has_password: i64 = conn.query_row(
        "SELECT COUNT(*) FROM security_state WHERE id = 1",
        [],
        |r| r.get(0),
    )?;
    Ok(BootstrapStatus {
        has_password: has_password > 0,
        has_employees: table_count(conn, "employees")? > 0,
        // salary_rules 由 insert_default_data 预置，新库即非空（前端清单不展示该项）
        has_salary_rules: table_count(conn, "salary_rules")? > 0,
        has_social_profiles: table_count(conn, "social_insurance_profiles")? > 0,
        has_opening_balance: table_count(conn, "opening_balances")? > 0,
        has_locked_salary: {
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM salary_monthly_results WHERE locked = 1",
                [],
                |r| r.get(0),
            )?;
            n > 0
        },
        is_empty_ledger: is_empty_ledger(conn)?,
        setup_wizard_done: is_setup_wizard_done(conn)?,
        setup_checklist_dismissed: is_setup_checklist_dismissed(conn)?,
        has_demo_data: crate::db::get_setting(conn, DEMO_MARKER_KEY)?
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        company_name: crate::db::get_setting(conn, "company_name")?.filter(|s| !s.is_empty()),
        taxpayer_id: crate::db::get_setting(conn, "taxpayer_id")?.filter(|s| !s.is_empty()),
    })
}

// ==================== 公司信息 ====================

pub fn get_company_info(conn: &Connection) -> AppResult<(Option<String>, Option<String>)> {
    let non_empty = |v: Option<String>| v.filter(|s| !s.is_empty());
    Ok((
        non_empty(crate::db::get_setting(conn, "company_name")?),
        non_empty(crate::db::get_setting(conn, "taxpayer_id")?),
    ))
}

pub fn save_company_info(
    conn: &Connection,
    company_name: Option<String>,
    taxpayer_id: Option<String>,
) -> AppResult<()> {
    let set = |key: &str, value: &Option<String>| -> AppResult<()> {
        match value {
            Some(v) if !v.trim().is_empty() => crate::db::set_setting(conn, key, v.trim())?,
            _ => crate::db::set_setting(conn, key, "")?,
        }
        Ok(())
    };
    set("company_name", &company_name)?;
    set("taxpayer_id", &taxpayer_id)?;
    Ok(())
}

// ==================== 向导/检查清单标记 ====================

pub const SETUP_WIZARD_DONE_KEY: &str = "setup_wizard_done";
pub const SETUP_CHECKLIST_DISMISSED_KEY: &str = "setup_checklist_dismissed";

/// 向导完成标记：'1' 后不再自动进入向导（手动重进入口不受限）。
pub fn finish_setup_wizard(conn: &Connection) -> AppResult<()> {
    crate::db::set_setting(conn, SETUP_WIZARD_DONE_KEY, "1")
}

pub fn is_setup_wizard_done(conn: &Connection) -> AppResult<bool> {
    Ok(crate::db::get_setting(conn, SETUP_WIZARD_DONE_KEY)?
        .map(|v| v == "1")
        .unwrap_or(false))
}

/// 首页检查清单"不再显示"标记。
pub fn dismiss_setup_checklist(conn: &Connection) -> AppResult<()> {
    crate::db::set_setting(conn, SETUP_CHECKLIST_DISMISSED_KEY, "1")
}

pub fn is_setup_checklist_dismissed(conn: &Connection) -> AppResult<bool> {
    Ok(crate::db::get_setting(conn, SETUP_CHECKLIST_DISMISSED_KEY)?
        .map(|v| v == "1")
        .unwrap_or(false))
}

// ==================== 办税征期日历 ====================

/// 征期规则（内置常量，不入库）。due 口径均为自然日，遇节假日顺延以税局公告为准，
/// 不内置节假日表（逐年公告维护成本高且易给错信息）。
struct TaxCalendarRule {
    kind: &'static str,
    label: &'static str,
    /// 每月征期：期次=当月，截止=次月 due_day 日
    monthly_due_day: Option<u32>,
    /// 季度征期：期次=季末月所在季度，截止=季后次月 due_day 日（仅在季末月生成）
    quarterly_due_day: Option<u32>,
    /// 年度征期：期次=上一年度，截止=当年 (month, day)
    annual_due: Option<(u32, u32)>,
    enabled_by_default: bool,
}

const TAX_CALENDAR_RULES: &[TaxCalendarRule] = &[
    TaxCalendarRule {
        kind: "eit_withholding",
        label: "个人所得税扣缴申报",
        monthly_due_day: Some(15),
        quarterly_due_day: None,
        annual_due: None,
        enabled_by_default: true,
    },
    TaxCalendarRule {
        kind: "cit_quarterly",
        label: "企业所得税季度预缴",
        monthly_due_day: None,
        quarterly_due_day: Some(15),
        annual_due: None,
        enabled_by_default: false,
    },
    TaxCalendarRule {
        kind: "eit_annual",
        label: "个税综合所得年度汇算",
        monthly_due_day: None,
        quarterly_due_day: None,
        annual_due: Some((6, 30)),
        enabled_by_default: true,
    },
    TaxCalendarRule {
        kind: "gs_annual_report",
        label: "工商年报",
        monthly_due_day: None,
        quarterly_due_day: None,
        annual_due: Some((6, 30)),
        enabled_by_default: true,
    },
    TaxCalendarRule {
        kind: "social_base",
        label: "社保缴费基数年度申报",
        monthly_due_day: None,
        quarterly_due_day: None,
        annual_due: Some((7, 15)),
        enabled_by_default: false,
    },
];

pub const TAX_CALENDAR_SETTING_KEY: &str = "tax_calendar_enabled_kinds";

#[derive(Debug, Serialize)]
pub struct TaxCalendarItem {
    pub period: String,
    pub kind: String,
    pub label: String,
    pub due_date: String,
    pub days_left: i64,
    pub status: String, // pending | done
    pub overdue: bool,
    pub hint: Option<String>,
}

/// 启用的征期 kinds；未配置时取各规则默认值。
pub fn get_tax_calendar_settings(conn: &Connection) -> AppResult<Vec<String>> {
    let all: Vec<&str> = TAX_CALENDAR_RULES.iter().map(|r| r.kind).collect();
    match crate::db::get_setting(conn, TAX_CALENDAR_SETTING_KEY)? {
        None => Ok(TAX_CALENDAR_RULES
            .iter()
            .filter(|r| r.enabled_by_default)
            .map(|r| r.kind.to_string())
            .collect()),
        Some(json) => {
            let kinds: Vec<String> = serde_json::from_str(&json)
                .map_err(|e| AppError::General(format!("征期设置解析失败: {e}")))?;
            for k in &kinds {
                if !all.contains(&k.as_str()) {
                    return Err(AppError::InvalidParam(format!("未知征期类型: {k}")));
                }
            }
            Ok(kinds)
        }
    }
}

pub fn save_tax_calendar_settings(conn: &Connection, kinds: &[String]) -> AppResult<()> {
    let all: Vec<&str> = TAX_CALENDAR_RULES.iter().map(|r| r.kind).collect();
    for k in kinds {
        if !all.contains(&k.as_str()) {
            return Err(AppError::InvalidParam(format!("未知征期类型: {k}")));
        }
    }
    crate::db::set_setting(
        conn,
        TAX_CALENDAR_SETTING_KEY,
        &serde_json::to_string(kinds)?,
    )?;
    Ok(())
}

/// 生成 [from_month, from_month + months) 视窗内的征期期次，按截止日升序。
pub fn get_tax_calendar(
    conn: &Connection,
    from_month: &str,
    months: u32,
) -> AppResult<Vec<TaxCalendarItem>> {
    let from = chrono::NaiveDate::parse_from_str(&format!("{from_month}-01"), "%Y-%m-%d")
        .map_err(|_| AppError::InvalidParam(format!("月份格式应为 YYYY-MM：{from_month}")))?
        .checked_add_months(chrono::Months::new(0))
        .ok_or_else(|| AppError::General("月份越界".into()))?;
    let today = chrono::Local::now().date_naive();
    let enabled = get_tax_calendar_settings(conn)?;

    let done: std::collections::HashSet<(String, String)> = {
        let mut stmt = conn.prepare("SELECT period, kind FROM tax_filing_records")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>()?.into_iter().collect()
    };

    let mut items: Vec<TaxCalendarItem> = Vec::new();
    for i in 0..months {
        let month_start = from
            .checked_add_months(chrono::Months::new(i))
            .ok_or_else(|| AppError::General("月份越界".into()))?;
        let year = month_start.year();
        let month = month_start.month();
        let period_month = month_start.format("%Y-%m").to_string();

        for rule in TAX_CALENDAR_RULES {
            if !enabled.iter().any(|k| k == rule.kind) {
                continue;
            }
            let (period, due) = if let Some(day) = rule.monthly_due_day {
                let next = month_start
                    .checked_add_months(chrono::Months::new(1))
                    .ok_or_else(|| AppError::General("月份越界".into()))?;
                (
                    period_month.clone(),
                    next.with_day(day)
                        .ok_or_else(|| AppError::General("日期越界".into()))?,
                )
            } else if let Some(day) = rule.quarterly_due_day {
                if !matches!(month, 3 | 6 | 9 | 12) {
                    continue;
                }
                let quarter = (month - 1) / 3 + 1;
                let next = month_start
                    .checked_add_months(chrono::Months::new(1))
                    .ok_or_else(|| AppError::General("月份越界".into()))?;
                (
                    format!("{year}-Q{quarter}"),
                    next.with_day(day)
                        .ok_or_else(|| AppError::General("日期越界".into()))?,
                )
            } else if let Some((m, d)) = rule.annual_due {
                if month != m {
                    continue;
                }
                (
                    (year - 1).to_string(),
                    chrono::NaiveDate::from_ymd_opt(year, m, d)
                        .ok_or_else(|| AppError::General("日期越界".into()))?,
                )
            } else {
                continue;
            };

            let is_done = done.contains(&(period.clone(), rule.kind.to_string()));
            let days_left = (due - today).num_days();
            let hint = if rule.kind == "eit_withholding" && !is_done {
                Some(if prev_month_salary_locked(conn, &period_month)? {
                    "上月工资已锁定，可导出扣缴申报表与申报模板".to_string()
                } else {
                    "上月工资未锁定，请先完成算薪、复核并锁定".to_string()
                })
            } else {
                None
            };
            items.push(TaxCalendarItem {
                period,
                kind: rule.kind.to_string(),
                label: rule.label.to_string(),
                due_date: due.format("%Y-%m-%d").to_string(),
                days_left,
                status: if is_done { "done" } else { "pending" }.to_string(),
                overdue: !is_done && days_left < 0,
                hint,
            });
        }
    }
    items.sort_by(|a, b| a.due_date.cmp(&b.due_date));
    Ok(items)
}

fn prev_month_salary_locked(conn: &Connection, month: &str) -> AppResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month = ?1 AND locked = 1",
        params![month],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// 标记某期次已完成申报（幂等）。删除记录即恢复提醒。
pub fn complete_tax_filing(
    conn: &Connection,
    period: &str,
    kind: &str,
    operator: &str,
) -> AppResult<bool> {
    ensure_known_kind(kind)?;
    let n = conn.execute(
        "INSERT OR IGNORE INTO tax_filing_records (period, kind, completed_at, operator)
         VALUES (?1, ?2, ?3, ?4)",
        params![period, kind, chrono::Utc::now().to_rfc3339(), operator],
    )?;
    Ok(n > 0)
}

pub fn undo_tax_filing(conn: &Connection, period: &str, kind: &str) -> AppResult<bool> {
    ensure_known_kind(kind)?;
    let n = conn.execute(
        "DELETE FROM tax_filing_records WHERE period = ?1 AND kind = ?2",
        params![period, kind],
    )?;
    Ok(n > 0)
}

fn ensure_known_kind(kind: &str) -> AppResult<()> {
    if TAX_CALENDAR_RULES.iter().any(|r| r.kind == kind) {
        Ok(())
    } else {
        Err(AppError::InvalidParam(format!("未知征期类型: {kind}")))
    }
}

// ==================== 示例账套 ====================

const DEMO_MARKER_KEY: &str = "demo_marker";

/// 种子写入记录（roll-back 与 clear 的依据）。JSON 存 app_settings.demo_marker。
#[derive(Debug, Default, Serialize, Deserialize)]
struct DemoMarker {
    operator_id: Option<i64>,
    opening_month: Option<String>,
    months: Vec<String>,
    employee_ids: Vec<i64>,
    social_profile_ids: Vec<i64>,
    invoice_ids: Vec<i64>,
    claim_ids: Vec<i64>,
    partner_ids: Vec<i64>,
    fund_account_ids: Vec<i64>,
    fund_document_ids: Vec<i64>,
    bank_tx_ids: Vec<i64>,
    payment_batch_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
pub struct DemoSeedSummary {
    pub months: Vec<String>,
    pub employees: usize,
    pub invoices: usize,
    pub claims: usize,
    pub fund_documents: usize,
    pub bank_transactions: usize,
    pub payment_batches: usize,
    pub locked_month: Option<String>,
    /// 体验提示：付款批次导出→标记已付款→银行对账可自动匹配对应流水
    pub try_auto_match_amount: Option<f64>,
}

/// 种子示例账套。门禁：核心业务表全 0 行。全程调用既有领域函数，
/// 保证凭证联动与真实操作路径一致；失败时按 marker 回滚已写入数据。
pub fn seed_demo_data(
    mutex_conn: &Mutex<Connection>,
    app_data_dir: &std::path::Path,
    sec: Option<&SecurityState>,
    current: &CurrentOperatorState,
) -> AppResult<DemoSeedSummary> {
    {
        let conn = mutex_conn
            .lock()
            .map_err(|e| AppError::General(e.to_string()))?;
        if !is_empty_ledger(&conn)? {
            return Err(AppError::General(
                "账套已有业务数据，示例数据仅限全新账套体验".into(),
            ));
        }
    }

    let today = chrono::Local::now().date_naive();
    let m_cur = today.format("%Y-%m").to_string();
    let m_prev = today
        .checked_add_months(chrono::Months::new(0))
        .and_then(|d| {
            d.with_day(1)
                .unwrap()
                .checked_sub_months(chrono::Months::new(1))
        })
        .map(|d| d.format("%Y-%m").to_string())
        .ok_or_else(|| AppError::General("月份计算失败".into()))?;
    let expected: f64 = 21.0;

    let mut marker = DemoMarker {
        months: vec![m_prev.clone(), m_cur.clone()],
        opening_month: Some(m_prev.clone()),
        ..Default::default()
    };

    let result = seed_demo_inner(
        mutex_conn,
        app_data_dir,
        sec,
        current,
        &m_prev,
        &m_cur,
        expected,
        &mut marker,
    );
    {
        let conn = mutex_conn
            .lock()
            .map_err(|e| AppError::General(e.to_string()))?;
        match result {
            Ok(summary) => {
                crate::db::set_setting(&conn, DEMO_MARKER_KEY, &serde_json::to_string(&marker)?)?;
                crate::db::log_operation(
                    &conn,
                    "seed_demo_data",
                    &format!(
                        "载入示例账套：{} 名员工，月份 {}，发票 {} 张，报销 {} 单，资金单 {} 张，流水 {} 条",
                        summary.employees,
                        summary.months.join("/"),
                        summary.invoices,
                        summary.claims,
                        summary.fund_documents,
                        summary.bank_transactions
                    ),
                    "system",
                    None,
                )?;
                Ok(summary)
            }
            Err(e) => {
                let _ = rollback_demo_seed(&conn, &marker);
                Err(e)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn seed_demo_inner(
    mutex_conn: &Mutex<Connection>,
    app_data_dir: &std::path::Path,
    sec: Option<&SecurityState>,
    current: &CurrentOperatorState,
    m_prev: &str,
    m_cur: &str,
    expected_days: f64,
    marker: &mut DemoMarker,
) -> AppResult<DemoSeedSummary> {
    let conn = mutex_conn
        .lock()
        .map_err(|e| AppError::General(e.to_string()))?;

    // ---- 1. 示例操作人并设为当前（新账套无操作人，向导少一步） ----
    let (op, _) = cashier::save_operator_profile(
        &conn,
        current,
        &OperatorProfileInput {
            id: None,
            name: "示例出纳".into(),
            role: "cashier".into(),
            is_active: Some(true),
            remark: Some("示例数据，清除示例账套时一并删除".into()),
        },
    )?;
    marker.operator_id = Some(op.id);
    cashier::set_current_operator(&conn, current, op.id)?;

    // ---- 2. 期初余额（启用月 = 上月）：库存现金 5000 + 银行存款 200000 = 实收资本 205000 ----
    crate::accounting::save_opening_balances(
        &conn,
        m_prev,
        &[
            crate::models::OpeningBalanceRow {
                account_code: "1001".into(),
                debit_amount: 5000.0,
                credit_amount: 0.0,
            },
            crate::models::OpeningBalanceRow {
                account_code: "1002".into(),
                debit_amount: 200000.0,
                credit_amount: 0.0,
            },
            crate::models::OpeningBalanceRow {
                account_code: "3001".into(),
                debit_amount: 0.0,
                credit_amount: 205000.0,
            },
        ],
    )?;

    // ---- 3. 资金账户 ×2（期初与账簿一致） ----
    let bank = cashier::save_fund_account(
        &conn,
        &FundAccountInput {
            id: None,
            account_code: "DEMO-BANK".into(),
            name: "工商银行基本户（示例）".into(),
            account_type: "bank".into(),
            bank_name: Some("工商银行".into()),
            account_no: Some("6222020200098765432".into()),
            gl_account_code: "1002".into(),
            opening_date: Some(format!("{m_prev}-01")),
            opening_balance: Some(200000.0),
            is_default: Some(true),
            is_active: Some(true),
            strict_reconciliation: Some(false),
            remark: Some("示例数据".into()),
        },
    )?;
    let cash = cashier::save_fund_account(
        &conn,
        &FundAccountInput {
            id: None,
            account_code: "DEMO-CASH".into(),
            name: "库存现金（示例）".into(),
            account_type: "cash".into(),
            bank_name: None,
            account_no: None,
            gl_account_code: "1001".into(),
            opening_date: Some(format!("{m_prev}-01")),
            opening_balance: Some(5000.0),
            is_default: Some(false),
            is_active: Some(true),
            strict_reconciliation: Some(false),
            remark: Some("示例数据".into()),
        },
    )?;
    marker.fund_account_ids = vec![bank.id, cash.id];

    // ---- 4. 员工 ×8（身份证用 110101 1900 假号段，邮箱 example.com） ----
    let employees_spec: [(&str, &str, &str, &str, f64, f64, f64, f64, f64); 8] = [
        // (工号, 姓名, 部门, 岗位, 基本工资, 岗位工资, 社保基数, 公积金基数, 专项附加)
        (
            "DEMO001",
            "张伟",
            "管理部",
            "总经理",
            15000.0,
            5000.0,
            20000.0,
            20000.0,
            1500.0,
        ),
        (
            "DEMO002",
            "王芳",
            "财务部",
            "会计",
            8000.0,
            2000.0,
            10000.0,
            8000.0,
            1000.0,
        ),
        (
            "DEMO003",
            "李娜",
            "销售部",
            "销售经理",
            7000.0,
            3000.0,
            10000.0,
            8000.0,
            1000.0,
        ),
        (
            "DEMO004",
            "刘强",
            "生产部",
            "车间主任",
            6500.0,
            2500.0,
            9000.0,
            6000.0,
            0.0,
        ),
        (
            "DEMO005",
            "陈静",
            "行政部",
            "行政专员",
            5500.0,
            1500.0,
            7000.0,
            5000.0,
            0.0,
        ),
        (
            "DEMO006",
            "杨帆",
            "销售部",
            "销售专员",
            4500.0,
            2500.0,
            7000.0,
            5000.0,
            0.0,
        ),
        (
            "DEMO007",
            "赵磊",
            "生产部",
            "技术工人",
            5000.0,
            2000.0,
            7000.0,
            5000.0,
            0.0,
        ),
        (
            "DEMO008",
            "孙丽",
            "生产部",
            "操作工",
            4200.0,
            1800.0,
            6000.0,
            4000.0,
            0.0,
        ),
    ];
    for (i, (no, name, dept, pos, base, posi, ss, hf, ded)) in employees_spec.iter().enumerate() {
        let emp = crate::db::create_employee(
            &conn,
            &EmployeeInput {
                employee_no: no.to_string(),
                name: name.to_string(),
                department: Some(dept.to_string()),
                position: Some(pos.to_string()),
                id_card: Some(format!("110101199001{:08}", 100000 + i * 7)),
                phone: Some(format!("138{:08}", 10000000 + i * 111)),
                email: Some(format!("demo{}@example.com", i + 1)),
                bank_account: Some(format!("62220202000{:011}", 10000000 + i)),
                bank_name: Some("工商银行".into()),
                hire_date: Some("2024-03-01".into()),
                status: Some("active".into()),
                base_salary: Some(*base),
                position_salary: Some(*posi),
                performance_salary: Some(0.0),
                social_security_base: Some(*ss),
                housing_fund_base: Some(*hf),
                special_deduction: Some(*ded),
                remark: Some("示例数据".into()),
            },
        )?;
        marker.employee_ids.push(emp.id);

        // ---- 7'. 社保台账（当年，费率对齐全局规则：个人三险 10.5%、公积金双侧 12%） ----
        let year = chrono::Local::now().year() as i64;
        let profile = crate::db::upsert_social_profile(
            &conn,
            &SocialInsuranceProfileInput {
                id: None,
                employee_no: no.to_string(),
                profile_year: year,
                ss_base: Some(*ss),
                hf_base: Some(*hf),
                ss_employer_rate: Some(0.26),
                ss_personal_rate: Some(0.105),
                hf_employer_rate: Some(0.12),
                hf_personal_rate: Some(0.12),
                pension_personal_rate: Some(0.08),
                medical_personal_rate: Some(0.02),
                unemployment_personal_rate: Some(0.005),
                remark: Some("示例数据".into()),
            },
        )?;
        marker.social_profile_ids.push(profile.id);

        // ---- 5. 考勤（两个月）：上月满勤，当月个别异常制造演示差异 ----
        for (month, actual, late, overtime) in [
            (m_prev, expected_days, 0, 4.0),
            (m_cur, expected_days - (i % 3) as f64, (i % 2) as i32, 2.0),
        ] {
            crate::db::upsert_attendance_record(
                &conn,
                &AttendanceRecordInput {
                    id: None,
                    salary_month: month.to_string(),
                    employee_no: no.to_string(),
                    name: Some(name.to_string()),
                    expected_days: Some(expected_days),
                    actual_days: Some(actual),
                    late_count: Some(late),
                    early_leave_count: Some(0),
                    personal_leave_days: Some(if month == m_cur && i == 2 { 1.0 } else { 0.0 }),
                    sick_leave_days: Some(0.0),
                    absent_days: Some(0.0),
                    overtime_hours: Some(overtime),
                    source_type: Some("demo".into()),
                    ocr_batch_id: None,
                    remark: Some("示例数据".into()),
                },
            )?;
        }
    }

    // ---- 6. 算薪：上月复核+锁定（完整月度节奏），当月只算（留给用户演练锁定） ----
    crate::salary::calculate_monthly_salary(m_prev, &conn)?;
    crate::db::review_salary_results(&conn, m_prev)?;
    crate::db::lock_salary_results(&conn, m_prev)?;
    crate::salary::calculate_monthly_salary(m_cur, &conn)?;
    let locked_month = m_prev.to_string();

    drop(conn);

    // ---- 8. 发票 ×3（锁外调用：save_invoice_with_mutex 自行持锁） ----
    let invoice_specs: [(&str, &str, f64, f64, &str); 3] = [
        (
            "北京示例办公用品有限公司",
            "91110108DEMO000001",
            2000.0,
            120.0,
            "office",
        ),
        (
            "上海示例差旅服务有限公司",
            "91310115DEMO000002",
            3500.0,
            210.0,
            "travel",
        ),
        (
            "广州示例餐饮管理有限公司",
            "91440101DEMO000003",
            800.0,
            48.0,
            "meal",
        ),
    ];
    let buyer = {
        let conn = mutex_conn
            .lock()
            .map_err(|e| AppError::General(e.to_string()))?;
        crate::db::get_setting(&conn, "company_name")?
    }
    .unwrap_or_else(|| "示例科技有限公司".into());
    for (i, (seller, tax_id, amount, tax, exp)) in invoice_specs.iter().enumerate() {
        let inv = crate::invoice::save_invoice_with_mutex(
            &InvoiceInput {
                invoice_code: None,
                invoice_number: Some(format!("DEMO{:020}", 10000001 + i)),
                invoice_type: Some("全电发票".into()),
                issue_date: Some(format!("{m_cur}-0{}", i + 2)),
                check_code: None,
                amount: Some(*amount),
                tax_amount: Some(*tax),
                total_amount: Some(amount + tax),
                seller_name: Some(seller.to_string()),
                seller_tax_id: Some(tax_id.to_string()),
                buyer_name: Some(buyer.clone()),
                buyer_tax_id: None,
                expense_type_code: Some(exp.to_string()),
                // 发票归属与报销单员工对齐：发票0→员工0（报销1），发票1/2→员工1（报销2）
                employee_id: Some(marker.employee_ids[if i == 0 { 0 } else { 1 }]),
                belong_month: Some(m_cur.to_string()),
                direction: None,
                partner_id: None,
                remark: Some("示例数据".into()),
                image_path: None,
                raw_ocr_json: None,
            },
            mutex_conn,
            app_data_dir,
            sec,
        )?;
        marker.invoice_ids.push(inv.id);
    }

    let mut conn = mutex_conn
        .lock()
        .map_err(|e| AppError::General(e.to_string()))?;

    // ---- 9. 报销 ×2（提交+审批走完整状态机，审批事件生成凭证） ----
    let claim_specs: [(usize, &str, Vec<usize>, &str); 2] = [
        (0, "办公耗材采购报销", vec![0], "情况属实，同意报销"),
        (1, "出差差旅费报销", vec![1, 2], "票据齐全，同意报销"),
    ];
    for (emp_idx, title, inv_idxs, comment) in claim_specs {
        let claim = crate::db::save_reimbursement_claim(
            &conn,
            &ReimbursementClaimInput {
                id: None,
                employee_id: Some(marker.employee_ids[emp_idx]),
                belong_month: m_cur.to_string(),
                title: title.to_string(),
                invoice_ids: inv_idxs.iter().map(|&i| marker.invoice_ids[i]).collect(),
                remark: Some("示例数据".into()),
            },
        )?;
        cashier::submit_reimbursement_claim(&conn, current, claim.id, Some("示例提交"))?;
        cashier::approve_reimbursement_claim(&conn, current, claim.id, comment)?;
        marker.claim_ids.push(claim.id);
    }

    // ---- 10. 报销付款批次（停在已生成状态：导出→标记已付款留给用户演练，
    //          标记付款后银行对账可自动匹配金额对应的流水） ----
    let batch = crate::db::create_payment_batch(
        &mut conn,
        &PaymentBatchInput {
            belong_month: m_cur.to_string(),
            batch_type: "reimbursement".into(),
            fund_account_id: Some(bank.id),
            source_ids: None,
            remark: Some("示例数据：导出→标记已付款→银行对账自动匹配".into()),
        },
        current,
    )?;
    marker.payment_batch_ids.push(batch.batch.id);
    let try_amount = batch.batch.total_amount;

    // ---- 11. 示例往来单位（客户/供应商）+ 资金单：收款单（审批+直接结算）+
    //          付款单（审批，结算留给付款批次演练） ----
    let customer = cashier::save_business_partner(
        &conn,
        &crate::models::BusinessPartnerInput {
            id: None,
            partner_code: "DEMO-CUST01".into(),
            name: "北京示例客户有限公司".into(),
            partner_type: "customer".into(),
            tax_id: Some("91110108DEMO100001".into()),
            contact_person: Some("钱经理".into()),
            phone: None,
            bank_name: None,
            bank_account: None,
            gl_account_code: Some("1122".into()),
            status: Some("active".into()),
            remark: Some("示例数据".into()),
        },
    )?;
    let supplier = cashier::save_business_partner(
        &conn,
        &crate::models::BusinessPartnerInput {
            id: None,
            partner_code: "DEMO-SUP01".into(),
            name: "北京示例办公用品有限公司".into(),
            partner_type: "supplier".into(),
            tax_id: Some("91110108DEMO000001".into()),
            contact_person: Some("孙经理".into()),
            phone: None,
            bank_name: None,
            bank_account: None,
            gl_account_code: Some("2202".into()),
            status: Some("active".into()),
            remark: Some("示例数据".into()),
        },
    )?;
    marker.partner_ids = vec![customer.id, supplier.id];

    let receipt = cashier::create_fund_document(
        &conn,
        current,
        &FundDocumentInput {
            id: None,
            document_type: "receipt".into(),
            belong_month: m_cur.to_string(),
            document_date: format!("{m_cur}-05"),
            amount: 50000.0,
            summary: "收到客户货款（示例）".into(),
            department: None,
            expense_type: None,
            remark: Some("示例数据".into()),
            partner_id: Some(customer.id),
            employee_id: None,
            source_account_id: None,
            target_account_id: Some(bank.id),
            counter_account_code: Some("6001".into()),
            settlement_mode: None,
            due_date: None,
            advance_allocations: None,
        },
    )?;
    cashier::submit_fund_document(&conn, current, receipt.id, Some("示例提交"))?;
    cashier::approve_fund_document(&conn, current, receipt.id, "合同约定，同意收款")?;
    cashier::settle_fund_document(&conn, current, receipt.id)?;
    marker.fund_document_ids.push(receipt.id);

    let payment_doc = cashier::create_fund_document(
        &conn,
        current,
        &FundDocumentInput {
            id: None,
            document_type: "payment".into(),
            belong_month: m_cur.to_string(),
            document_date: format!("{m_cur}-06"),
            amount: 1200.0,
            summary: "购买办公用品（示例）".into(),
            department: Some("行政部".into()),
            expense_type: Some("office".into()),
            remark: Some("示例数据：付款单须经付款批次标记付款后结算".into()),
            partner_id: Some(supplier.id),
            employee_id: None,
            source_account_id: Some(bank.id),
            target_account_id: None,
            counter_account_code: Some("6602".into()),
            settlement_mode: None,
            due_date: None,
            advance_allocations: None,
        },
    )?;
    cashier::submit_fund_document(&conn, current, payment_doc.id, Some("示例提交"))?;
    cashier::approve_fund_document(&conn, current, payment_doc.id, "预算内支出，同意")?;
    marker.fund_document_ids.push(payment_doc.id);

    // ---- 12. 银行流水 ×4（与批次金额一致的一条供用户标记付款后自动匹配） ----
    let tx_specs: [(i64, f64, f64, &str, &str); 4] = [
        (5, 50000.0, 0.0, "收到XX公司货款", "北京示例客户有限公司"),
        (8, 0.0, try_amount, "批量代付款", "工资代发专户"),
        (10, 20000.0, 0.0, "收到往来款", "天津示例贸易有限公司"),
        (15, 0.0, 3000.0, "支付水电费", "市供电公司"),
    ];
    for (day, income, expense, summary, counterparty) in tx_specs {
        let tx = crate::models::BankTransaction {
            id: 0,
            transaction_date: format!("{m_cur}-{day:02}"),
            belong_month: m_cur.to_string(),
            summary: Some(summary.to_string()),
            counterparty_name: Some(counterparty.to_string()),
            counterparty_account: None,
            income_amount: income,
            expense_amount: expense,
            balance: None,
            status: "unmatched".into(),
            ignore_reason: None,
            imported_file: Some("示例数据".into()),
            raw_json: None,
            fund_account_id: Some(bank.id),
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
        };
        if crate::db::insert_bank_transaction(&conn, &tx)? {
            let tx_id = conn.query_row(
                "SELECT id FROM bank_transactions WHERE belong_month=?1 AND transaction_date=?2
                 AND IFNULL(income_amount,0)=?3 AND IFNULL(expense_amount,0)=?4
                 ORDER BY id DESC LIMIT 1",
                params![m_cur, tx.transaction_date, income, expense],
                |r| r.get(0),
            )?;
            marker.bank_tx_ids.push(tx_id);
        }
    }

    Ok(DemoSeedSummary {
        months: vec![m_prev.to_string(), m_cur.to_string()],
        employees: marker.employee_ids.len(),
        invoices: marker.invoice_ids.len(),
        claims: marker.claim_ids.len(),
        fund_documents: marker.fund_document_ids.len(),
        bank_transactions: marker.bank_tx_ids.len(),
        payment_batches: marker.payment_batch_ids.len(),
        locked_month: Some(locked_month),
        try_auto_match_amount: Some(try_amount),
    })
}

/// 按 marker 回滚种子数据（seed 中途失败时尽力而为；空账套门禁下整表清理等价）。
fn rollback_demo_seed(conn: &Connection, marker: &DemoMarker) -> AppResult<()> {
    delete_demo_business_data(conn)?;
    if let Some(op_id) = marker.operator_id {
        conn.execute(
            "DELETE FROM operator_profiles WHERE id = ?1",
            params![op_id],
        )?;
    }
    Ok(())
}

/// 清空全部核心业务数据（保留科目/规则/设置/审计日志/OCR 记录）。
/// 仅在"业务数据只含种子"的场景调用：clear 前置校验、seed 失败回滚。
fn delete_demo_business_data(conn: &Connection) -> AppResult<()> {
    // 子表在前、父表在后；最终 foreign_key_check 兜底
    const CLEAR_TABLES: &[&str] = &[
        "bank_reconciliation_allocations",
        "bank_reconciliation_periods",
        "bank_transaction_matches",
        "bank_transactions",
        "payment_items",
        "payment_batches",
        "advance_settlement_links",
        "business_attachments",
        "approval_events",
        "fund_documents",
        "reimbursement_claim_invoices",
        "reimbursement_claims",
        "invoices",
        "salary_monthly_results",
        "attendance_records",
        "social_insurance_profiles",
        "employees",
        "business_partners",
        "voucher_lines",
        "vouchers",
        "fund_accounts",
        "opening_balances",
    ];
    let tx = conn.unchecked_transaction()?;
    for table in CLEAR_TABLES {
        tx.execute(&format!("DELETE FROM {table}"), [])
            .map_err(|e| AppError::General(format!("清除 {table} 失败: {e}")))?;
    }
    // foreign_key_check 无悬挂引用时返回空结果集，query_row 会报 NoRows，属通过
    let violations: i64 = tx
        .query_row("PRAGMA foreign_key_check", [], |r| r.get(0))
        .unwrap_or(0);
    if violations > 0 {
        return Err(AppError::General(format!(
            "清除示例数据校验失败：存在 {violations} 条外键悬挂引用"
        )));
    }
    tx.commit()?;
    Ok(())
}

/// 一键清除示例账套。门禁：marker 存在、种子实体数量与库内一致、
/// 未月结且无种子外业务数据（往来单位/票据/盘点/对账期次）。
pub fn clear_demo_data(conn: &Connection, current: &CurrentOperatorState) -> AppResult<()> {
    let marker_json = crate::db::get_setting(conn, DEMO_MARKER_KEY)?
        .ok_or_else(|| AppError::General("当前账套没有示例数据".into()))?;
    let marker: DemoMarker = serde_json::from_str(&marker_json)
        .map_err(|e| AppError::General(format!("示例数据标记解析失败: {e}")))?;

    // ---- 校验：种子实体数量一致，无种子外业务数据 ----
    let checks: [(i64, &str, usize); 10] = [
        (
            table_count(conn, "employees")?,
            "employees",
            marker.employee_ids.len(),
        ),
        (
            table_count(conn, "invoices")?,
            "invoices",
            marker.invoice_ids.len(),
        ),
        (
            table_count(conn, "reimbursement_claims")?,
            "reimbursement_claims",
            marker.claim_ids.len(),
        ),
        (
            table_count(conn, "business_partners")?,
            "business_partners",
            marker.partner_ids.len(),
        ),
        (
            table_count(conn, "fund_accounts")?,
            "fund_accounts",
            marker.fund_account_ids.len(),
        ),
        (
            table_count(conn, "fund_documents")?,
            "fund_documents",
            marker.fund_document_ids.len(),
        ),
        (
            table_count(conn, "bank_transactions")?,
            "bank_transactions",
            marker.bank_tx_ids.len(),
        ),
        (
            table_count(conn, "payment_batches")?,
            "payment_batches",
            marker.payment_batch_ids.len(),
        ),
        (
            table_count(conn, "salary_monthly_results")?,
            "salary_monthly_results",
            marker.employee_ids.len() * marker.months.len(),
        ),
        (
            table_count(conn, "attendance_records")?,
            "attendance_records",
            marker.employee_ids.len() * marker.months.len(),
        ),
    ];
    for (actual, table, expected) in checks {
        if actual != expected as i64 {
            return Err(AppError::General(format!(
                "账套数据与示例数据标记不一致（{table} 库内 {actual} 行，标记 {expected} 行），\
                 可能已录入真实数据，请保留或逐项处理后清除"
            )));
        }
    }
    for table in [
        "month_closes",
        "negotiable_instruments",
        "cash_count_sheets",
    ] {
        let n = table_count(conn, table)?;
        if n > 0 {
            return Err(AppError::General(format!(
                "{table} 存在 {n} 行业务数据（非示例数据），请先处理后清除"
            )));
        }
    }

    delete_demo_business_data(conn)?;
    if let Some(op_id) = marker.operator_id {
        conn.execute(
            "DELETE FROM operator_profiles WHERE id = ?1",
            params![op_id],
        )?;
    }
    crate::db::set_setting(conn, DEMO_MARKER_KEY, "")?;
    crate::db::log_operation(
        conn,
        "clear_demo_data",
        "清除示例账套（业务数据已重置，科目/规则/设置/审计日志保留）",
        &cashier::current_operator_name(conn, current),
        None,
    )?;
    Ok(())
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::seed_gl_accounts(&conn).unwrap();
        crate::db::insert_default_data(&conn).unwrap();
        conn
    }

    #[test]
    fn bootstrap_status_empty_ledger() {
        let conn = setup();
        let status = get_bootstrap_status(&conn).unwrap();
        assert!(status.is_empty_ledger);
        // salary_rules 由默认数据预置
        assert!(status.has_salary_rules);
        assert!(!status.has_password);
        assert!(!status.has_employees);
        assert!(!status.has_locked_salary);
        assert!(status.company_name.is_none());
    }

    #[test]
    fn company_info_roundtrip() {
        let conn = setup();
        save_company_info(
            &conn,
            Some("示例科技有限公司".into()),
            Some("91110108DEMO0001X".into()),
        )
        .unwrap();
        let (name, tax_id) = get_company_info(&conn).unwrap();
        assert_eq!(name.as_deref(), Some("示例科技有限公司"));
        assert_eq!(tax_id.as_deref(), Some("91110108DEMO0001X"));
        // 空串清空
        save_company_info(&conn, Some("".into()), None).unwrap();
        let (name, _) = get_company_info(&conn).unwrap();
        assert_eq!(name, None);
    }

    #[test]
    fn seed_requires_empty_ledger() {
        let mutex_conn = Mutex::new(setup());
        let current = CurrentOperatorState::new();
        let dir = std::env::temp_dir();
        seed_demo_data(&mutex_conn, &dir, None, &current).unwrap();
        let err = seed_demo_data(&mutex_conn, &dir, None, &current).unwrap_err();
        assert!(err.to_string().contains("仅限全新账套"), "{err}");
    }

    #[test]
    fn seed_and_clear_roundtrip() {
        let mutex_conn = Mutex::new(setup());
        let current = CurrentOperatorState::new();
        let dir = std::env::temp_dir();
        let summary = seed_demo_data(&mutex_conn, &dir, None, &current).unwrap();
        assert_eq!(summary.employees, 8);
        assert_eq!(summary.invoices, 3);
        assert_eq!(summary.claims, 2);
        assert_eq!(summary.fund_documents, 2);
        assert_eq!(summary.bank_transactions, 4);
        assert_eq!(summary.payment_batches, 1);
        assert!(summary.locked_month.is_some());
        assert!(summary.try_auto_match_amount.unwrap_or(0.0) > 0.0);

        {
            let conn = mutex_conn.lock().unwrap();
            // 上月已锁定、当月未锁定
            let locked_prev: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month=?1 AND locked=1",
                    params![summary.months[0]],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(locked_prev, 8);
            let locked_cur: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month=?1 AND locked=1",
                    params![summary.months[1]],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(locked_cur, 0);
            // 业务事件生成凭证且借贷平衡（容差比较，排除浮点和误差）
            let unbalanced: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM (
                       SELECT v.id FROM vouchers v
                       JOIN voucher_lines l ON l.voucher_id = v.id
                       GROUP BY v.id
                       HAVING ABS(SUM(l.debit_amount) - SUM(l.credit_amount)) > 0.005
                     )",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(unbalanced, 0, "示例凭证必须借贷平衡");
            // 收款单已结算
            let settled: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM fund_documents WHERE status='settled'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(settled, 1);
            // 批次停在已生成（导出/付款留给用户演练）
            let batched: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM payment_batches WHERE status NOT IN ('void')",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(batched, 1);
        }

        // 清除：实体计数一致 → 成功，业务表归零，marker 清空，无外键悬挂
        {
            let conn = mutex_conn.lock().unwrap();
            clear_demo_data(&conn, &current).unwrap();
            assert!(is_empty_ledger(&conn).unwrap());
            assert!(crate::db::get_setting(&conn, DEMO_MARKER_KEY)
                .unwrap()
                .map_or(true, |v| v.is_empty()));
            let violations: i64 = conn
                .query_row("PRAGMA foreign_key_check", [], |r| r.get(0))
                .unwrap_or(0);
            assert_eq!(violations, 0);
            // 科目/规则/审计日志保留
            let accounts: i64 = conn
                .query_row("SELECT COUNT(*) FROM gl_accounts", [], |r| r.get(0))
                .unwrap();
            assert!(accounts >= 62);
            let logs: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM operation_logs WHERE operation_type='seed_demo_data'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(logs >= 1);
        }

        // 清除后可再次种子（幂等重建）
        let again = seed_demo_data(&mutex_conn, &dir, None, &current).unwrap();
        assert_eq!(again.employees, 8);
        {
            let conn = mutex_conn.lock().unwrap();
            clear_demo_data(&conn, &current).unwrap();
        }
    }

    #[test]
    fn clear_refuses_when_real_data_mixed_in() {
        let mutex_conn = Mutex::new(setup());
        let current = CurrentOperatorState::new();
        let dir = std::env::temp_dir();
        seed_demo_data(&mutex_conn, &dir, None, &current).unwrap();
        {
            let conn = mutex_conn.lock().unwrap();
            // 往来单位（种子不含）视为真实数据
            conn.execute(
                "INSERT INTO business_partners (partner_code, name, partner_type, status, created_at, updated_at)
                 VALUES ('P001', '真实供应商', 'supplier', 'active', '2026-01-01', '2026-01-01')",
                [],
            )
            .unwrap();
            let err = clear_demo_data(&conn, &current).unwrap_err();
            assert!(err.to_string().contains("business_partners"), "{err}");
        }
    }

    #[test]
    fn clear_refuses_when_counts_drift() {
        let mutex_conn = Mutex::new(setup());
        let current = CurrentOperatorState::new();
        let dir = std::env::temp_dir();
        seed_demo_data(&mutex_conn, &dir, None, &current).unwrap();
        {
            let conn = mutex_conn.lock().unwrap();
            // 用户在种子后又录了一名员工 → 数量不一致拒绝清除
            crate::db::create_employee(
                &conn,
                &EmployeeInput {
                    employee_no: "REAL001".into(),
                    name: "真实员工".into(),
                    department: None,
                    position: None,
                    id_card: None,
                    phone: None,
                    email: None,
                    bank_account: None,
                    bank_name: None,
                    hire_date: None,
                    status: None,
                    base_salary: Some(5000.0),
                    position_salary: None,
                    performance_salary: None,
                    social_security_base: None,
                    housing_fund_base: None,
                    special_deduction: None,
                    remark: None,
                },
            )
            .unwrap();
            let err = clear_demo_data(&conn, &current).unwrap_err();
            assert!(err.to_string().contains("employees"), "{err}");
        }
    }

    #[test]
    fn tax_calendar_generation_and_filing() {
        let conn = setup();
        // 默认启用项：eit_withholding / eit_annual / gs_annual_report
        let enabled = get_tax_calendar_settings(&conn).unwrap();
        assert!(enabled.contains(&"eit_withholding".to_string()));
        assert!(!enabled.contains(&"cit_quarterly".to_string()));

        let items = get_tax_calendar(&conn, "2026-10", 3).unwrap();
        // 10/11/12 三月各一条个税按月期次，截止为次月 15 日
        let eit: Vec<&TaxCalendarItem> = items
            .iter()
            .filter(|i| i.kind == "eit_withholding")
            .collect();
        assert_eq!(eit.len(), 3);
        assert_eq!(eit[0].period, "2026-10");
        assert_eq!(eit[0].due_date, "2026-11-15");
        assert_eq!(eit[2].due_date, "2027-01-15");
        // 12 月是季末月但 cit_quarterly 默认关闭 → 不出现
        assert!(!items.iter().any(|i| i.kind == "cit_quarterly"));
        // 视窗不含 6 月 → 无年度期次
        assert!(!items.iter().any(|i| i.kind == "eit_annual"));
        // 个税期次 hint：无工资数据 → 未锁定提示
        assert!(eit[0].hint.as_deref().unwrap().contains("未锁定"));

        // 跨 6 月视窗出现年度期次（汇算/工商年报针对上一年度）
        let june = get_tax_calendar(&conn, "2026-06", 1).unwrap();
        let annual: Vec<&TaxCalendarItem> =
            june.iter().filter(|i| i.kind == "eit_annual").collect();
        assert_eq!(annual.len(), 1);
        assert_eq!(annual[0].period, "2025");
        assert_eq!(annual[0].due_date, "2026-06-30");
        assert!(june
            .iter()
            .any(|i| i.kind == "gs_annual_report" && i.period == "2025"));
        // social_base 默认关闭
        assert!(!june.iter().any(|i| i.kind == "social_base"));
        // 7 月视窗出现社保基数（开启后）
        save_tax_calendar_settings(
            &conn,
            &["eit_withholding".to_string(), "social_base".to_string()],
        )
        .unwrap();
        let july = get_tax_calendar(&conn, "2026-07", 1).unwrap();
        assert!(july
            .iter()
            .any(|i| i.kind == "social_base" && i.period == "2025" && i.due_date == "2026-07-15"));

        // 完成留痕幂等 + 撤销恢复提醒 + 未知 kind 拒绝
        assert!(complete_tax_filing(&conn, "2026-10", "eit_withholding", "测试员").unwrap());
        assert!(!complete_tax_filing(&conn, "2026-10", "eit_withholding", "测试员").unwrap());
        let items = get_tax_calendar(&conn, "2026-10", 1).unwrap();
        assert_eq!(items[0].status, "done");
        assert!(!items[0].overdue);
        assert!(undo_tax_filing(&conn, "2026-10", "eit_withholding").unwrap());
        let items = get_tax_calendar(&conn, "2026-10", 1).unwrap();
        assert_eq!(items[0].status, "pending");
        assert!(complete_tax_filing(&conn, "2026-10", "unknown_kind", "x").is_err());
        assert!(save_tax_calendar_settings(&conn, &["bogus".to_string()]).is_err());
    }

    #[test]
    fn tax_calendar_cross_year_window() {
        let conn = setup();
        let items = get_tax_calendar(&conn, "2025-12", 2).unwrap();
        // 12 月个税期次截止跨年：2026-01-15
        let eit: Vec<&TaxCalendarItem> = items
            .iter()
            .filter(|i| i.kind == "eit_withholding")
            .collect();
        assert_eq!(eit.len(), 2);
        assert_eq!(eit[0].due_date, "2026-01-15");
        assert_eq!(eit[1].period, "2026-01");
        assert_eq!(eit[1].due_date, "2026-02-15");
    }

    #[test]
    fn invalid_month_format_rejected() {
        let conn = setup();
        assert!(get_tax_calendar(&conn, "2026/10", 3).is_err());
        assert!(get_tax_calendar(&conn, "", 3).is_err());
    }
}
