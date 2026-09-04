//! 第七阶段出纳领域模块（cashier）：资金账户 / 往来单位 / 操作人基础资料与当前操作人会话。
//!
//! 模块边界（spec 第 10 节）：
//! - 本模块负责出纳主数据及其业务校验；后续批次在此追加资金单据、审批事件、
//!   资金日记账、银行对账与借款核销。
//! - `db.rs` 只保留 schema/迁移与低层通用 helper；凭证生成、冲正与报表归 `accounting.rs`；
//! - `commands.rs` 负责 State 管理、文件对话框参数与日志编排。
//!
//! 基础资料为主数据：不受 `ensure_month_open` 月结保护（spec 4.4 仅资金单据受月结限制）。
//! 金额一律正数存储，比较容差 0.005；更新入参 `Option` 字段为 patch 语义（None=保留原值，
//! `Some("")`=清空）；错误信息统一中文。

use std::sync::Mutex;

use crate::db;
use crate::errors::{AppError, AppResult};
use crate::models::*;
use chrono::{NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};

/// 金额比较容差
const AMOUNT_TOLERANCE: f64 = 0.005;

/// 资金账户类型（spec 4.1）
const FUND_ACCOUNT_TYPES: &[&str] = &["bank", "cash", "third_party"];

/// 往来单位类型（spec 4.2；员工继续引用 employees，不在本表维护）
const PARTNER_TYPES: &[&str] = &["supplier", "customer", "other"];

/// 往来单位状态
const PARTNER_STATUSES: &[&str] = &["active", "inactive"];

/// 操作人岗位角色（spec 4.3）
const OPERATOR_ROLES: &[&str] = &["requester", "approver", "cashier", "admin"];

// ==================== 当前操作人会话 ====================

/// app_settings 记录最近一次当前操作人选择的键（追溯用；内存 State 才是会话权威）
const ACTIVE_OPERATOR_SETTING: &str = "active_operator_id";

/// 当前操作人会话 State。
///
/// 有意不随锁屏/解锁清空：历史署名不因锁屏丢失（安全事件仍由 security 模块记录）。
/// 仅在当前操作人被停用/注销时失效并要求重新选择；应用重启后为空，
/// 进入业务操作前须通过 `set_current_operator` 重新选择。
pub struct CurrentOperatorState {
    operator_id: Mutex<Option<i64>>,
}

impl CurrentOperatorState {
    pub fn new() -> Self {
        Self {
            operator_id: Mutex::new(None),
        }
    }

    fn get(&self) -> Option<i64> {
        self.operator_id.lock().ok().and_then(|g| *g)
    }

    fn set(&self, id: Option<i64>) {
        if let Ok(mut guard) = self.operator_id.lock() {
            *guard = id;
        }
    }
}

impl Default for CurrentOperatorState {
    fn default() -> Self {
        Self::new()
    }
}

/// 校验并返回当前操作人 `(id, 姓名)`。
/// 未选择、已被删除或已停用时返回错误（要求重新选择）。
/// 供后续批次的资金单据等业务命令强校验署名；安全命令仍记录 `security`。
pub fn require_current_operator(
    conn: &Connection,
    current: &CurrentOperatorState,
) -> AppResult<(i64, String)> {
    let id = current
        .get()
        .ok_or_else(|| AppError::General("尚未选择当前操作人，请先选择操作人".into()))?;
    match conn.query_row(
        "SELECT name, is_active FROM operator_profiles WHERE id = ?1",
        params![id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0)),
    ) {
        Ok((name, true)) => Ok((id, name)),
        Ok((_, false)) => Err(AppError::General(
            "当前操作人已停用，请重新选择操作人".into(),
        )),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(AppError::General(
            "当前操作人已被删除，请重新选择操作人".into(),
        )),
        Err(e) => Err(e.into()),
    }
}

/// 业务基础资料日志署名（尽力而为）：已选有效操作人返回其姓名，否则回退 `system`。
/// 资金单据等业务命令须改用 `require_current_operator` 强校验，不允许静默回退。
pub fn current_operator_name(conn: &Connection, current: &CurrentOperatorState) -> String {
    require_current_operator(conn, current)
        .map(|(_, name)| name)
        .unwrap_or_else(|_| "system".to_string())
}

/// 设置当前操作人：校验存在且启用后写入内存 State，并把最近一次选择
/// 持久化到 `app_settings.active_operator_id`（重启后不自动恢复，须重新选择）。
pub fn set_current_operator(
    conn: &Connection,
    current: &CurrentOperatorState,
    operator_id: i64,
) -> AppResult<OperatorProfile> {
    let profile = get_operator_profile(conn, operator_id)?;
    if !profile.is_active {
        return Err(AppError::General(format!(
            "操作人 {} 已停用，不能设为当前操作人",
            profile.name
        )));
    }
    db::set_setting(conn, ACTIVE_OPERATOR_SETTING, &operator_id.to_string())?;
    current.set(Some(operator_id));
    Ok(profile)
}

/// 查询当前操作人（视图用）：未选择或已失效返回 None，不报错。
pub fn get_current_operator(
    conn: &Connection,
    current: &CurrentOperatorState,
) -> AppResult<Option<OperatorProfile>> {
    let Some(id) = current.get() else {
        return Ok(None);
    };
    let profile = conn
        .query_row(
            &format!(
                "SELECT {OPERATOR_COLS} FROM operator_profiles WHERE id = ?1 AND is_active = 1"
            ),
            params![id],
            operator_from_row,
        )
        .optional()?;
    Ok(profile)
}

// ==================== 操作人档案 ====================

const OPERATOR_COLS: &str = "id, name, role, is_active, remark, created_at, updated_at";

fn operator_from_row(r: &rusqlite::Row) -> rusqlite::Result<OperatorProfile> {
    Ok(OperatorProfile {
        id: r.get(0)?,
        name: r.get(1)?,
        role: r.get(2)?,
        is_active: r.get::<_, i64>(3)? != 0,
        remark: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

/// 查询全部操作人（按 id 升序；前端可按 is_active 过滤展示）
pub fn get_operator_profiles(conn: &Connection) -> AppResult<Vec<OperatorProfile>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {OPERATOR_COLS} FROM operator_profiles ORDER BY id"
    ))?;
    let rows = stmt
        .query_map([], operator_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn get_operator_profile(conn: &Connection, id: i64) -> AppResult<OperatorProfile> {
    conn.query_row(
        &format!("SELECT {OPERATOR_COLS} FROM operator_profiles WHERE id = ?1"),
        params![id],
        operator_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("操作人不存在：id={id}")))
}

/// 保存操作人（id=Some 更新，否则新增）。
/// 至少保留一名启用操作人：停用最后一名启用操作人时拦截；
/// 当前操作人被停用时清除会话，要求重新选择。
/// 返回 (档案, 变更前审计署名)：停用当前操作人会先清空会话，命令层须用该署名写审计，
/// 否则事后取名会退化为 system。
pub fn save_operator_profile(
    conn: &Connection,
    current: &CurrentOperatorState,
    input: &OperatorProfileInput,
) -> AppResult<(OperatorProfile, String)> {
    // 审计署名须在变更前捕获（停用当前操作人会清空会话）
    let actor = current_operator_name(conn, current);
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidParam("操作人姓名不能为空".into()));
    }
    ensure_in_list(&input.role, OPERATOR_ROLES, "操作人角色")?;
    let existing = match input.id {
        Some(id) => Some(get_operator_profile(conn, id)?),
        None => None,
    };
    let is_active = input
        .is_active
        .unwrap_or(existing.as_ref().map(|e| e.is_active).unwrap_or(true));
    let remark = resolve_optional(
        &input.remark,
        existing.as_ref().and_then(|e| e.remark.clone()),
    );
    if !is_active {
        ensure_other_active_operator(conn, input.id)?;
    }
    let now = Utc::now().to_rfc3339();
    let id = match input.id {
        Some(id) => {
            conn.execute(
                "UPDATE operator_profiles SET name = ?2, role = ?3, is_active = ?4, remark = ?5, updated_at = ?6 WHERE id = ?1",
                params![id, name, input.role, is_active as i64, remark, now],
            )?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO operator_profiles (name, role, is_active, remark, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![name, input.role, is_active as i64, remark, now],
            )?;
            conn.last_insert_rowid()
        }
    };
    if !is_active && current.get() == Some(id) {
        // 停用当前操作人：清空会话，要求重新选择（app_settings 保留最近一次选择作追溯）
        current.set(None);
    }
    Ok((get_operator_profile(conn, id)?, actor))
}

/// 启用/停用操作人。停用最后一名启用操作人时拦截；
/// 停用当前操作人时清除会话，要求重新选择。
/// 返回 (档案, 变更前审计署名)：署名在变更前捕获，供命令层写审计（原因同 save_operator_profile）。
pub fn set_active_operator_profile(
    conn: &Connection,
    current: &CurrentOperatorState,
    id: i64,
    active: bool,
) -> AppResult<(OperatorProfile, String)> {
    // 审计署名须在变更前捕获（停用当前操作人会清空会话）
    let actor = current_operator_name(conn, current);
    let existing = get_operator_profile(conn, id)?;
    if existing.is_active == active {
        return Ok((existing, actor));
    }
    if !active {
        ensure_other_active_operator(conn, Some(id))?;
    }
    conn.execute(
        "UPDATE operator_profiles SET is_active = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, active as i64, Utc::now().to_rfc3339()],
    )?;
    if !active && current.get() == Some(id) {
        current.set(None);
    }
    Ok((get_operator_profile(conn, id)?, actor))
}

/// 系统至少保留一名启用操作人：排除 exclude_id 后启用数须大于 0
fn ensure_other_active_operator(conn: &Connection, exclude_id: Option<i64>) -> AppResult<()> {
    let active_count: i64 = match exclude_id {
        Some(id) => conn.query_row(
            "SELECT COUNT(*) FROM operator_profiles WHERE is_active = 1 AND id != ?1",
            params![id],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*) FROM operator_profiles WHERE is_active = 1",
            [],
            |r| r.get(0),
        )?,
    };
    if active_count == 0 {
        return Err(AppError::General("至少需要保留一名启用状态的操作人".into()));
    }
    Ok(())
}

// ==================== 资金账户 ====================

const FUND_ACCOUNT_COLS: &str = "id, account_code, name, account_type, bank_name, account_no, currency, gl_account_code, opening_date, opening_balance, is_default, is_active, remark, created_at, updated_at";

fn fund_account_from_row(r: &rusqlite::Row) -> rusqlite::Result<FundAccount> {
    Ok(FundAccount {
        id: r.get(0)?,
        account_code: r.get(1)?,
        name: r.get(2)?,
        account_type: r.get(3)?,
        bank_name: r.get(4)?,
        account_no: r.get(5)?,
        currency: r.get(6)?,
        gl_account_code: r.get(7)?,
        opening_date: r.get(8)?,
        opening_balance: r.get(9)?,
        is_default: r.get::<_, i64>(10)? != 0,
        is_active: r.get::<_, i64>(11)? != 0,
        remark: r.get(12)?,
        created_at: r.get(13)?,
        updated_at: r.get(14)?,
    })
}

/// 按条件查询资金账户（类型/启用状态/关键字），按类型+编码排序
pub fn get_fund_accounts(conn: &Connection, q: &FundAccountQuery) -> AppResult<Vec<FundAccount>> {
    let mut where_clauses: Vec<String> = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;
    if let Some(t) = q
        .account_type
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        where_clauses.push(format!("account_type = ?{idx}"));
        params_vec.push(Box::new(t.to_string()));
        idx += 1;
    }
    if let Some(a) = q.is_active {
        where_clauses.push(format!("is_active = ?{idx}"));
        params_vec.push(Box::new(a as i64));
        idx += 1;
    }
    if let Some(kw) = q
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        where_clauses.push(format!(
            "(account_code LIKE ?{idx} OR name LIKE ?{idx} OR COALESCE(bank_name,'') LIKE ?{idx} OR COALESCE(account_no,'') LIKE ?{idx})"
        ));
        params_vec.push(Box::new(format!("%{kw}%")));
    }
    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_clauses.join(" AND "))
    };
    let sql = format!(
        "SELECT {FUND_ACCOUNT_COLS} FROM fund_accounts{where_sql} ORDER BY account_type, account_code"
    );
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), fund_account_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn get_fund_account(conn: &Connection, id: i64) -> AppResult<FundAccount> {
    conn.query_row(
        &format!("SELECT {FUND_ACCOUNT_COLS} FROM fund_accounts WHERE id = ?1"),
        params![id],
        fund_account_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("资金账户不存在：id={id}")))
}

/// 保存资金账户（id=Some 更新，否则新增）。
///
/// 校验：编码/名称非空、类型合法、期初余额非负（容差 0.005）、启用日期格式、
/// 挂接科目必须是资金科目（1001/1002/1012）且存在、编码与账号不重复；
/// 同类型默认账户切换在同一事务中完成（满足 partial unique index）；
/// 已被凭证分录/银行流水/付款批次引用的账户不允许修改账户类型（引用保护）。
/// 默认账户不允许停用（须先把同类型其他账户设为默认）。
pub fn save_fund_account(conn: &Connection, input: &FundAccountInput) -> AppResult<FundAccount> {
    let account_code = input.account_code.trim();
    let name = input.name.trim();
    if account_code.is_empty() {
        return Err(AppError::InvalidParam("资金账户编码不能为空".into()));
    }
    if name.is_empty() {
        return Err(AppError::InvalidParam("资金账户名称不能为空".into()));
    }
    ensure_in_list(&input.account_type, FUND_ACCOUNT_TYPES, "资金账户类型")?;

    // 更新为 patch 语义：None 保留原值（避免部分更新误清默认标志/误停用/误改期初）
    let existing = match input.id {
        Some(id) => Some(get_fund_account(conn, id)?),
        None => None,
    };
    let (prev_default, prev_active) = match &existing {
        Some(e) => (e.is_default, e.is_active),
        None => (false, true),
    };
    let is_default = input.is_default.unwrap_or(prev_default);
    let is_active = input.is_active.unwrap_or(prev_active);
    if is_default && !is_active {
        return Err(AppError::InvalidParam("默认账户不能同时停用".into()));
    }

    let opening_balance = match input.opening_balance {
        Some(v) => {
            if v < -AMOUNT_TOLERANCE {
                return Err(AppError::InvalidParam("期初余额不能为负数".into()));
            }
            if v < 0.0 {
                0.0
            } else {
                v
            }
        }
        None => existing.as_ref().map(|e| e.opening_balance).unwrap_or(0.0),
    };
    let opening_date = resolve_optional(
        &input.opening_date,
        existing.as_ref().and_then(|e| e.opening_date.clone()),
    );
    if let Some(d) = &opening_date {
        NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|_| AppError::InvalidParam(format!("启用日期格式应为 YYYY-MM-DD：{d}")))?;
    }
    let gl_account_code = input.gl_account_code.trim();
    if !db::STAGE7_FUND_GL_CODES.contains(&gl_account_code) {
        return Err(AppError::InvalidParam(format!(
            "资金账户只能挂接资金科目（{}）",
            db::STAGE7_FUND_GL_CODES.join(" / ")
        )));
    }
    ensure_gl_account_exists(conn, gl_account_code)?;
    let bank_name = resolve_optional(
        &input.bank_name,
        existing.as_ref().and_then(|e| e.bank_name.clone()),
    );
    let account_no = resolve_optional(
        &input.account_no,
        existing.as_ref().and_then(|e| e.account_no.clone()),
    );
    let remark = resolve_optional(
        &input.remark,
        existing.as_ref().and_then(|e| e.remark.clone()),
    );

    let tx = conn.unchecked_transaction()?;
    // 编码唯一（排除自身）
    let code_dup_id: Option<i64> = tx
        .query_row(
            "SELECT id FROM fund_accounts WHERE account_code = ?1 AND id != ?2",
            params![account_code, input.id.unwrap_or(-1)],
            |r| r.get(0),
        )
        .optional()?;
    if code_dup_id.is_some() {
        return Err(AppError::General(format!(
            "资金账户编码 {account_code} 已存在"
        )));
    }
    // 账号唯一（排除自身；仅非空时校验）
    if let Some(no) = &account_no {
        let no_dup_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM fund_accounts WHERE account_no = ?1 AND id != ?2",
                params![no, input.id.unwrap_or(-1)],
                |r| r.get(0),
            )
            .optional()?;
        if no_dup_id.is_some() {
            return Err(AppError::General(format!("账号 {no} 已被其他资金账户使用")));
        }
    }

    let id = match input.id {
        Some(id) => {
            let existing = existing.as_ref().expect("更新路径已有原记录");
            if existing.account_type != input.account_type && fund_account_referenced(&tx, id)? {
                return Err(AppError::General(format!(
                    "资金账户 {} 已被凭证分录/银行流水/付款批次引用，不能修改账户类型",
                    existing.account_code
                )));
            }
            if is_default {
                clear_same_type_default(&tx, &input.account_type, Some(id))?;
            }
            tx.execute(
                "UPDATE fund_accounts SET account_code = ?2, name = ?3, account_type = ?4,
                 bank_name = ?5, account_no = ?6, gl_account_code = ?7, opening_date = ?8,
                 opening_balance = ?9, is_default = ?10, is_active = ?11, remark = ?12, updated_at = ?13
                 WHERE id = ?1",
                params![
                    id,
                    account_code,
                    name,
                    input.account_type,
                    bank_name,
                    account_no,
                    gl_account_code,
                    opening_date,
                    opening_balance,
                    is_default as i64,
                    is_active as i64,
                    remark,
                    Utc::now().to_rfc3339()
                ],
            )?;
            id
        }
        None => {
            if is_default {
                clear_same_type_default(&tx, &input.account_type, None)?;
            }
            tx.execute(
                "INSERT INTO fund_accounts (account_code, name, account_type, bank_name, account_no,
                 currency, gl_account_code, opening_date, opening_balance, is_default, is_active, remark, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'CNY', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
                params![
                    account_code,
                    name,
                    input.account_type,
                    bank_name,
                    account_no,
                    gl_account_code,
                    opening_date,
                    opening_balance,
                    is_default as i64,
                    is_active as i64,
                    remark,
                    Utc::now().to_rfc3339()
                ],
            )?;
            tx.last_insert_rowid()
        }
    };
    tx.commit()?;
    get_fund_account(conn, id)
}

/// 启用/停用资金账户。默认账户不允许停用（须先把同类型其他账户设为默认）；
/// 已被业务引用的账户允许停用（spec 4.1：存在引用只允许停用，不删除）。
pub fn set_active_fund_account(conn: &Connection, id: i64, active: bool) -> AppResult<FundAccount> {
    let existing = get_fund_account(conn, id)?;
    if existing.is_active == active {
        return Ok(existing);
    }
    if !active && existing.is_default {
        return Err(AppError::General(format!(
            "资金账户 {} 是{}类默认账户，不能停用；请先将同类型其他账户设为默认账户",
            existing.account_code, existing.account_type
        )));
    }
    conn.execute(
        "UPDATE fund_accounts SET is_active = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, active as i64, Utc::now().to_rfc3339()],
    )?;
    get_fund_account(conn, id)
}

/// 统计资金账户是否被凭证分录/银行流水/付款批次引用（引用保护依据）
fn fund_account_referenced(conn: &Connection, id: i64) -> AppResult<bool> {
    let used: i64 = conn.query_row(
        "SELECT
           (SELECT COUNT(*) FROM voucher_lines WHERE fund_account_id = ?1) +
           (SELECT COUNT(*) FROM bank_transactions WHERE fund_account_id = ?1) +
           (SELECT COUNT(*) FROM payment_batches WHERE fund_account_id = ?1)",
        params![id],
        |r| r.get(0),
    )?;
    Ok(used > 0)
}

/// 同类型默认账户唯一：把该类型其它账户的 is_default 清零（须在写默认账户前执行）
fn clear_same_type_default(
    conn: &Connection,
    account_type: &str,
    exclude_id: Option<i64>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE fund_accounts SET is_default = 0 WHERE account_type = ?1 AND id != ?2",
        params![account_type, exclude_id.unwrap_or(-1)],
    )?;
    Ok(())
}

// ==================== 往来单位 ====================

const PARTNER_COLS: &str = "id, partner_code, name, partner_type, tax_id, contact_person, phone, bank_name, bank_account, gl_account_code, status, remark, created_at, updated_at";

fn partner_from_row(r: &rusqlite::Row) -> rusqlite::Result<BusinessPartner> {
    Ok(BusinessPartner {
        id: r.get(0)?,
        partner_code: r.get(1)?,
        name: r.get(2)?,
        partner_type: r.get(3)?,
        tax_id: r.get(4)?,
        contact_person: r.get(5)?,
        phone: r.get(6)?,
        bank_name: r.get(7)?,
        bank_account: r.get(8)?,
        gl_account_code: r.get(9)?,
        status: r.get(10)?,
        remark: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
    })
}

/// 按条件查询往来单位（类型/状态/关键字），按编码排序
pub fn get_business_partners(
    conn: &Connection,
    q: &BusinessPartnerQuery,
) -> AppResult<Vec<BusinessPartner>> {
    let mut where_clauses: Vec<String> = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;
    if let Some(t) = q
        .partner_type
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        where_clauses.push(format!("partner_type = ?{idx}"));
        params_vec.push(Box::new(t.to_string()));
        idx += 1;
    }
    if let Some(s) = q.status.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        where_clauses.push(format!("status = ?{idx}"));
        params_vec.push(Box::new(s.to_string()));
        idx += 1;
    }
    if let Some(kw) = q
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        where_clauses.push(format!(
            "(partner_code LIKE ?{idx} OR name LIKE ?{idx} OR COALESCE(tax_id,'') LIKE ?{idx} OR COALESCE(contact_person,'') LIKE ?{idx})"
        ));
        params_vec.push(Box::new(format!("%{kw}%")));
    }
    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_clauses.join(" AND "))
    };
    let sql =
        format!("SELECT {PARTNER_COLS} FROM business_partners{where_sql} ORDER BY partner_code");
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), partner_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn get_business_partner(conn: &Connection, id: i64) -> AppResult<BusinessPartner> {
    conn.query_row(
        &format!("SELECT {PARTNER_COLS} FROM business_partners WHERE id = ?1"),
        params![id],
        partner_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("往来单位不存在：id={id}")))
}

/// 保存往来单位（id=Some 更新，否则新增）。
///
/// 校验：编码/名称非空、类型与状态合法、默认科目（可空）必须存在、
/// 编码唯一、名称+税号唯一（税号空按空串归一化）。
pub fn save_business_partner(
    conn: &Connection,
    input: &BusinessPartnerInput,
) -> AppResult<BusinessPartner> {
    let partner_code = input.partner_code.trim();
    let name = input.name.trim();
    if partner_code.is_empty() {
        return Err(AppError::InvalidParam("往来单位编码不能为空".into()));
    }
    if name.is_empty() {
        return Err(AppError::InvalidParam("往来单位名称不能为空".into()));
    }
    ensure_in_list(&input.partner_type, PARTNER_TYPES, "往来单位类型")?;
    let existing = match input.id {
        Some(id) => Some(get_business_partner(conn, id)?),
        None => None,
    };
    let status = input.status.clone().unwrap_or_else(|| {
        existing
            .as_ref()
            .map(|e| e.status.clone())
            .unwrap_or_else(|| "active".into())
    });
    ensure_in_list(&status, PARTNER_STATUSES, "往来单位状态")?;
    let tax_id = resolve_optional(
        &input.tax_id,
        existing.as_ref().and_then(|e| e.tax_id.clone()),
    );
    let gl_account_code = resolve_optional(
        &input.gl_account_code,
        existing.as_ref().and_then(|e| e.gl_account_code.clone()),
    );
    if let Some(code) = &gl_account_code {
        ensure_gl_account_exists(conn, code)?;
    }

    let tx = conn.unchecked_transaction()?;
    let self_id = input.id.unwrap_or(-1);
    let code_dup_id: Option<i64> = tx
        .query_row(
            "SELECT id FROM business_partners WHERE partner_code = ?1 AND id != ?2",
            params![partner_code, self_id],
            |r| r.get(0),
        )
        .optional()?;
    if code_dup_id.is_some() {
        return Err(AppError::General(format!(
            "往来单位编码 {partner_code} 已存在"
        )));
    }
    let name_tax_dup_id: Option<i64> = tx
        .query_row(
            "SELECT id FROM business_partners WHERE name = ?1 AND COALESCE(tax_id, '') = COALESCE(?2, '') AND id != ?3",
            params![name, tax_id, self_id],
            |r| r.get(0),
        )
        .optional()?;
    if name_tax_dup_id.is_some() {
        let tax_desc = tax_id.as_deref().unwrap_or("无税号");
        return Err(AppError::General(format!(
            "往来单位「{name}」（{tax_desc}）已存在"
        )));
    }

    let now = Utc::now().to_rfc3339();
    let prev = existing.as_ref();
    let id = match input.id {
        Some(id) => {
            tx.execute(
                "UPDATE business_partners SET partner_code = ?2, name = ?3, partner_type = ?4,
                 tax_id = ?5, contact_person = ?6, phone = ?7, bank_name = ?8, bank_account = ?9,
                 gl_account_code = ?10, status = ?11, remark = ?12, updated_at = ?13
                 WHERE id = ?1",
                params![
                    id,
                    partner_code,
                    name,
                    input.partner_type,
                    tax_id,
                    resolve_optional(
                        &input.contact_person,
                        prev.and_then(|e| e.contact_person.clone())
                    ),
                    resolve_optional(&input.phone, prev.and_then(|e| e.phone.clone())),
                    resolve_optional(&input.bank_name, prev.and_then(|e| e.bank_name.clone())),
                    resolve_optional(
                        &input.bank_account,
                        prev.and_then(|e| e.bank_account.clone())
                    ),
                    gl_account_code,
                    status,
                    resolve_optional(&input.remark, prev.and_then(|e| e.remark.clone())),
                    now
                ],
            )?;
            id
        }
        None => {
            tx.execute(
                "INSERT INTO business_partners (partner_code, name, partner_type, tax_id, contact_person,
                 phone, bank_name, bank_account, gl_account_code, status, remark, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
                params![
                    partner_code,
                    name,
                    input.partner_type,
                    tax_id,
                    resolve_optional(&input.contact_person, None),
                    resolve_optional(&input.phone, None),
                    resolve_optional(&input.bank_name, None),
                    resolve_optional(&input.bank_account, None),
                    gl_account_code,
                    status,
                    resolve_optional(&input.remark, None),
                    now
                ],
            )?;
            tx.last_insert_rowid()
        }
    };
    tx.commit()?;
    get_business_partner(conn, id)
}

/// 启用/停用往来单位（改 status）。
/// 引用保护（被资金单引用后只允许停用）自 Task 6 引入 fund_documents 后生效。
pub fn set_active_business_partner(
    conn: &Connection,
    id: i64,
    active: bool,
) -> AppResult<BusinessPartner> {
    let existing = get_business_partner(conn, id)?;
    let target = if active { "active" } else { "inactive" };
    if existing.status == target {
        return Ok(existing);
    }
    conn.execute(
        "UPDATE business_partners SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, target, Utc::now().to_rfc3339()],
    )?;
    get_business_partner(conn, id)
}

// ==================== 通用校验 helper ====================

/// 校验取值在合法枚举内
fn ensure_in_list(value: &str, allowed: &[&str], label: &str) -> AppResult<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(AppError::InvalidParam(format!(
            "{label}无效：{value}（允许：{}）",
            allowed.join(" / ")
        )))
    }
}

/// 校验会计科目存在
fn ensure_gl_account_exists(conn: &Connection, code: &str) -> AppResult<()> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM gl_accounts WHERE code = ?1",
        params![code],
        |r| r.get(0),
    )?;
    if count == 0 {
        return Err(AppError::InvalidParam(format!("会计科目 {code} 不存在")));
    }
    Ok(())
}

/// patch 语义的可空字符串解析：`Some(v)` 去空格后生效（空串=清空为 NULL），`None` 保留原值
fn resolve_optional(input: &Option<String>, existing: Option<String>) -> Option<String> {
    match input {
        Some(v) => {
            let t = v.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        None => existing,
    }
}

// ==================== 业务附件（通用加密附件底座） ====================

/// 附件可挂接的实体类型（与 approval_events 实体维度一致：报销单 / 资金单据）。
/// 本任务仅校验枚举；实体存在性校验：reimbursement_claim 可直接查表（见删除门禁），
/// fund_document 由 Task 6（7B 资金单据）建表后补充存在性与状态校验。
pub(crate) const ATTACHMENT_ENTITY_TYPES: &[&str] = &["fund_document", "reimbursement_claim"];

/// 附件大小上限 20MB。spec 未明示；参照常见扫描件/合同 PDF 大小设定，
/// 并避免附件随备份整树打包后备份包体积失控。
pub(crate) const ATTACHMENT_MAX_FILE_SIZE: i64 = 20 * 1024 * 1024;

/// 报销单附件允许删除/变更的状态（spec 4.6 + 第 8 节：删除仅允许未提交实体；
/// 已提交附件只允许通过反审批后变更）。draft=未提交、rejected=已驳回、void=已作废。
const REIMBURSEMENT_ATTACHMENT_EDITABLE_STATUSES: &[&str] = &["draft", "rejected", "void"];

fn validate_attachment_entity_type(entity_type: &str) -> AppResult<()> {
    if !ATTACHMENT_ENTITY_TYPES.contains(&entity_type) {
        return Err(AppError::InvalidParam(format!(
            "不支持的附件实体类型: {entity_type}（允许: {}）",
            ATTACHMENT_ENTITY_TYPES.join("/")
        )));
    }
    Ok(())
}

/// 附件归档目录：`{app_data_dir}/attachments/{entity_type}/{belong_month}/`（spec 4.6）。
/// belong_month 缺失或含非法字符时回退 `unclassified`（与发票归档同规则）。
fn attachment_archive_dir(
    app_data_dir: &std::path::Path,
    entity_type: &str,
    belong_month: Option<&str>,
) -> std::path::PathBuf {
    let raw_month = belong_month.unwrap_or("unclassified");
    let sanitized: String = raw_month
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let month = if sanitized.is_empty() {
        "unclassified"
    } else {
        sanitized.as_str()
    };
    app_data_dir
        .join("attachments")
        .join(entity_type)
        .join(month)
}

/// 上传（登记）业务附件：复制源文件到 attachments/ 归档目录 → DEK 已加载时就地加密
/// （先写 `.enc.tmp` 再原子 rename，与发票图片同原语、同密文格式）→ 写 business_attachments。
/// 归档/加密/落库任一步失败都补偿清理已产生的文件，不留孤儿文件。
///
/// 入参 `file_path` 为源文件绝对路径（前端文件对话框选取）；`file_name` 为空时取源文件名；
/// `encrypted`/`file_size`/`uploaded_by` 由后端裁决（覆盖入参值）。
///
/// TODO(Task 6): fund_documents 建表后，在此补充实体存在性校验（当前仅校验实体类型枚举）。
pub fn add_business_attachment(
    conn: &Connection,
    sec: &crate::security::SecurityState,
    current: &CurrentOperatorState,
    app_data_dir: &std::path::Path,
    input: &BusinessAttachmentInput,
) -> AppResult<BusinessAttachment> {
    add_business_attachment_impl(conn, sec, current, app_data_dir, input, Utc::now())
}

/// impl 入口把"归档时间"参数化：文件名时间戳与 created_at 同源，且测试可注入固定时间
/// 得到可预测的目标路径（补偿清理测试依赖此能力）。
pub(crate) fn add_business_attachment_impl(
    conn: &Connection,
    sec: &crate::security::SecurityState,
    current: &CurrentOperatorState,
    app_data_dir: &std::path::Path,
    input: &BusinessAttachmentInput,
    now: chrono::DateTime<Utc>,
) -> AppResult<BusinessAttachment> {
    let (_, operator_name) = require_current_operator(conn, current)?;
    validate_attachment_entity_type(&input.entity_type)?;
    if input.entity_id <= 0 {
        return Err(AppError::InvalidParam(
            "附件实体 ID 非法（必须为正整数）".into(),
        ));
    }
    let src = std::path::Path::new(input.file_path.trim());
    if !src.is_file() {
        return Err(AppError::InvalidParam(format!(
            "源文件不存在: {}",
            input.file_path
        )));
    }
    let file_size = std::fs::metadata(src)?.len() as i64;
    if file_size > ATTACHMENT_MAX_FILE_SIZE {
        return Err(AppError::InvalidParam(format!(
            "附件超过大小限制（最大 {}MB）",
            ATTACHMENT_MAX_FILE_SIZE / 1024 / 1024
        )));
    }

    // 原文件名：入参 file_name 为空时取源文件名，统一净化路径分隔符与特殊字符
    let raw_name = if input.file_name.trim().is_empty() {
        src.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "attachment.bin".to_string())
    } else {
        input.file_name.trim().to_string()
    };
    let sanitized = crate::invoice::sanitize_archive_filename(&raw_name);
    let file_name = if sanitized.is_empty() {
        "attachment.bin".to_string()
    } else {
        sanitized
    };

    let dir = attachment_archive_dir(
        app_data_dir,
        &input.entity_type,
        input.belong_month.as_deref(),
    );
    std::fs::create_dir_all(&dir)?;
    let mut target_path = dir.join(format!("{}_{}", now.format("%Y%m%d%H%M%S"), file_name));
    // 同秒同名极小概率冲突（file_path UNIQUE）：存在即追加序号，既避免覆盖也避免落库冲突
    let mut seq = 0u32;
    while target_path.exists() {
        seq += 1;
        target_path = dir.join(format!(
            "{}_{seq}_{}",
            now.format("%Y%m%d%H%M%S"),
            file_name
        ));
    }

    // 归档 + 加密：任一步失败补偿清理，不留半成品/孤儿文件
    let encrypt_result = (|| -> AppResult<i64> {
        std::fs::copy(src, &target_path)?;
        match sec.dek() {
            Some(dek) => {
                crate::security::encrypt_file_in_place(&target_path, &dek)?;
                Ok(1)
            }
            None => Ok(0), // DEK 未加载：明文归档（与发票归档同语义，encrypted=0 如实记录）
        }
    })();
    let encrypted = match encrypt_result {
        Ok(flag) => flag,
        Err(e) => {
            let _ = std::fs::remove_file(&target_path);
            let _ = std::fs::remove_file(target_path.with_extension("enc.tmp"));
            return Err(e);
        }
    };

    let created_at = now.to_rfc3339();
    let insert = conn.execute(
        "INSERT INTO business_attachments
            (entity_type, entity_id, file_name, file_path, encrypted, file_size, belong_month, uploaded_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            input.entity_type,
            input.entity_id,
            file_name,
            target_path.to_string_lossy(),
            encrypted,
            file_size,
            input.belong_month,
            operator_name,
            created_at,
        ],
    );
    if let Err(e) = insert {
        // 补偿清理：DB 写入失败时删除已归档文件，保持 DB 与磁盘一致
        let _ = std::fs::remove_file(&target_path);
        let _ = std::fs::remove_file(target_path.with_extension("enc.tmp"));
        return Err(e.into());
    }

    Ok(BusinessAttachment {
        id: conn.last_insert_rowid(),
        entity_type: input.entity_type.clone(),
        entity_id: input.entity_id,
        file_name,
        file_path: target_path.to_string_lossy().to_string(),
        encrypted: encrypted != 0,
        file_size: Some(file_size),
        belong_month: input.belong_month.clone(),
        uploaded_by: Some(operator_name),
        created_at,
    })
}

/// 按实体列出附件（id 升序）。
pub fn list_business_attachments(
    conn: &Connection,
    entity_type: &str,
    entity_id: i64,
) -> AppResult<Vec<BusinessAttachment>> {
    validate_attachment_entity_type(entity_type)?;
    let mut stmt = conn.prepare(
        "SELECT id, entity_type, entity_id, file_name, file_path, encrypted, file_size,
                belong_month, uploaded_by, created_at
         FROM business_attachments
         WHERE entity_type = ?1 AND entity_id = ?2
         ORDER BY id",
    )?;
    let rows = stmt.query_map(params![entity_type, entity_id], map_attachment_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 按主键取单个附件（删除门禁与预览命令共用）。
pub(crate) fn get_business_attachment(conn: &Connection, id: i64) -> AppResult<BusinessAttachment> {
    conn.query_row(
        "SELECT id, entity_type, entity_id, file_name, file_path, encrypted, file_size,
                belong_month, uploaded_by, created_at
         FROM business_attachments WHERE id = ?1",
        params![id],
        map_attachment_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("附件ID={id}未找到")))
}

fn map_attachment_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BusinessAttachment> {
    Ok(BusinessAttachment {
        id: row.get(0)?,
        entity_type: row.get(1)?,
        entity_id: row.get(2)?,
        file_name: row.get(3)?,
        file_path: row.get(4)?,
        encrypted: row.get::<_, i64>(5)? != 0,
        file_size: row.get(6)?,
        belong_month: row.get(7)?,
        uploaded_by: row.get(8)?,
        created_at: row.get(9)?,
    })
}

/// 删除业务附件：先删磁盘文件（失败即中止，DB 行保留可重试），再删 DB 行。
/// 实体状态门禁（spec：删除仅允许未提交实体；已提交附件须反审批后变更）：
/// - reimbursement_claim：查 reimbursement_claims.status，仅未提交/已驳回/已作废可删；
///   记录已不存在视为孤儿附件，允许清理。
/// - fund_document：fund_documents 表由 Task 6 创建，届时补充状态校验（当前放行）。
/// 返回被删除的原文件名（供命令层写审计）。
pub fn delete_business_attachment(
    conn: &Connection,
    current: &CurrentOperatorState,
    id: i64,
) -> AppResult<String> {
    require_current_operator(conn, current)?;
    let att = get_business_attachment(conn, id)?;
    ensure_attachment_entity_editable(conn, &att)?;

    let path = std::path::Path::new(&att.file_path);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    conn.execute(
        "DELETE FROM business_attachments WHERE id = ?1",
        params![id],
    )?;
    Ok(att.file_name)
}

/// 实体提交状态门禁：已提交（待审批/已审批）实体的附件禁止删除。
fn ensure_attachment_entity_editable(conn: &Connection, att: &BusinessAttachment) -> AppResult<()> {
    match att.entity_type.as_str() {
        "reimbursement_claim" => {
            let status: Option<String> = conn
                .query_row(
                    "SELECT status FROM reimbursement_claims WHERE id = ?1",
                    params![att.entity_id],
                    |r| r.get(0),
                )
                .optional()?;
            match status {
                Some(s) if REIMBURSEMENT_ATTACHMENT_EDITABLE_STATUSES.contains(&s.as_str()) => {
                    Ok(())
                }
                Some(s) => Err(AppError::General(format!(
                    "报销单已提交审批（状态: {s}），附件须先反审批/驳回后才能删除"
                ))),
                None => Ok(()), // 实体记录已不存在：允许清理孤儿附件
            }
        }
        // TODO(Task 6): fund_documents 建表后，按单据状态拦截已提交单据的附件删除
        _ => Ok(()),
    }
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::setup_financial_db;

    // ---------- 测试数据构造 ----------

    fn bank_input(code: &str, name: &str, account_no: &str) -> FundAccountInput {
        FundAccountInput {
            id: None,
            account_code: code.into(),
            name: name.into(),
            account_type: "bank".into(),
            bank_name: Some("工商银行".into()),
            account_no: Some(account_no.into()),
            gl_account_code: "1002".into(),
            opening_date: Some("2026-08-01".into()),
            opening_balance: Some(1000.0),
            is_default: Some(false),
            is_active: Some(true),
            remark: None,
        }
    }

    fn cash_input(code: &str, name: &str) -> FundAccountInput {
        FundAccountInput {
            account_type: "cash".into(),
            bank_name: None,
            account_no: None,
            gl_account_code: "1001".into(),
            opening_balance: Some(500.0),
            opening_date: None,
            ..bank_input(code, name, "")
        }
    }

    fn partner_input(code: &str, name: &str, tax: Option<&str>) -> BusinessPartnerInput {
        BusinessPartnerInput {
            id: None,
            partner_code: code.into(),
            name: name.into(),
            partner_type: "supplier".into(),
            tax_id: tax.map(str::to_string),
            contact_person: None,
            phone: None,
            bank_name: None,
            bank_account: None,
            gl_account_code: None,
            status: None,
            remark: None,
        }
    }

    fn operator_input(name: &str, role: &str) -> OperatorProfileInput {
        OperatorProfileInput {
            id: None,
            name: name.into(),
            role: role.into(),
            is_active: Some(true),
            remark: None,
        }
    }

    // ---------- 资金账户 ----------

    #[test]
    fn test_fund_account_create_query_and_filters() {
        let conn = setup_financial_db();
        let bank =
            save_fund_account(&conn, &bank_input("BANK-001", "基本户", "622200001")).unwrap();
        assert!(bank.id > 0);
        assert_eq!(bank.currency, "CNY");
        assert!(bank.is_active);
        assert!(!bank.is_default);
        save_fund_account(&conn, &cash_input("CASH-001", "现金库")).unwrap();

        let all = get_fund_accounts(&conn, &FundAccountQuery::default()).unwrap();
        assert_eq!(all.len(), 2);
        let by_type = get_fund_accounts(
            &conn,
            &FundAccountQuery {
                account_type: Some("bank".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_type.len(), 1);
        assert_eq!(by_type[0].account_code, "BANK-001");
        let by_kw = get_fund_accounts(
            &conn,
            &FundAccountQuery {
                keyword: Some("基本".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_kw.len(), 1);
        let by_active = get_fund_accounts(
            &conn,
            &FundAccountQuery {
                is_active: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(by_active.is_empty());
    }

    #[test]
    fn test_fund_account_duplicate_code_and_account_no_blocked() {
        let conn = setup_financial_db();
        save_fund_account(&conn, &bank_input("BANK-001", "基本户", "622200001")).unwrap();

        let err = save_fund_account(&conn, &bank_input("BANK-001", "另一个户", "622200002"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("已存在"), "编码重复应拦截：{err}");

        let err = save_fund_account(&conn, &bank_input("BANK-002", "一般户", "622200001"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("账号"), "账号重复应拦截：{err}");

        // 更新时保留自身编码/账号不误报
        let mut input = bank_input("BANK-001", "基本户改名", "622200001");
        input.id = Some(1);
        let updated = save_fund_account(&conn, &input).unwrap();
        assert_eq!(updated.name, "基本户改名");
        assert_eq!(updated.account_no.as_deref(), Some("622200001"));
    }

    #[test]
    fn test_fund_account_field_validation() {
        let conn = setup_financial_db();

        // 类型非法
        let mut bad_type = bank_input("BANK-X", "类型错", "622209");
        bad_type.account_type = "wallet".into();
        assert!(save_fund_account(&conn, &bad_type)
            .unwrap_err()
            .to_string()
            .contains("资金账户类型"));

        // 挂接科目必须属于资金科目
        let mut bad_gl = bank_input("BANK-X", "科目错", "622209");
        bad_gl.gl_account_code = "6602".into();
        assert!(save_fund_account(&conn, &bad_gl)
            .unwrap_err()
            .to_string()
            .contains("资金科目"));

        // 挂接科目必须存在（先移除 1012 科目模拟科目缺失；9999 会被资金科目白名单先拦截）
        conn.execute("DELETE FROM gl_accounts WHERE code = '1012'", [])
            .unwrap();
        let mut missing_gl = bank_input("BANK-X", "科目缺失", "622209");
        missing_gl.gl_account_code = "1012".into();
        assert!(save_fund_account(&conn, &missing_gl)
            .unwrap_err()
            .to_string()
            .contains("不存在"));

        // 期初余额为负
        let mut negative = bank_input("BANK-X", "负余额", "622209");
        negative.opening_balance = Some(-10.0);
        assert!(save_fund_account(&conn, &negative)
            .unwrap_err()
            .to_string()
            .contains("期初余额"));

        // 启用日期格式
        let mut bad_date = bank_input("BANK-X", "日期错", "622209");
        bad_date.opening_date = Some("2026/08/01".into());
        assert!(save_fund_account(&conn, &bad_date)
            .unwrap_err()
            .to_string()
            .contains("YYYY-MM-DD"));

        // 编码/名称必填
        let mut blank = bank_input("  ", "空编码", "622209");
        assert!(save_fund_account(&conn, &blank).is_err());
        blank.account_code = "BANK-X".into();
        blank.name = "  ".into();
        assert!(save_fund_account(&conn, &blank).is_err());

        // 默认与停用互斥
        let mut both = bank_input("BANK-X", "互斥", "622209");
        both.is_default = Some(true);
        both.is_active = Some(false);
        assert!(save_fund_account(&conn, &both)
            .unwrap_err()
            .to_string()
            .contains("默认账户"));
    }

    #[test]
    fn test_fund_account_default_unique_per_type_in_transaction() {
        let conn = setup_financial_db();
        let mut a = bank_input("BANK-001", "基本户", "622200001");
        a.is_default = Some(true);
        let a = save_fund_account(&conn, &a).unwrap();
        let mut b = bank_input("BANK-002", "一般户", "622200002");
        b.is_default = Some(true);
        let b = save_fund_account(&conn, &b).unwrap();

        // 同类型切换默认后仍只有一个默认（partial unique index 未被触发）
        let defaults = get_fund_accounts(
            &conn,
            &FundAccountQuery {
                account_type: Some("bank".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .into_iter()
        .filter(|acc| acc.is_default)
        .count();
        assert_eq!(defaults, 1, "同类型只能有一个默认账户");
        assert!(get_fund_account(&conn, b.id).unwrap().is_default);
        assert!(!get_fund_account(&conn, a.id).unwrap().is_default);

        // 不同类型各自可拥有默认账户
        let mut cash = cash_input("CASH-001", "现金库");
        cash.is_default = Some(true);
        save_fund_account(&conn, &cash).unwrap();
        let cash_default = get_fund_accounts(
            &conn,
            &FundAccountQuery {
                account_type: Some("cash".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(cash_default[0].is_default);
    }

    #[test]
    fn test_fund_account_update_default_switch_and_patch() {
        let conn = setup_financial_db();
        let mut a = bank_input("BANK-001", "基本户", "622200001");
        a.is_default = Some(true);
        let a = save_fund_account(&conn, &a).unwrap();
        let b = save_fund_account(&conn, &bank_input("BANK-002", "一般户", "622200002")).unwrap();

        // 通过更新把默认切换到 b（更新路径同样在事务中清理旧默认）
        let mut switch = bank_input("BANK-002", "一般户", "622200002");
        switch.id = Some(b.id);
        switch.is_default = Some(true);
        save_fund_account(&conn, &switch).unwrap();

        assert!(!get_fund_account(&conn, a.id).unwrap().is_default);
        assert!(get_fund_account(&conn, b.id).unwrap().is_default);

        // patch 语义：仅改名称时，默认/启用/期初余额均保留
        let mut rename = bank_input("BANK-001", "基本户改名", "622200001");
        rename.id = Some(a.id);
        rename.is_default = None;
        rename.is_active = None;
        rename.opening_balance = None;
        let renamed = save_fund_account(&conn, &rename).unwrap();
        assert_eq!(renamed.name, "基本户改名");
        assert!(!renamed.is_default);
        assert!(renamed.is_active);
        assert_eq!(renamed.opening_balance, 1000.0);
    }

    #[test]
    fn test_fund_account_cannot_deactivate_default() {
        let conn = setup_financial_db();
        let mut a = bank_input("BANK-001", "基本户", "622200001");
        a.is_default = Some(true);
        let a = save_fund_account(&conn, &a).unwrap();

        // 唯一账户且为默认：停用拦截
        let err = set_active_fund_account(&conn, a.id, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("默认账户"), "默认账户停用应拦截：{err}");

        // 切换默认到 b 后，a 可停用
        let b = save_fund_account(&conn, &bank_input("BANK-002", "一般户", "622200002")).unwrap();
        let mut switch = bank_input("BANK-002", "一般户", "622200002");
        switch.id = Some(b.id);
        switch.is_default = Some(true);
        save_fund_account(&conn, &switch).unwrap();
        let a = set_active_fund_account(&conn, a.id, false).unwrap();
        assert!(!a.is_active);

        // 重复停用幂等
        let again = set_active_fund_account(&conn, a.id, false).unwrap();
        assert!(!again.is_active);
    }

    #[test]
    fn test_fund_account_type_change_blocked_when_referenced() {
        let conn = setup_financial_db();
        let bank =
            save_fund_account(&conn, &bank_input("BANK-001", "基本户", "622200001")).unwrap();
        let unreferenced =
            save_fund_account(&conn, &bank_input("BANK-002", "一般户", "622200002")).unwrap();

        // 制造一条资金辅助分录引用 bank 账户
        conn.execute_batch(&format!(
            "INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status, created_at, updated_at)
             VALUES ('V-TEST-1', '2026-08-01', '2026-08', 'bank_manual', 999, 100, 'active', '2026-08-01', '2026-08-01');
             INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount, fund_account_id, line_order)
             VALUES (1, '1002', 100, 0, {}, 1);",
            bank.id
        ))
        .unwrap();

        // 被引用账户修改类型被拦截
        let mut change_type = bank_input("BANK-001", "基本户", "622200001");
        change_type.id = Some(bank.id);
        change_type.account_type = "cash".into();
        let err = save_fund_account(&conn, &change_type)
            .unwrap_err()
            .to_string();
        assert!(err.contains("账户类型"), "被引用账户改类型应拦截：{err}");

        // 未被引用账户可修改类型
        let mut change_ok = bank_input("BANK-002", "一般户", "622200002");
        change_ok.id = Some(unreferenced.id);
        change_ok.account_type = "third_party".into();
        let changed = save_fund_account(&conn, &change_ok).unwrap();
        assert_eq!(changed.account_type, "third_party");
    }

    // ---------- 往来单位 ----------

    #[test]
    fn test_business_partner_crud_and_unique() {
        let conn = setup_financial_db();
        let p = save_business_partner(
            &conn,
            &partner_input("GYS-001", "供应商甲", Some("91110000X")),
        )
        .unwrap();
        assert!(p.id > 0);
        assert_eq!(p.status, "active");

        // 编码重复
        let err = save_business_partner(&conn, &partner_input("GYS-001", "供应商乙", None))
            .unwrap_err()
            .to_string();
        assert!(err.contains("编码"), "编码重复应拦截：{err}");

        // 名称 + 税号重复
        let err = save_business_partner(
            &conn,
            &partner_input("GYS-002", "供应商甲", Some("91110000X")),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("已存在"), "名称+税号重复应拦截：{err}");

        // 同名不同税号允许；同名无税号与同名有税号允许
        save_business_partner(
            &conn,
            &partner_input("GYS-003", "供应商甲", Some("91110000Y")),
        )
        .unwrap();
        save_business_partner(&conn, &partner_input("GYS-004", "供应商甲", None)).unwrap();
        // 同名且都无税号：冲突
        let err = save_business_partner(&conn, &partner_input("GYS-005", "供应商甲", None))
            .unwrap_err()
            .to_string();
        assert!(err.contains("已存在"), "同名无税号重复应拦截：{err}");

        // 类型/状态非法
        let mut bad_type = partner_input("GYS-006", "类型错", None);
        bad_type.partner_type = "employee".into();
        assert!(save_business_partner(&conn, &bad_type)
            .unwrap_err()
            .to_string()
            .contains("往来单位类型"));
        let mut bad_status = partner_input("GYS-006", "状态错", None);
        bad_status.status = Some("frozen".into());
        assert!(save_business_partner(&conn, &bad_status)
            .unwrap_err()
            .to_string()
            .contains("往来单位状态"));

        // 默认科目可空；填写时必须存在
        let mut bad_gl = partner_input("GYS-006", "科目错", None);
        bad_gl.gl_account_code = Some("9999".into());
        assert!(save_business_partner(&conn, &bad_gl).is_err());
        let mut ok_gl = partner_input("GYS-006", "科目对", None);
        ok_gl.gl_account_code = Some("2202".into());
        let with_gl = save_business_partner(&conn, &ok_gl).unwrap();
        assert_eq!(with_gl.gl_account_code.as_deref(), Some("2202"));

        // 停用/启用
        let inactive = set_active_business_partner(&conn, p.id, false).unwrap();
        assert_eq!(inactive.status, "inactive");
        // patch 语义：更新不传 status 时保留 inactive，不会被误改回 active
        let mut keep_status = partner_input("GYS-001", "供应商甲", Some("91110000X"));
        keep_status.id = Some(p.id);
        keep_status.status = None;
        let kept = save_business_partner(&conn, &keep_status).unwrap();
        assert_eq!(kept.status, "inactive");
        let active = set_active_business_partner(&conn, p.id, true).unwrap();
        assert_eq!(active.status, "active");

        // 关键字查询
        let found = get_business_partners(
            &conn,
            &BusinessPartnerQuery {
                keyword: Some("供应商甲".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(found.len(), 3);
    }

    // ---------- 操作人 ----------

    #[test]
    fn test_operator_last_active_guard() {
        let conn = setup_financial_db();
        let current = CurrentOperatorState::new();
        let a = save_operator_profile(&conn, &current, &operator_input("张会计", "cashier"))
            .unwrap()
            .0;

        // 停用唯一启用操作人：拦截
        let err = set_active_operator_profile(&conn, &current, a.id, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("至少"), "最后一名启用操作人停用应拦截：{err}");

        // 通过 save 路径停用同样拦截
        let mut deactivate = operator_input("张会计", "cashier");
        deactivate.id = Some(a.id);
        deactivate.is_active = Some(false);
        let err = save_operator_profile(&conn, &current, &deactivate)
            .unwrap_err()
            .to_string();
        assert!(err.contains("至少"), "save 路径停用最后操作人应拦截：{err}");

        // patch 语义：save 不传 is_active 时保留启用
        let mut rename = operator_input("张会计改", "cashier");
        rename.id = Some(a.id);
        rename.is_active = None;
        let renamed = save_operator_profile(&conn, &current, &rename).unwrap().0;
        assert!(renamed.is_active);

        // 新增第二名操作人后可停用第一名
        let b = save_operator_profile(&conn, &current, &operator_input("李出纳", "approver"))
            .unwrap()
            .0;
        let a = set_active_operator_profile(&conn, &current, a.id, false)
            .unwrap()
            .0;
        assert!(!a.is_active);

        // 再停用最后一名（b）被拦截：b 仍处于启用状态
        let err = set_active_operator_profile(&conn, &current, b.id, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("至少"));

        // 兜底不变量：库里没有任何启用操作人时（如历史数据/直接改库），
        // 新增停用状态操作人同样被拦截；新增启用操作人可恢复系统可用
        conn.execute("UPDATE operator_profiles SET is_active = 0", [])
            .unwrap();
        let mut inactive_new = operator_input("王实习", "requester");
        inactive_new.is_active = Some(false);
        assert!(save_operator_profile(&conn, &current, &inactive_new).is_err());
        assert!(save_operator_profile(&conn, &current, &operator_input("王主管", "admin")).is_ok());

        // 角色非法
        let bad_role = operator_input("赵无", "boss");
        assert!(save_operator_profile(&conn, &current, &bad_role)
            .unwrap_err()
            .to_string()
            .contains("操作人角色"));
    }

    #[test]
    fn test_current_operator_session() {
        let conn = setup_financial_db();
        let current = CurrentOperatorState::new();

        // 未选择：require 报错，get 返回 None
        assert!(require_current_operator(&conn, &current).is_err());
        assert!(get_current_operator(&conn, &current).unwrap().is_none());
        assert_eq!(current_operator_name(&conn, &current), "system");

        let a = save_operator_profile(&conn, &current, &operator_input("张会计", "cashier"))
            .unwrap()
            .0;
        let b = save_operator_profile(&conn, &current, &operator_input("李出纳", "approver"))
            .unwrap()
            .0;

        // 停用操作人不能设为当前
        set_active_operator_profile(&conn, &current, a.id, false).unwrap();
        let err = set_current_operator(&conn, &current, a.id)
            .unwrap_err()
            .to_string();
        assert!(err.contains("停用"), "停用操作人不可设为当前：{err}");
        // 不存在的操作人
        assert!(set_current_operator(&conn, &current, 999).is_err());

        // 正常选择：require 返回 (id, 姓名)，并写入 app_settings 追溯
        let selected = set_current_operator(&conn, &current, b.id).unwrap();
        assert_eq!(selected.name, "李出纳");
        let (id, name) = require_current_operator(&conn, &current).unwrap();
        assert_eq!(id, b.id);
        assert_eq!(name, "李出纳");
        assert_eq!(
            db::get_setting(&conn, ACTIVE_OPERATOR_SETTING)
                .unwrap()
                .as_deref(),
            Some(b.id.to_string()).as_deref()
        );
        let view = get_current_operator(&conn, &current).unwrap().unwrap();
        assert_eq!(view.id, b.id);

        // best-effort 署名取真实姓名
        assert_eq!(current_operator_name(&conn, &current), "李出纳");

        // DB 中被直接停用（绕过 API 的失效场景）：require 拦截，要求重选
        conn.execute(
            "UPDATE operator_profiles SET is_active = 0 WHERE id = ?1",
            params![b.id],
        )
        .unwrap();
        let err = require_current_operator(&conn, &current)
            .unwrap_err()
            .to_string();
        assert!(err.contains("停用"), "失效当前操作人应要求重选：{err}");

        // 重新启用 b，另选 c 为当前；通过 API 停用当前操作人：会话被清空，要求重新选择
        set_active_operator_profile(&conn, &current, b.id, true).unwrap();
        let c = save_operator_profile(&conn, &current, &operator_input("王主管", "admin"))
            .unwrap()
            .0;
        set_current_operator(&conn, &current, c.id).unwrap();
        set_active_operator_profile(&conn, &current, c.id, false).unwrap();
        assert_eq!(current_operator_name(&conn, &current), "system");
        assert!(get_current_operator(&conn, &current).unwrap().is_none());
    }

    /// 停用当前操作人（命令层时序）：cashier 层在清会话前捕获署名并返回，
    /// 命令层用返回署名写审计——operation_logs 的 operator 仍是被停用者而非 system。
    #[test]
    fn test_deactivate_current_operator_audit_actor_captured_before_session_clear() {
        let conn = setup_financial_db();
        let current = CurrentOperatorState::new();
        let a = save_operator_profile(&conn, &current, &operator_input("张会计", "cashier"))
            .unwrap()
            .0;
        let b = save_operator_profile(&conn, &current, &operator_input("李出纳", "approver"))
            .unwrap()
            .0;
        set_current_operator(&conn, &current, a.id).unwrap();

        // set_active 路径：返回署名 = 被停用的当前操作人本人
        let (profile, actor) = set_active_operator_profile(&conn, &current, a.id, false).unwrap();
        assert!(!profile.is_active);
        assert_eq!(actor, "张会计", "停用前署名应为被停用者本人");
        // 会话已清空：事后取名退化为 system，不得再用于该条审计
        assert_eq!(current_operator_name(&conn, &current), "system");

        // 命令层用返回署名写审计：operator 字段为被停用者而非 system
        db::log_operation(
            &conn,
            "set_active_operator_profile",
            "停用操作人 张会计",
            &actor,
            None,
        )
        .unwrap();
        let logged: String = conn
            .query_row(
                "SELECT operator FROM operation_logs
                 WHERE operation_type = 'set_active_operator_profile'
                 ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(logged, "张会计", "审计署名不应退化为 system");

        // save 路径停用当前操作人同样返回变更前署名（先重新启用 a，满足至少一名启用操作人约束）
        set_active_operator_profile(&conn, &current, a.id, true).unwrap();
        set_current_operator(&conn, &current, b.id).unwrap();
        let mut deactivate = operator_input("李出纳", "approver");
        deactivate.id = Some(b.id);
        deactivate.is_active = Some(false);
        let (_, actor) = save_operator_profile(&conn, &current, &deactivate).unwrap();
        assert_eq!(
            actor, "李出纳",
            "save 路径停用当前操作人署名不应退化为 system"
        );
    }

    // ---------- 业务附件 ----------

    use crate::security::{self, SecurityState};
    use std::path::PathBuf;

    /// 附件测试环境：全量财务库 + 已初始化安全态（DEK 已加载）+ 当前操作人 + 临时 app_dir。
    fn attachment_env(name: &str) -> (Connection, CurrentOperatorState, SecurityState, PathBuf) {
        let conn = setup_financial_db();
        let current = CurrentOperatorState::new();
        let sec = SecurityState::new();
        security::setup(
            &conn,
            &sec,
            "Abcd1234",
            "RC-AAAA",
            "你小学班主任姓什么？",
            "王",
        )
        .unwrap();
        let op = save_operator_profile(&conn, &current, &operator_input("张会计", "cashier"))
            .unwrap()
            .0;
        set_current_operator(&conn, &current, op.id).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "salary-att-{name}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (conn, current, sec, dir)
    }

    fn write_source_file(dir: &PathBuf, name: &str, content: &[u8]) -> String {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().to_string()
    }

    fn attachment_input(path: &str, entity_type: &str, entity_id: i64) -> BusinessAttachmentInput {
        BusinessAttachmentInput {
            entity_type: entity_type.into(),
            entity_id,
            file_name: String::new(), // 空 → 取源文件名
            file_path: path.into(),
            encrypted: None,
            file_size: None,
            belong_month: Some("2026-08".into()),
            uploaded_by: None,
        }
    }

    /// 加密上传：落盘文件必须是密文（含 nonce 前缀、非明文内容），DB 标志/署名/大小正确，
    /// 解密 roundtrip 还原明文；归档路径符合 spec 4.6 目录结构。
    #[test]
    fn test_business_attachment_add_encrypted_and_roundtrip() {
        let (conn, current, sec, app_dir) = attachment_env("add-enc");
        let plain = b"payment voucher scan \x00\x01\xff attachment content";
        let src = write_source_file(&app_dir, "voucher.pdf", plain);

        let att = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src, "fund_document", 1),
        )
        .unwrap();

        assert!(att.encrypted, "DEK 已加载时附件必须加密");
        assert!(
            att.file_path.contains("attachments"),
            "归档路径: {}",
            att.file_path
        );
        assert!(att.file_path.contains(&format!(
            "attachments{}fund_document{}2026-08",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        )));
        assert!(
            att.file_name.ends_with("voucher.pdf"),
            "保留原文件名: {}",
            att.file_name
        );
        assert_eq!(att.file_size, Some(plain.len() as i64));
        assert_eq!(
            att.uploaded_by.as_deref(),
            Some("张会计"),
            "署名取当前操作人"
        );

        // 落盘文件确为密文：长度 = 12(nonce) + 明文 + 16(tag)，且字节内容不等于明文
        let stored = std::fs::read(&att.file_path).unwrap();
        assert_eq!(stored.len(), plain.len() + 12 + 16);
        assert_ne!(&stored[..], &plain[..]);
        assert_ne!(
            &stored[..12.min(stored.len())],
            &plain[..12.min(plain.len())]
        );

        // 解密 roundtrip
        let restored = app_dir.join("restored.bin");
        security::decrypt_file(
            std::path::Path::new(&att.file_path),
            &restored,
            &sec.dek().unwrap(),
        )
        .unwrap();
        assert_eq!(std::fs::read(&restored).unwrap(), plain);

        // 列表按实体返回
        let list = list_business_attachments(&conn, "fund_document", 1).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, att.id);
        assert!(list_business_attachments(&conn, "fund_document", 2)
            .unwrap()
            .is_empty());

        let _ = std::fs::remove_dir_all(&app_dir);
    }

    /// DEK 未加载（未解锁）时与发票归档同语义：明文归档，encrypted=0。
    #[test]
    fn test_business_attachment_add_without_dek_stays_plain() {
        let (conn, current, _sec_loaded, app_dir) = attachment_env("add-plain");
        let sec = SecurityState::new(); // 未 setup → 无 DEK
        let plain = b"plain attachment";
        let src = write_source_file(&app_dir, "note.txt", plain);

        let att = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src, "fund_document", 1),
        )
        .unwrap();

        assert!(!att.encrypted);
        assert_eq!(std::fs::read(&att.file_path).unwrap(), plain);

        let _ = std::fs::remove_dir_all(&app_dir);
    }

    /// 错误路径：实体类型枚举、实体 ID、源文件存在性、大小上限、未选操作人。
    #[test]
    fn test_business_attachment_validation_errors() {
        let (conn, current, sec, app_dir) = attachment_env("validation");
        let src = write_source_file(&app_dir, "a.pdf", b"data");

        // 实体类型不在枚举内
        let err = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src, "contract", 1),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("实体类型"), "非法实体类型应拦截: {err}");

        // 实体 ID 非法
        let err = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src, "fund_document", 0),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("实体 ID"), "实体 ID 应拦截: {err}");

        // 源文件不存在
        let err = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(
                &app_dir.join("missing.pdf").to_string_lossy(),
                "fund_document",
                1,
            ),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("源文件不存在"), "缺失源文件应拦截: {err}");

        // 超过大小上限
        let big = write_source_file(
            &app_dir,
            "big.bin",
            &vec![0u8; (ATTACHMENT_MAX_FILE_SIZE + 1) as usize],
        );
        let err = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&big, "fund_document", 1),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("大小限制"), "超限附件应拦截: {err}");
        // 失败不落库
        assert!(list_business_attachments(&conn, "fund_document", 1)
            .unwrap()
            .is_empty());

        // 未选择当前操作人
        let fresh_current = CurrentOperatorState::new();
        let err = add_business_attachment(
            &conn,
            &sec,
            &fresh_current,
            &app_dir,
            &attachment_input(&src, "fund_document", 1),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("操作人"), "未选操作人应拦截: {err}");

        let _ = std::fs::remove_dir_all(&app_dir);
    }

    /// 删除门禁：未提交报销单可删（文件+DB 行一并清理）；已提交/已审批的报销单拦截；
    /// fund_document 待 Task 6 建表后补状态校验（当前放行）。
    #[test]
    fn test_business_attachment_delete_rules() {
        let (conn, current, sec, app_dir) = attachment_env("delete");
        conn.execute(
            "INSERT INTO reimbursement_claims (id, claim_no, employee_id, belong_month, title,
                total_amount, status, payment_status, created_at, updated_at)
             VALUES (501, 'BX-TASK5', 1, '2026-08', '差旅报销', 100, 'draft', 'unpaid',
                     '2026-08-01', '2026-08-01')",
            [],
        )
        .unwrap();
        let src = write_source_file(&app_dir, "receipt.pdf", b"receipt");
        let att = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src, "reimbursement_claim", 501),
        )
        .unwrap();

        // 不存在的附件
        let err = delete_business_attachment(&conn, &current, 999)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("未找到") || err.contains("NotFound"),
            "删除不存在附件应报错: {err}"
        );

        // 已提交（submitted）拦截
        conn.execute(
            "UPDATE reimbursement_claims SET status='submitted' WHERE id=501",
            [],
        )
        .unwrap();
        let err = delete_business_attachment(&conn, &current, att.id)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("反审批") || err.contains("驳回"),
            "已提交实体附件应拦截删除: {err}"
        );
        assert!(
            std::path::Path::new(&att.file_path).exists(),
            "拦截后文件必须保留"
        );

        // 已审批（approved）同样拦截
        conn.execute(
            "UPDATE reimbursement_claims SET status='approved' WHERE id=501",
            [],
        )
        .unwrap();
        assert!(delete_business_attachment(&conn, &current, att.id).is_err());

        // 驳回后可删：文件与 DB 行一并清理
        conn.execute(
            "UPDATE reimbursement_claims SET status='rejected' WHERE id=501",
            [],
        )
        .unwrap();
        let removed = delete_business_attachment(&conn, &current, att.id).unwrap();
        assert_eq!(removed, att.file_name);
        assert!(
            !std::path::Path::new(&att.file_path).exists(),
            "删除后归档文件应清理"
        );
        assert!(list_business_attachments(&conn, "reimbursement_claim", 501)
            .unwrap()
            .is_empty());

        // fund_document（实体表未建）：删除放行，留待 Task 6 收紧
        let src2 = write_source_file(&app_dir, "pay.pdf", b"pay");
        let att2 = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src2, "fund_document", 7),
        )
        .unwrap();
        delete_business_attachment(&conn, &current, att2.id).unwrap();
        assert!(!std::path::Path::new(&att2.file_path).exists());

        let _ = std::fs::remove_dir_all(&app_dir);
    }

    /// 补偿清理：DB 写入失败（file_path UNIQUE 冲突）时，已归档文件与加密临时文件
    /// 必须被清理，不留孤儿文件。用固定时间戳的 impl 入口保证目标路径可预测。
    #[test]
    fn test_business_attachment_add_db_failure_cleans_up_file() {
        use chrono::TimeZone;

        let (conn, current, sec, app_dir) = attachment_env("db-fail");
        let src = write_source_file(&app_dir, "dup.pdf", b"dup");
        let input = attachment_input(&src, "fund_document", 1);

        // 固定归档时间 → 目标路径完全可预测
        let fixed = chrono::Utc.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
        let expected_target = app_dir
            .join("attachments")
            .join("fund_document")
            .join("2026-08")
            .join(format!("{}_dup.pdf", fixed.format("%Y%m%d%H%M%S")));

        // 预插一行占用预期 file_path → add 落库时触发 UNIQUE 冲突
        conn.execute(
            "INSERT INTO business_attachments
                (entity_type, entity_id, file_name, file_path, encrypted, file_size, belong_month, uploaded_by, created_at)
             VALUES ('fund_document', 1, 'dup.pdf', ?1, 0, 3, '2026-08', NULL, 'now')",
            params![expected_target.to_string_lossy()],
        )
        .unwrap();

        // 用固定时间戳的 impl 入口（目标路径可预测），落库失败触发补偿清理
        let err = add_business_attachment_impl(&conn, &sec, &current, &app_dir, &input, fixed)
            .unwrap_err()
            .to_string();
        assert!(!err.is_empty());
        // 补偿清理：归档文件与 .enc.tmp 都不得残留
        assert!(
            !expected_target.exists(),
            "落库失败后归档文件应被补偿清理: {}",
            expected_target.display()
        );
        assert!(!expected_target.with_extension("enc.tmp").exists());
        assert_eq!(
            list_business_attachments(&conn, "fund_document", 1)
                .unwrap()
                .len(),
            1
        );

        let _ = std::fs::remove_dir_all(&app_dir);
    }
}
