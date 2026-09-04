//! 第七阶段出纳领域模块（cashier）：资金账户 / 往来单位 / 操作人基础资料、当前操作人会话、
//! 通用资金单据状态机与追加式审批事件。
//!
//! 模块边界（spec 第 10 节）：
//! - 本模块负责出纳主数据、资金单据（fund_documents）命令驱动状态机与审批轨迹；
//!   后续批次在此追加资金日记账、银行对账与借款核销。
//! - `db.rs` 只保留 schema/迁移与低层通用 helper；凭证生成、冲正凭证与报表归 `accounting.rs`；
//! - `commands.rs` 负责 State 管理、文件对话框参数与日志编排。
//!
//! 基础资料为主数据：不受 `ensure_month_open` 月结保护（spec 4.4 仅资金单据受月结限制）。
//! 金额一律正数存储，比较容差 0.005；更新入参 `Option` 字段为 patch 语义（None=保留原值，
//! `Some("")`=清空）；错误信息统一中文。
//! 资金单据状态只能经命令流转（submit/approve/reject/withdraw/void/mark_batched/settle/reverse），
//! 状态更新与 approval_events 追加同事务；前端不得直接编辑状态字段。

use std::sync::Mutex;

use crate::db;
use crate::db::ensure_month_open;
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
/// 编码唯一、名称+税号唯一（税号空按空串归一化）；
/// 引用保护（spec 4.2）：被资金单据引用后只允许停用，不允许修改类型
/// （本表无删除入口，停用始终允许；改类型会破坏既有单据的核算口径）。
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
    // 引用保护（spec 4.2）：被资金单据引用后只允许停用，不允许改类型
    if let Some(id) = input.id {
        let type_changed = prev
            .map(|e| e.partner_type != input.partner_type)
            .unwrap_or(false);
        if type_changed && business_partner_referenced(&tx, id)? {
            return Err(AppError::General(format!(
                "往来单位 {name} 已被资金单据引用，不能修改类型；只允许停用"
            )));
        }
    }
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

/// 启用/停用往来单位（改 status）。停用始终允许——包括被资金单据引用的单位
/// （spec 4.2：被引用后只允许停用；改类型限制见 save_business_partner）。
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

/// 统计往来单位是否被资金单据引用（引用保护依据）
fn business_partner_referenced(conn: &Connection, id: i64) -> AppResult<bool> {
    let used: i64 = conn.query_row(
        "SELECT COUNT(*) FROM fund_documents WHERE partner_id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    Ok(used > 0)
}

// ==================== 资金单据与状态机 ====================
// 说明：本节为 Task 6 领域层（状态机 + 审批事件），Tauri 命令由 Task 7 暴露；
// 命令暴露前整节对 lib 构建不可达，逐项挂 #[allow(dead_code)]（Task 7 统一移除）。

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 资金单据类型（spec 4.4）
const FUND_DOCUMENT_TYPES: &[&str] = &[
    "receipt",
    "payment",
    "transfer",
    "advance",
    "advance_settlement",
    "reversal",
];

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 资金单据状态（spec 4.4/5.1）
const FUND_DOCUMENT_STATUSES: &[&str] = &[
    "draft",
    "submitted",
    "approved",
    "rejected",
    "batched",
    "settled",
    "void",
    "reversed",
];

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 经办复核开关的 app_settings 键（spec 2：可选 maker_checker_enabled；
/// 启用时提交人与审批人不得相同。单机切换身份属流程约束，不宣称安全隔离）
const MAKER_CHECKER_SETTING: &str = "maker_checker_enabled";

/// 资金单附件可变更状态：仅未进入审批流的单据（与报销单口径一致，spec 4.6/第 8 节）
const FUND_DOCUMENT_ATTACHMENT_EDITABLE_STATUSES: &[&str] = &["draft", "rejected", "void"];

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 可直接结算的单据类型（spec 5.1：receipt/transfer 直接收支结算；
/// advance_settlement 核销回流走结算；payment/advance 必须经付款批次标记付款）
const DIRECT_SETTLE_TYPES: &[&str] = &["receipt", "transfer", "advance_settlement"];

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 可进入付款批次的单据类型（spec 5.1：payment/advance）
const BATCHABLE_TYPES: &[&str] = &["payment", "advance"];

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
const FUND_DOCUMENT_COLS: &str = "id, document_no, document_type, belong_month, document_date, amount, summary, department, expense_type, remark, partner_id, employee_id, source_account_id, target_account_id, counter_account_code, status, payment_batch_id, reversal_of_id, submitted_by, submitted_at, approved_by, approved_at, settled_by, settled_at, voided_by, voided_at, created_by, created_at, updated_at";

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
fn fund_document_from_row(r: &rusqlite::Row) -> rusqlite::Result<FundDocument> {
    Ok(FundDocument {
        id: r.get(0)?,
        document_no: r.get(1)?,
        document_type: r.get(2)?,
        belong_month: r.get(3)?,
        document_date: r.get(4)?,
        amount: r.get(5)?,
        summary: r.get(6)?,
        department: r.get(7)?,
        expense_type: r.get(8)?,
        remark: r.get(9)?,
        partner_id: r.get(10)?,
        employee_id: r.get(11)?,
        source_account_id: r.get(12)?,
        target_account_id: r.get(13)?,
        counter_account_code: r.get(14)?,
        status: r.get(15)?,
        payment_batch_id: r.get(16)?,
        reversal_of_id: r.get(17)?,
        submitted_by: r.get(18)?,
        submitted_at: r.get(19)?,
        approved_by: r.get(20)?,
        approved_at: r.get(21)?,
        settled_by: r.get(22)?,
        settled_at: r.get(23)?,
        voided_by: r.get(24)?,
        voided_at: r.get(25)?,
        created_by: r.get(26)?,
        created_at: r.get(27)?,
        updated_at: r.get(28)?,
    })
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 状态中文名（错误信息用）
fn fund_status_label(status: &str) -> &'static str {
    match status {
        "draft" => "草稿",
        "submitted" => "已提交",
        "approved" => "已审批",
        "rejected" => "已驳回",
        "batched" => "已进批次",
        "settled" => "已结算",
        "void" => "已作废",
        "reversed" => "已冲正",
        _ => "未知状态",
    }
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 单据类型中文名（错误信息用）
fn fund_document_type_label(doc_type: &str) -> &'static str {
    match doc_type {
        "receipt" => "收款单",
        "payment" => "付款单",
        "transfer" => "内部转账单",
        "advance" => "员工借款单",
        "advance_settlement" => "借款核销单",
        "reversal" => "冲正单",
        _ => "资金单据",
    }
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 状态机动作中文名（错误信息与审批事件注释校验提示用）
fn fund_action_label(action: &str) -> &'static str {
    match action {
        "submit" => "提交",
        "approve" => "审批",
        "reject" => "驳回",
        "withdraw" => "撤回",
        "void" => "作废",
        "batch" => "进入付款批次",
        "settle" => "结算",
        "reverse" => "冲正",
        _ => "状态变更",
    }
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 单据编号：类型前缀 + 单据日期 + 进程相关唯一后缀（与付款批次同防撞策略，spec 4.4 按类型和日期生成）
fn fund_document_no(document_type: &str, document_date: &str) -> String {
    let prefix = match document_type {
        "receipt" => "SK",
        "payment" => "FK",
        "transfer" => "NB",
        "advance" => "JK",
        "advance_settlement" => "HX",
        _ => "CZ",
    };
    let date_compact = document_date.replace('-', "");
    let nanos = Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or_else(|| Utc::now().timestamp_millis() as i64);
    let suffix = std::process::id() as i64 ^ nanos;
    format!("{prefix}{date_compact}{nanos}{:04X}", suffix & 0xFFFF)
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 归属月份格式校验（YYYY-MM）
fn validate_belong_month(month: &str) -> AppResult<()> {
    let valid = month.len() == 7
        && month.as_bytes().get(4) == Some(&b'-')
        && NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").is_ok();
    if valid {
        Ok(())
    } else {
        Err(AppError::InvalidParam(format!(
            "归属月份格式应为 YYYY-MM：{month}"
        )))
    }
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 单据日期格式校验（YYYY-MM-DD）且必须落在归属月份内
fn validate_document_date(document_date: &str, belong_month: &str) -> AppResult<()> {
    let parsed = NaiveDate::parse_from_str(document_date, "%Y-%m-%d").map_err(|_| {
        AppError::InvalidParam(format!("单据日期格式应为 YYYY-MM-DD：{document_date}"))
    })?;
    if !document_date.starts_with(&format!("{belong_month}-")) {
        return Err(AppError::InvalidParam(format!(
            "单据日期 {document_date} 必须落在归属月份 {belong_month} 内"
        )));
    }
    let _ = parsed;
    Ok(())
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// patch 语义可空字符串规范化：去空格，空串归一为 None
fn trimmed_optional(v: Option<&str>) -> Option<String> {
    v.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 收款/付款单往来对象：往来单位与员工二选一（spec 4.4 择一规则）
fn ensure_single_counterparty(input: &FundDocumentInput) -> AppResult<()> {
    match (input.partner_id.is_some(), input.employee_id.is_some()) {
        (true, false) | (false, true) => Ok(()),
        (true, true) => Err(AppError::InvalidParam(
            "收款/付款单的往来单位与员工只能二选一".into(),
        )),
        (false, false) => Err(AppError::InvalidParam(
            "收款/付款单必须选择往来单位或员工".into(),
        )),
    }
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 对方科目校验：收款/付款结算前必填（spec 4.4）；可填时必须为存在的总账科目
fn ensure_counter_account(
    conn: &Connection,
    code: Option<&str>,
    label: &str,
    required: bool,
) -> AppResult<()> {
    let trimmed = code.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        if required {
            return Err(AppError::InvalidParam(format!(
                "{label}必须填写对方科目（资金科目以外的总账科目）"
            )));
        }
        return Ok(());
    }
    ensure_gl_account_exists(conn, trimmed)
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 往来单位存在且启用
fn ensure_partner_usable(conn: &Connection, partner_id: i64) -> AppResult<()> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT partner_code, status FROM business_partners WHERE id = ?1",
            params![partner_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        None => Err(AppError::NotFound(format!(
            "往来单位不存在：id={partner_id}"
        ))),
        Some((code, status)) if status != "active" => Err(AppError::InvalidParam(format!(
            "往来单位 {code} 已停用，不能用于新单据"
        ))),
        Some(_) => Ok(()),
    }
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 员工存在且在职
fn ensure_employee_usable(conn: &Connection, employee_id: i64) -> AppResult<()> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT name, status FROM employees WHERE id = ?1",
            params![employee_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        None => Err(AppError::NotFound(format!("员工不存在：id={employee_id}"))),
        Some((name, status)) if status != "active" => Err(AppError::InvalidParam(format!(
            "员工 {name} 已离职/停用，不能用于新单据"
        ))),
        Some(_) => Ok(()),
    }
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 资金账户存在且启用
fn ensure_fund_account_usable(conn: &Connection, label: &str, account_id: i64) -> AppResult<()> {
    let row: Option<(String, i64)> = conn
        .query_row(
            "SELECT account_code, is_active FROM fund_accounts WHERE id = ?1",
            params![account_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        None => Err(AppError::NotFound(format!(
            "{label}不存在：id={account_id}"
        ))),
        Some((code, active)) if active == 0 => Err(AppError::InvalidParam(format!(
            "资金账户 {code} 已停用，不能用于新单据"
        ))),
        Some(_) => Ok(()),
    }
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 单据内容校验（spec 4.4/5.1）：类型合法、金额正数、摘要必填、月份/日期格式与匹配、
/// 账户方向与往来对象择一规则、引用对象存在且启用、对方科目必填/存在。
fn validate_fund_document_content(conn: &Connection, input: &FundDocumentInput) -> AppResult<()> {
    if !FUND_DOCUMENT_TYPES.contains(&input.document_type.as_str()) {
        return Err(AppError::InvalidParam(format!(
            "资金单据类型无效：{}（允许：{}）",
            input.document_type,
            FUND_DOCUMENT_TYPES.join(" / ")
        )));
    }
    if input.document_type == "reversal" {
        return Err(AppError::InvalidParam(
            "冲正单不能手工创建，请对已结算单据执行冲正操作".into(),
        ));
    }
    if input.summary.trim().is_empty() {
        return Err(AppError::InvalidParam("单据摘要不能为空".into()));
    }
    // 金额统一正数（容差 0.005：低于容差视为零金额；NaN 一并拦截）
    if !(input.amount >= AMOUNT_TOLERANCE) {
        return Err(AppError::InvalidParam("单据金额必须为正数".into()));
    }
    validate_belong_month(&input.belong_month)?;
    validate_document_date(&input.document_date, &input.belong_month)?;

    match input.document_type.as_str() {
        "transfer" => {
            let (src, tgt) = match (input.source_account_id, input.target_account_id) {
                (Some(s), Some(t)) => (s, t),
                _ => {
                    return Err(AppError::InvalidParam(
                        "内部转账单必须同时选择来源账户与目标账户".into(),
                    ))
                }
            };
            if src == tgt {
                return Err(AppError::InvalidParam(
                    "内部转账单的来源账户与目标账户不能相同".into(),
                ));
            }
            if input.partner_id.is_some() || input.employee_id.is_some() {
                return Err(AppError::InvalidParam(
                    "内部转账单不需要选择往来单位或员工".into(),
                ));
            }
            ensure_fund_account_usable(conn, "来源账户", src)?;
            ensure_fund_account_usable(conn, "目标账户", tgt)?;
            ensure_counter_account(
                conn,
                input.counter_account_code.as_deref(),
                "内部转账单",
                false,
            )?;
        }
        "receipt" => {
            if input.source_account_id.is_some() {
                return Err(AppError::InvalidParam(
                    "收款单只能选择目标账户（资金流入），不能选择来源账户".into(),
                ));
            }
            let Some(target) = input.target_account_id else {
                return Err(AppError::InvalidParam(
                    "收款单必须选择目标账户（资金流入账户）".into(),
                ));
            };
            ensure_single_counterparty(input)?;
            ensure_fund_account_usable(conn, "目标账户", target)?;
            ensure_counter_account(conn, input.counter_account_code.as_deref(), "收款单", true)?;
        }
        "payment" => {
            if input.target_account_id.is_some() {
                return Err(AppError::InvalidParam(
                    "付款单只能选择来源账户（资金流出），不能选择目标账户".into(),
                ));
            }
            let Some(source) = input.source_account_id else {
                return Err(AppError::InvalidParam(
                    "付款单必须选择来源账户（资金流出账户）".into(),
                ));
            };
            ensure_single_counterparty(input)?;
            ensure_fund_account_usable(conn, "来源账户", source)?;
            ensure_counter_account(conn, input.counter_account_code.as_deref(), "付款单", true)?;
        }
        "advance" => {
            if input.target_account_id.is_some() {
                return Err(AppError::InvalidParam(
                    "员工借款单只能选择来源账户（资金流出），不能选择目标账户".into(),
                ));
            }
            let Some(source) = input.source_account_id else {
                return Err(AppError::InvalidParam(
                    "员工借款单必须选择来源账户（资金流出账户）".into(),
                ));
            };
            if input.employee_id.is_none() || input.partner_id.is_some() {
                return Err(AppError::InvalidParam(
                    "员工借款单必须选择员工，不能选择往来单位".into(),
                ));
            }
            ensure_fund_account_usable(conn, "来源账户", source)?;
        }
        "advance_settlement" => {
            if input.source_account_id.is_some() {
                return Err(AppError::InvalidParam(
                    "借款核销单只能选择目标账户（资金回流），不能选择来源账户".into(),
                ));
            }
            let Some(target) = input.target_account_id else {
                return Err(AppError::InvalidParam(
                    "借款核销单必须选择目标账户（资金回流账户）".into(),
                ));
            };
            if input.employee_id.is_none() || input.partner_id.is_some() {
                return Err(AppError::InvalidParam(
                    "借款核销单必须选择员工，不能选择往来单位".into(),
                ));
            }
            ensure_fund_account_usable(conn, "目标账户", target)?;
        }
        _ => unreachable!("reversal 已在上方拦截"),
    }

    if let Some(pid) = input.partner_id {
        ensure_partner_usable(conn, pid)?;
    }
    if let Some(eid) = input.employee_id {
        ensure_employee_usable(conn, eid)?;
    }
    Ok(())
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 按条件查询资金单据（月份/类型/状态/往来对象/关键字），按 id 倒序（新单在前）
pub fn get_fund_documents(
    conn: &Connection,
    q: &FundDocumentQuery,
) -> AppResult<Vec<FundDocument>> {
    let mut where_clauses: Vec<String> = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;
    if let Some(m) = q
        .belong_month
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        where_clauses.push(format!("belong_month = ?{idx}"));
        params_vec.push(Box::new(m.to_string()));
        idx += 1;
    }
    if let Some(t) = q
        .document_type
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        where_clauses.push(format!("document_type = ?{idx}"));
        params_vec.push(Box::new(t.to_string()));
        idx += 1;
    }
    if let Some(s) = q.status.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        where_clauses.push(format!("status = ?{idx}"));
        params_vec.push(Box::new(s.to_string()));
        idx += 1;
    }
    if let Some(p) = q.partner_id {
        where_clauses.push(format!("partner_id = ?{idx}"));
        params_vec.push(Box::new(p));
        idx += 1;
    }
    if let Some(e) = q.employee_id {
        where_clauses.push(format!("employee_id = ?{idx}"));
        params_vec.push(Box::new(e));
        idx += 1;
    }
    if let Some(kw) = q
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        where_clauses.push(format!("(document_no LIKE ?{idx} OR summary LIKE ?{idx})"));
        params_vec.push(Box::new(format!("%{kw}%")));
    }
    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_clauses.join(" AND "))
    };
    let sql =
        format!("SELECT {FUND_DOCUMENT_COLS} FROM fund_documents{where_sql} ORDER BY id DESC");
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), fund_document_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 按主键取单个资金单据（领域内部与命令层共用）
fn get_fund_document(conn: &Connection, id: i64) -> AppResult<FundDocument> {
    conn.query_row(
        &format!("SELECT {FUND_DOCUMENT_COLS} FROM fund_documents WHERE id = ?1"),
        params![id],
        fund_document_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("资金单据不存在：id={id}")))
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 单据详情：单据 + 审批轨迹（id 升序，可重放完整历史，spec 4.5）
pub fn get_fund_document_detail(conn: &Connection, id: i64) -> AppResult<FundDocumentDetail> {
    let document = get_fund_document(conn, id)?;
    let events = list_approval_events(conn, "fund_document", id)?;
    Ok(FundDocumentDetail { document, events })
}

/// 查询单据状态（附件门禁用；None = 单据不存在）
fn get_fund_document_status(conn: &Connection, id: i64) -> AppResult<Option<String>> {
    conn.query_row(
        "SELECT status FROM fund_documents WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )
    .optional()
    .map_err(AppError::from)
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 追加一条审批事件（仅插入：模块刻意不提供 UPDATE/DELETE 路径，spec 4.5）。
/// 必须与状态更新同事务调用。
fn insert_approval_event(
    conn: &Connection,
    entity_type: &str,
    entity_id: i64,
    action: &str,
    from_status: Option<&str>,
    to_status: Option<&str>,
    operator_id: Option<i64>,
    comment: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO approval_events
            (entity_type, entity_id, action, from_status, to_status, operator_id, comment, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            entity_type,
            entity_id,
            action,
            from_status,
            to_status,
            operator_id,
            comment,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 按实体查询审批轨迹（id 升序 = 时间重放顺序，spec 4.5）
pub fn list_approval_events(
    conn: &Connection,
    entity_type: &str,
    entity_id: i64,
) -> AppResult<Vec<ApprovalEvent>> {
    if !ATTACHMENT_ENTITY_TYPES.contains(&entity_type) {
        return Err(AppError::InvalidParam(format!(
            "不支持的审批实体类型: {entity_type}"
        )));
    }
    let mut stmt = conn.prepare(
        "SELECT id, entity_type, entity_id, action, from_status, to_status, operator_id, comment, created_at
         FROM approval_events
         WHERE entity_type = ?1 AND entity_id = ?2
         ORDER BY id",
    )?;
    let rows = stmt.query_map(params![entity_type, entity_id], |r| {
        Ok(ApprovalEvent {
            id: r.get(0)?,
            entity_type: r.get(1)?,
            entity_id: r.get(2)?,
            action: r.get(3)?,
            from_status: r.get(4)?,
            to_status: r.get(5)?,
            operator_id: r.get(6)?,
            comment: r.get(7)?,
            created_at: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 读取经办复核开关（app_settings；缺省关闭）
fn maker_checker_enabled(conn: &Connection) -> AppResult<bool> {
    Ok(db::get_setting(conn, MAKER_CHECKER_SETTING)?
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false))
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 设置经办复核开关（Task 7 命令层暴露给设置界面）
pub fn set_maker_checker_enabled(conn: &Connection, enabled: bool) -> AppResult<()> {
    db::set_setting(
        conn,
        MAKER_CHECKER_SETTING,
        if enabled { "true" } else { "false" },
    )
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 创建资金单据（草稿）。须当前操作人署名且归属月份未月结（spec 4.4）。
pub fn create_fund_document(
    conn: &Connection,
    current: &CurrentOperatorState,
    input: &FundDocumentInput,
) -> AppResult<FundDocument> {
    let (operator_id, _) = require_current_operator(conn, current)?;
    if input.id.is_some() {
        return Err(AppError::InvalidParam(
            "创建单据不需要传 id，请使用更新接口".into(),
        ));
    }
    validate_fund_document_content(conn, input)?;
    ensure_month_open(conn, &input.belong_month)?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO fund_documents
            (document_no, document_type, belong_month, document_date, amount, summary,
             department, expense_type, remark, partner_id, employee_id, source_account_id,
             target_account_id, counter_account_code, status, created_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 'draft', ?15, ?16, ?16)",
        params![
            fund_document_no(&input.document_type, &input.document_date),
            input.document_type,
            input.belong_month,
            input.document_date,
            input.amount,
            input.summary.trim(),
            trimmed_optional(input.department.as_deref()),
            trimmed_optional(input.expense_type.as_deref()),
            trimmed_optional(input.remark.as_deref()),
            input.partner_id,
            input.employee_id,
            input.source_account_id,
            input.target_account_id,
            trimmed_optional(input.counter_account_code.as_deref()),
            operator_id,
            now,
        ],
    )?;
    get_fund_document(conn, conn.last_insert_rowid())
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 更新资金单据（仅草稿可编辑；submitted 后业务字段冻结，spec 5.1）。
/// 入参不含状态字段：状态只能经状态机命令流转，无法由字段更新绕过。
pub fn update_fund_document(
    conn: &Connection,
    current: &CurrentOperatorState,
    input: &FundDocumentInput,
) -> AppResult<FundDocument> {
    require_current_operator(conn, current)?;
    let Some(id) = input.id else {
        return Err(AppError::InvalidParam("更新单据必须传 id".into()));
    };
    let existing = get_fund_document(conn, id)?;
    if existing.status != "draft" {
        return Err(AppError::General(format!(
            "单据 {} 当前状态「{}」，仅草稿可编辑；已提交单据请先撤回",
            existing.document_no,
            fund_status_label(&existing.status)
        )));
    }
    validate_fund_document_content(conn, input)?;
    // 归属月份可改：原月份与新月份都必须开放
    ensure_month_open(conn, &existing.belong_month)?;
    ensure_month_open(conn, &input.belong_month)?;
    conn.execute(
        "UPDATE fund_documents SET document_type = ?2, belong_month = ?3, document_date = ?4,
            amount = ?5, summary = ?6, department = ?7, expense_type = ?8, remark = ?9,
            partner_id = ?10, employee_id = ?11, source_account_id = ?12, target_account_id = ?13,
            counter_account_code = ?14, updated_at = ?15
         WHERE id = ?1",
        params![
            id,
            input.document_type,
            input.belong_month,
            input.document_date,
            input.amount,
            input.summary.trim(),
            trimmed_optional(input.department.as_deref()),
            trimmed_optional(input.expense_type.as_deref()),
            trimmed_optional(input.remark.as_deref()),
            input.partner_id,
            input.employee_id,
            input.source_account_id,
            input.target_account_id,
            trimmed_optional(input.counter_account_code.as_deref()),
            Utc::now().to_rfc3339()
        ],
    )?;
    get_fund_document(conn, id)
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 状态机命令公共骨架：当前操作人署名 → 事务内取单据 → 校验来源状态 →
/// 月结保护 → 单据状态更新 + 追加审批事件（同事务，spec 5.1/4.5）→ 提交。
/// `extra` 在状态更新前执行（maker_checker 校验、署名字段/批次回写等），
/// 任一步失败整体回滚，不留半成品。
#[allow(clippy::too_many_arguments)]
fn transition_fund_document<F>(
    conn: &Connection,
    current: &CurrentOperatorState,
    document_id: i64,
    action: &str,
    from_statuses: &[&str],
    to_status: &str,
    comment: Option<&str>,
    require_comment: bool,
    extra: F,
) -> AppResult<FundDocument>
where
    F: FnOnce(&Connection, &FundDocument, i64, &str) -> AppResult<()>,
{
    let (operator_id, _) = require_current_operator(conn, current)?;
    let trimmed = comment.map(str::trim).unwrap_or("");
    if require_comment && trimmed.is_empty() {
        return Err(AppError::InvalidParam(format!(
            "{}必须填写意见或原因",
            fund_action_label(action)
        )));
    }
    let now = Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;
    let doc = get_fund_document(&tx, document_id)?;
    if !from_statuses.contains(&doc.status.as_str()) {
        return Err(AppError::General(format!(
            "单据 {} 当前状态「{}」，不允许{}（仅允许来源状态：{}）",
            doc.document_no,
            fund_status_label(&doc.status),
            fund_action_label(action),
            from_statuses
                .iter()
                .map(|s| fund_status_label(s))
                .collect::<Vec<_>>()
                .join("、")
        )));
    }
    // 所有资金写操作均受月结保护（spec 4.4）
    ensure_month_open(&tx, &doc.belong_month)?;
    extra(&tx, &doc, operator_id, &now)?;
    tx.execute(
        "UPDATE fund_documents SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![document_id, to_status, now],
    )?;
    insert_approval_event(
        &tx,
        "fund_document",
        document_id,
        action,
        Some(&doc.status),
        Some(to_status),
        Some(operator_id),
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        },
    )?;
    tx.commit()?;
    get_fund_document(conn, document_id)
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 提交单据（draft → submitted）。提交后业务字段冻结，须撤回才可修改。
/// 记录提交人署名（maker_checker 审批人去重的比对依据）。
pub fn submit_fund_document(
    conn: &Connection,
    current: &CurrentOperatorState,
    id: i64,
    comment: Option<&str>,
) -> AppResult<FundDocument> {
    transition_fund_document(
        conn,
        current,
        id,
        "submit",
        &["draft"],
        "submitted",
        comment,
        false,
        |tx, doc, operator_id, now| {
            tx.execute(
                "UPDATE fund_documents SET submitted_by = ?2, submitted_at = ?3 WHERE id = ?1",
                params![doc.id, operator_id, now],
            )?;
            Ok(())
        },
    )
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 审批通过（submitted → approved）。
/// maker_checker 启用时审批人与提交人不得相同（spec 2）。
pub fn approve_fund_document(
    conn: &Connection,
    current: &CurrentOperatorState,
    id: i64,
    comment: &str,
) -> AppResult<FundDocument> {
    transition_fund_document(
        conn,
        current,
        id,
        "approve",
        &["submitted"],
        "approved",
        Some(comment),
        true,
        |tx, doc, operator_id, now| {
            if maker_checker_enabled(tx)? && doc.submitted_by == Some(operator_id) {
                return Err(AppError::General(
                    "经办复核已启用：审批人与提交人不能是同一人，请切换操作人后再审批".into(),
                ));
            }
            tx.execute(
                "UPDATE fund_documents SET approved_by = ?2, approved_at = ?3 WHERE id = ?1",
                params![doc.id, operator_id, now],
            )?;
            Ok(())
        },
    )
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 驳回（submitted → rejected）。须填写驳回意见。
pub fn reject_fund_document(
    conn: &Connection,
    current: &CurrentOperatorState,
    id: i64,
    comment: &str,
) -> AppResult<FundDocument> {
    transition_fund_document(
        conn,
        current,
        id,
        "reject",
        &["submitted"],
        "rejected",
        Some(comment),
        true,
        |_, _, _, _| Ok(()),
    )
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 撤回（submitted/rejected → draft）：提交人反悔或按驳回意见修改后重走流程。
pub fn withdraw_fund_document(
    conn: &Connection,
    current: &CurrentOperatorState,
    id: i64,
    comment: Option<&str>,
) -> AppResult<FundDocument> {
    transition_fund_document(
        conn,
        current,
        id,
        "withdraw",
        &["submitted", "rejected"],
        "draft",
        comment,
        false,
        |_, _, _, _| Ok(()),
    )
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 作废（仅未结算，spec 5.1）。已进批次须先由批次作废释放；已结算只能冲正。
pub fn void_fund_document(
    conn: &Connection,
    current: &CurrentOperatorState,
    id: i64,
    comment: &str,
) -> AppResult<FundDocument> {
    transition_fund_document(
        conn,
        current,
        id,
        "void",
        &["draft", "submitted", "approved", "rejected"],
        "void",
        Some(comment),
        true,
        |tx, doc, operator_id, now| {
            tx.execute(
                "UPDATE fund_documents SET voided_by = ?2, voided_at = ?3 WHERE id = ?1",
                params![doc.id, operator_id, now],
            )?;
            Ok(())
        },
    )
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 标记进入付款批次（approved → batched；仅付款/借款单，Task 9 通用批次创建时调用）。
pub fn mark_document_batched(
    conn: &Connection,
    current: &CurrentOperatorState,
    document_id: i64,
    batch_id: i64,
) -> AppResult<FundDocument> {
    transition_fund_document(
        conn,
        current,
        document_id,
        "batch",
        &["approved"],
        "batched",
        None,
        false,
        |tx, doc, _operator_id, _now| {
            if !BATCHABLE_TYPES.contains(&doc.document_type.as_str()) {
                return Err(AppError::General(format!(
                    "单据 {} 类型为「{}」，只有付款单/员工借款单可进入付款批次",
                    doc.document_no,
                    fund_document_type_label(&doc.document_type)
                )));
            }
            let exists: Option<i64> = tx
                .query_row(
                    "SELECT id FROM payment_batches WHERE id = ?1",
                    params![batch_id],
                    |r| r.get(0),
                )
                .optional()?;
            if exists.is_none() {
                return Err(AppError::NotFound(format!("付款批次不存在：id={batch_id}")));
            }
            tx.execute(
                "UPDATE fund_documents SET payment_batch_id = ?2 WHERE id = ?1",
                params![doc.id, batch_id],
            )?;
            Ok(())
        },
    )
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 结算（spec 5.1）：收款/内部转账/借款核销单审批后直接结算；
/// 付款/借款单须经付款批次标记付款后从 batched 结算。
pub fn settle_fund_document(
    conn: &Connection,
    current: &CurrentOperatorState,
    id: i64,
) -> AppResult<FundDocument> {
    transition_fund_document(
        conn,
        current,
        id,
        "settle",
        &["approved", "batched"],
        "settled",
        None,
        false,
        |tx, doc, operator_id, now| {
            match doc.status.as_str() {
                "approved" if DIRECT_SETTLE_TYPES.contains(&doc.document_type.as_str()) => {}
                "batched" if BATCHABLE_TYPES.contains(&doc.document_type.as_str()) => {}
                "approved" => {
                    return Err(AppError::General(format!(
                        "单据 {} 类型为「{}」，须经付款批次标记付款后结算",
                        doc.document_no,
                        fund_document_type_label(&doc.document_type)
                    )));
                }
                _ => {}
            }
            tx.execute(
                "UPDATE fund_documents SET settled_by = ?2, settled_at = ?3 WHERE id = ?1",
                params![doc.id, operator_id, now],
            )?;
            Ok(())
        },
    )
}

#[allow(dead_code)] // TODO(Task 7 命令暴露后移除)
/// 冲正（settled → reversed，spec 5.1）：在开放月份创建相反方向冲正单（立即结算生效），
/// 原单置为已冲正；原单月份与冲正月份均须未月结。
/// 凭证联动（反向凭证 + 原凭证保留 active 建立追溯）由 Task 8 挂接，见函数尾 TODO。
pub fn reverse_fund_document(
    conn: &Connection,
    current: &CurrentOperatorState,
    input: &FundDocumentReverseInput,
) -> AppResult<FundDocument> {
    let (operator_id, _) = require_current_operator(conn, current)?;
    let comment = input.comment.trim();
    if comment.is_empty() {
        return Err(AppError::InvalidParam("冲正必须填写原因".into()));
    }
    validate_belong_month(&input.belong_month)?;
    validate_document_date(&input.document_date, &input.belong_month)?;

    let tx = conn.unchecked_transaction()?;
    let original = get_fund_document(&tx, input.document_id)?;
    if original.status != "settled" {
        return Err(AppError::General(format!(
            "单据 {} 当前状态「{}」，仅已结算单据可冲正",
            original.document_no,
            fund_status_label(&original.status)
        )));
    }
    // 月结保护覆盖原月份和冲正月份
    ensure_month_open(&tx, &original.belong_month)?;
    ensure_month_open(&tx, &input.belong_month)?;

    let now = Utc::now().to_rfc3339();
    // 相反方向：来源/目标账户互换；往来对象、对方科目、部门、费用类型随原单
    tx.execute(
        "INSERT INTO fund_documents
            (document_no, document_type, belong_month, document_date, amount, summary,
             department, expense_type, remark, partner_id, employee_id, source_account_id,
             target_account_id, counter_account_code, status, reversal_of_id,
             settled_by, settled_at, created_by, created_at, updated_at)
         VALUES (?1, 'reversal', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'settled', ?14, ?15, ?16, ?15, ?16, ?16)",
        params![
            fund_document_no("reversal", &input.document_date),
            input.belong_month,
            input.document_date,
            original.amount,
            format!("冲正：{}", original.summary),
            original.department,
            original.expense_type,
            format!("冲正原单 {}，原因：{comment}", original.document_no),
            original.partner_id,
            original.employee_id,
            original.target_account_id,
            original.source_account_id,
            original.counter_account_code,
            original.id,
            operator_id,
            now,
        ],
    )?;
    let reversal_id = tx.last_insert_rowid();

    tx.execute(
        "UPDATE fund_documents SET status = 'reversed', updated_at = ?2 WHERE id = ?1",
        params![original.id, now],
    )?;
    // 双向审批事件同事务追加（spec 4.5：reverse 必须带原因）
    insert_approval_event(
        &tx,
        "fund_document",
        original.id,
        "reverse",
        Some("settled"),
        Some("reversed"),
        Some(operator_id),
        Some(comment),
    )?;
    insert_approval_event(
        &tx,
        "fund_document",
        reversal_id,
        "reverse",
        None,
        Some("settled"),
        Some(operator_id),
        Some(comment),
    )?;
    tx.commit()?;

    // TODO(Task 8): 冲正凭证联动——在冲正事务中生成反向凭证（写 fund_account_id 辅助核算），
    // 原凭证保留 active 并建立追溯关系（spec 4.7/5.1，Task 8 验收项）。
    get_fund_document(conn, reversal_id)
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
/// 实体存在性与状态校验见 `ensure_attachment_entity_exists_and_editable`（add 路径）
/// 与 `ensure_attachment_entity_editable`（删除路径）。
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
/// 实体门禁：目标实体必须存在且处于可编辑状态（未提交审批），
/// 防止向 submitted/approved 实体挂附件后因删除门禁形成"不可删死锁"（Task 5 挂账承接）。
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
    // 实体门禁（在文件归档前快速失败）：实体必须存在且未提交审批
    ensure_attachment_entity_exists_and_editable(conn, &input.entity_type, input.entity_id)?;
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
/// - reimbursement_claim：仅 draft/rejected/void 可删；记录已不存在视为孤儿附件，允许清理。
/// - fund_document：仅 draft/rejected/void 可删；单据已不存在视为孤儿附件，允许清理。
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
        "fund_document" => {
            let status = get_fund_document_status(conn, att.entity_id)?;
            match status {
                Some(s) => ensure_fund_document_attachment_editable(att.entity_id, &s),
                None => Ok(()), // 单据已不存在：视为孤儿附件，允许清理
            }
        }
        _ => Ok(()),
    }
}

/// 附件挂载实体门禁（add 路径）：目标实体必须存在，且处于可编辑状态（未提交审批）。
/// 与删除门禁共用同一套可编辑状态口径，保证"能挂上的都能删、挂不上的不产生死锁"
/// （Task 5 挂账承接：附件 add 路径缺实体存在性/状态门禁）。
fn ensure_attachment_entity_exists_and_editable(
    conn: &Connection,
    entity_type: &str,
    entity_id: i64,
) -> AppResult<()> {
    match entity_type {
        "fund_document" => {
            let status = get_fund_document_status(conn, entity_id)?;
            match status {
                None => Err(AppError::NotFound(format!(
                    "资金单据不存在：id={entity_id}，不能挂接附件"
                ))),
                Some(s) => ensure_fund_document_attachment_editable(entity_id, &s),
            }
        }
        "reimbursement_claim" => {
            let status: Option<String> = conn
                .query_row(
                    "SELECT status FROM reimbursement_claims WHERE id = ?1",
                    params![entity_id],
                    |r| r.get(0),
                )
                .optional()?;
            match status {
                None => Err(AppError::NotFound(format!(
                    "报销单不存在：id={entity_id}，不能挂接附件"
                ))),
                Some(s) if REIMBURSEMENT_ATTACHMENT_EDITABLE_STATUSES.contains(&s.as_str()) => {
                    Ok(())
                }
                Some(s) => Err(AppError::General(format!(
                    "报销单已提交审批（状态: {s}），附件须先反审批/驳回后才能挂接或变更"
                ))),
            }
        }
        _ => Err(AppError::InvalidParam(format!(
            "不支持的附件实体类型: {entity_type}"
        ))),
    }
}

/// 资金单附件可变更状态：仅未进入审批流的单据（草稿/已驳回/已作废）。
fn ensure_fund_document_attachment_editable(document_id: i64, status: &str) -> AppResult<()> {
    if FUND_DOCUMENT_ATTACHMENT_EDITABLE_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(AppError::General(format!(
            "资金单据 {document_id} 已进入审批流（状态: {status}），附件须先撤回/驳回/作废单据后才能变更"
        )))
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
        let doc = create_draft_receipt(&conn, &current);
        let plain = b"payment voucher scan \x00\x01\xff attachment content";
        let src = write_source_file(&app_dir, "voucher.pdf", plain);

        let att = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src, "fund_document", doc.id),
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
        let list = list_business_attachments(&conn, "fund_document", doc.id).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, att.id);
        assert!(
            list_business_attachments(&conn, "fund_document", doc.id + 1)
                .unwrap()
                .is_empty()
        );

        let _ = std::fs::remove_dir_all(&app_dir);
    }

    /// DEK 未加载（未解锁）时与发票归档同语义：明文归档，encrypted=0。
    #[test]
    fn test_business_attachment_add_without_dek_stays_plain() {
        let (conn, current, _sec_loaded, app_dir) = attachment_env("add-plain");
        let sec = SecurityState::new(); // 未 setup → 无 DEK
        let doc = create_draft_receipt(&conn, &current);
        let plain = b"plain attachment";
        let src = write_source_file(&app_dir, "note.txt", plain);

        let att = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src, "fund_document", doc.id),
        )
        .unwrap();

        assert!(!att.encrypted);
        assert_eq!(std::fs::read(&att.file_path).unwrap(), plain);

        let _ = std::fs::remove_dir_all(&app_dir);
    }

    /// 错误路径：实体类型枚举、实体 ID、实体存在性、源文件存在性、大小上限、未选操作人。
    #[test]
    fn test_business_attachment_validation_errors() {
        let (conn, current, sec, app_dir) = attachment_env("validation");
        let doc = create_draft_receipt(&conn, &current);
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

        // 实体不存在（Task 6 承接：add 路径实体存在性校验）
        let err = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src, "fund_document", doc.id + 100),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("不存在"), "挂接不存在单据应拦截: {err}");
        let err = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src, "reimbursement_claim", 8888),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("不存在"), "挂接不存在报销单应拦截: {err}");

        // 源文件不存在
        let err = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(
                &app_dir.join("missing.pdf").to_string_lossy(),
                "fund_document",
                doc.id,
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
            &attachment_input(&big, "fund_document", doc.id),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("大小限制"), "超限附件应拦截: {err}");
        // 失败不落库
        assert!(list_business_attachments(&conn, "fund_document", doc.id)
            .unwrap()
            .is_empty());

        // 未选择当前操作人
        let fresh_current = CurrentOperatorState::new();
        let err = add_business_attachment(
            &conn,
            &sec,
            &fresh_current,
            &app_dir,
            &attachment_input(&src, "fund_document", doc.id),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("操作人"), "未选操作人应拦截: {err}");

        let _ = std::fs::remove_dir_all(&app_dir);
    }

    /// 删除门禁：未提交报销单可删（文件+DB 行一并清理）；已提交/已审批的报销单拦截；
    /// 资金单据同样受状态门禁（详见 test_business_attachment_fund_document_entity_gates）。
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

        let _ = std::fs::remove_dir_all(&app_dir);
    }

    /// 补偿清理：DB 写入失败（file_path UNIQUE 冲突）时，已归档文件与加密临时文件
    /// 必须被清理，不留孤儿文件。用固定时间戳的 impl 入口保证目标路径可预测。
    #[test]
    fn test_business_attachment_add_db_failure_cleans_up_file() {
        use chrono::TimeZone;

        let (conn, current, sec, app_dir) = attachment_env("db-fail");
        let doc = create_draft_receipt(&conn, &current);
        let src = write_source_file(&app_dir, "dup.pdf", b"dup");
        let input = attachment_input(&src, "fund_document", doc.id);

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
             VALUES ('fund_document', ?2, 'dup.pdf', ?1, 0, 3, '2026-08', NULL, 'now')",
            params![expected_target.to_string_lossy(), doc.id],
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
            list_business_attachments(&conn, "fund_document", doc.id)
                .unwrap()
                .len(),
            1
        );

        let _ = std::fs::remove_dir_all(&app_dir);
    }

    // ---------- 资金单据与状态机 ----------

    /// 资金单测试环境：财务库 + 当前操作人张会计（cashier）
    fn fund_doc_env() -> (Connection, CurrentOperatorState) {
        let conn = setup_financial_db();
        let current = CurrentOperatorState::new();
        let op = save_operator_profile(&conn, &current, &operator_input("张会计", "cashier"))
            .unwrap()
            .0;
        set_current_operator(&conn, &current, op.id).unwrap();
        (conn, current)
    }

    struct DocFixtures {
        bank: FundAccount,
        cash: FundAccount,
        partner: BusinessPartner,
    }

    /// 标准引用对象：银行账户 + 现金账户 + 供应商
    fn setup_doc_fixtures(conn: &Connection) -> DocFixtures {
        let bank = save_fund_account(conn, &bank_input("BANK-001", "基本户", "622200001")).unwrap();
        let cash = save_fund_account(conn, &cash_input("CASH-001", "现金库")).unwrap();
        let partner = save_business_partner(
            conn,
            &partner_input("GYS-001", "供应商甲", Some("91110000X")),
        )
        .unwrap();
        DocFixtures {
            bank,
            cash,
            partner,
        }
    }

    fn doc_input(doc_type: &str) -> FundDocumentInput {
        FundDocumentInput {
            id: None,
            document_type: doc_type.into(),
            belong_month: "2026-08".into(),
            document_date: "2026-08-05".into(),
            amount: 500.0,
            summary: "测试单据".into(),
            department: None,
            expense_type: None,
            remark: None,
            partner_id: None,
            employee_id: None,
            source_account_id: None,
            target_account_id: None,
            counter_account_code: None,
        }
    }

    fn receipt_input(fx: &DocFixtures) -> FundDocumentInput {
        FundDocumentInput {
            target_account_id: Some(fx.bank.id),
            partner_id: Some(fx.partner.id),
            counter_account_code: Some("1122".into()),
            ..doc_input("receipt")
        }
    }

    fn payment_input(fx: &DocFixtures) -> FundDocumentInput {
        FundDocumentInput {
            source_account_id: Some(fx.bank.id),
            partner_id: Some(fx.partner.id),
            counter_account_code: Some("2202".into()),
            ..doc_input("payment")
        }
    }

    fn transfer_input(fx: &DocFixtures) -> FundDocumentInput {
        FundDocumentInput {
            source_account_id: Some(fx.bank.id),
            target_account_id: Some(fx.cash.id),
            ..doc_input("transfer")
        }
    }

    fn advance_input(fx: &DocFixtures) -> FundDocumentInput {
        FundDocumentInput {
            source_account_id: Some(fx.bank.id),
            employee_id: Some(1),
            summary: "员工出差借款".into(),
            ..doc_input("advance")
        }
    }

    fn settlement_input(fx: &DocFixtures) -> FundDocumentInput {
        FundDocumentInput {
            target_account_id: Some(fx.cash.id),
            employee_id: Some(1),
            summary: "借款核销退款".into(),
            ..doc_input("advance_settlement")
        }
    }

    /// 创建一张草稿收款单（附件门禁等测试用）
    fn create_draft_receipt(conn: &Connection, current: &CurrentOperatorState) -> FundDocument {
        let fx = setup_doc_fixtures(conn);
        create_fund_document(conn, current, &receipt_input(&fx)).unwrap()
    }

    /// 直插一条已月结记录（绕过月结工作台检查，专测月结保护）
    fn close_month_direct(conn: &Connection, month: &str) {
        conn.execute(
            "INSERT INTO month_closes (month, status, created_at, updated_at)
             VALUES (?1, 'closed', '2026-09-01', '2026-09-01')",
            params![month],
        )
        .unwrap();
    }

    fn reverse_input(document_id: i64, month: &str, date: &str) -> FundDocumentReverseInput {
        FundDocumentReverseInput {
            document_id,
            belong_month: month.into(),
            document_date: date.into(),
            comment: "收款错误，需冲正".into(),
        }
    }

    /// 五类单据合法创建（编号前缀按类型）+ 各类型账户方向/往来对象规则 + 草稿可编辑。
    #[test]
    fn test_fund_document_create_update_and_type_direction_rules() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);

        let receipt = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        assert_eq!(receipt.status, "draft");
        assert!(
            receipt.document_no.starts_with("SK20260805"),
            "收款单编号: {}",
            receipt.document_no
        );
        let payment = create_fund_document(&conn, &current, &payment_input(&fx)).unwrap();
        assert!(payment.document_no.starts_with("FK"));
        let transfer = create_fund_document(&conn, &current, &transfer_input(&fx)).unwrap();
        assert!(transfer.document_no.starts_with("NB"));
        let advance = create_fund_document(&conn, &current, &advance_input(&fx)).unwrap();
        assert!(advance.document_no.starts_with("JK"));
        let settlement = create_fund_document(&conn, &current, &settlement_input(&fx)).unwrap();
        assert!(settlement.document_no.starts_with("HX"));

        // 收款单不能带来源账户
        let mut bad = receipt_input(&fx);
        bad.source_account_id = Some(fx.bank.id);
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("收款单"));
        // 付款单不能带目标账户
        let mut bad = payment_input(&fx);
        bad.target_account_id = Some(fx.cash.id);
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("付款单"));
        // 转账缺目标账户
        let mut bad = transfer_input(&fx);
        bad.target_account_id = None;
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("内部转账单"));
        // 转账两侧账户相同
        let mut same = transfer_input(&fx);
        same.target_account_id = Some(fx.bank.id);
        assert!(create_fund_document(&conn, &current, &same)
            .unwrap_err()
            .to_string()
            .contains("不能相同"));
        // 转账不需要往来对象
        let mut with_partner = transfer_input(&fx);
        with_partner.partner_id = Some(fx.partner.id);
        assert!(create_fund_document(&conn, &current, &with_partner)
            .unwrap_err()
            .to_string()
            .contains("往来单位"));
        // 借款单必须员工、不能往来单位
        let mut bad = advance_input(&fx);
        bad.employee_id = None;
        bad.partner_id = Some(fx.partner.id);
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("员工借款单"));
        // 核销单不能带来源账户
        let mut bad = settlement_input(&fx);
        bad.source_account_id = Some(fx.cash.id);
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("借款核销单"));
        // 收款/付款往来对象二选一：缺一 / 同时两者都拦截
        let mut no_cp = receipt_input(&fx);
        no_cp.partner_id = None;
        assert!(create_fund_document(&conn, &current, &no_cp)
            .unwrap_err()
            .to_string()
            .contains("必须选择"));
        let mut both = receipt_input(&fx);
        both.employee_id = Some(2);
        assert!(create_fund_document(&conn, &current, &both)
            .unwrap_err()
            .to_string()
            .contains("二选一"));
        // 冲正单不能手工创建
        assert!(
            create_fund_document(&conn, &current, &doc_input("reversal"))
                .unwrap_err()
                .to_string()
                .contains("冲正")
        );
        // 创建传 id 拦截
        let mut with_id = receipt_input(&fx);
        with_id.id = Some(1);
        assert!(create_fund_document(&conn, &current, &with_id)
            .unwrap_err()
            .to_string()
            .contains("id"));

        // 草稿可编辑：金额/摘要生效、状态保持草稿、状态字段无法从入参注入
        let mut edit = receipt_input(&fx);
        edit.id = Some(receipt.id);
        edit.amount = 800.0;
        edit.summary = "改后的摘要".into();
        let edited = update_fund_document(&conn, &current, &edit).unwrap();
        assert_eq!(edited.amount, 800.0);
        assert_eq!(edited.summary, "改后的摘要");
        assert_eq!(edited.status, "draft");
        // 更新必须带 id
        assert!(update_fund_document(&conn, &current, &receipt_input(&fx)).is_err());
    }

    /// 内容校验：金额正数（容差）、摘要、月份/日期格式与匹配、对方科目必填/存在、
    /// 引用对象存在且启用。
    #[test]
    fn test_fund_document_content_validation_errors() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);

        let mut bad = receipt_input(&fx);
        bad.amount = 0.0;
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("正数"));
        bad.amount = -10.0;
        assert!(create_fund_document(&conn, &current, &bad).is_err());
        // 低于容差按零金额拦截
        bad.amount = 0.003;
        assert!(create_fund_document(&conn, &current, &bad).is_err());

        let mut bad = receipt_input(&fx);
        bad.summary = "  ".into();
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("摘要"));

        let mut bad = receipt_input(&fx);
        bad.belong_month = "2026/08".into();
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("YYYY-MM"));

        let mut bad = receipt_input(&fx);
        bad.document_date = "2026-09-01".into();
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("归属月份"));

        // 收款/付款对方科目必填且必须存在；转账对方科目可空
        let mut bad = payment_input(&fx);
        bad.counter_account_code = None;
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("对方科目"));
        let mut bad = payment_input(&fx);
        bad.counter_account_code = Some("9999".into());
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("不存在"));
        assert!(create_fund_document(&conn, &current, &transfer_input(&fx)).is_ok());

        // 引用对象不存在
        let mut bad = receipt_input(&fx);
        bad.partner_id = Some(999);
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("不存在"));
        let mut bad = advance_input(&fx);
        bad.employee_id = Some(999);
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("不存在"));

        // 停用账户不能用于新单据
        set_active_fund_account(&conn, fx.bank.id, false).unwrap();
        let mut bad = receipt_input(&fx);
        bad.target_account_id = Some(fx.bank.id);
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("停用"));
        set_active_fund_account(&conn, fx.bank.id, true).unwrap();

        // 停用往来单位不能用于新单据
        set_active_business_partner(&conn, fx.partner.id, false).unwrap();
        let bad = receipt_input(&fx);
        assert!(create_fund_document(&conn, &current, &bad)
            .unwrap_err()
            .to_string()
            .contains("停用"));
    }

    /// 允许路径全走通：直接结算（收款）、批次结算（付款）、撤回重提、驳回→撤回→修改→重走、作废。
    #[test]
    fn test_fund_document_state_machine_allowed_paths() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);

        // 路径1：收款单 draft → submitted → approved → settled（直接结算）
        let receipt = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        let submitted = submit_fund_document(&conn, &current, receipt.id, None).unwrap();
        assert_eq!(submitted.status, "submitted");
        assert!(submitted.submitted_by.is_some() && submitted.submitted_at.is_some());
        let approved = approve_fund_document(&conn, &current, receipt.id, "同意").unwrap();
        assert_eq!(approved.status, "approved");
        assert!(approved.approved_by.is_some() && approved.approved_at.is_some());
        let settled = settle_fund_document(&conn, &current, receipt.id).unwrap();
        assert_eq!(settled.status, "settled");
        assert!(settled.settled_by.is_some() && settled.settled_at.is_some());

        // 路径2：付款单经批次 draft → submitted → approved → batched → settled
        let payment = create_fund_document(&conn, &current, &payment_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, payment.id, None).unwrap();
        approve_fund_document(&conn, &current, payment.id, "同意").unwrap();
        conn.execute(
            "INSERT INTO payment_batches (batch_no, belong_month, batch_type, status, total_amount, item_count, created_at, updated_at)
             VALUES ('ZF-TEST-1', '2026-08', 'general', 'pending', 500, 1, '2026-08-05', '2026-08-05')",
            [],
        )
        .unwrap();
        let batch_id = conn.last_insert_rowid();
        let batched = mark_document_batched(&conn, &current, payment.id, batch_id).unwrap();
        assert_eq!(batched.status, "batched");
        assert_eq!(batched.payment_batch_id, Some(batch_id));
        let settled = settle_fund_document(&conn, &current, payment.id).unwrap();
        assert_eq!(settled.status, "settled");

        // 路径3：submitted 撤回 → draft 重新提交
        let transfer = create_fund_document(&conn, &current, &transfer_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, transfer.id, None).unwrap();
        let withdrawn = withdraw_fund_document(&conn, &current, transfer.id, None).unwrap();
        assert_eq!(withdrawn.status, "draft");
        let resubmitted = submit_fund_document(&conn, &current, transfer.id, None).unwrap();
        assert_eq!(resubmitted.status, "submitted");

        // 路径4：驳回 → 撤回 → 修改 → 重新提交 → 审批
        let rejected = reject_fund_document(&conn, &current, transfer.id, "金额不对").unwrap();
        assert_eq!(rejected.status, "rejected");
        let withdrawn =
            withdraw_fund_document(&conn, &current, transfer.id, Some("按意见修改")).unwrap();
        assert_eq!(withdrawn.status, "draft");
        let mut edit = transfer_input(&fx);
        edit.id = Some(transfer.id);
        edit.amount = 300.0;
        update_fund_document(&conn, &current, &edit).unwrap();
        submit_fund_document(&conn, &current, transfer.id, None).unwrap();
        approve_fund_document(&conn, &current, transfer.id, "通过").unwrap();

        // 路径5：作废（草稿/已审批均可，仅未结算）
        let v1 = create_fund_document(&conn, &current, &advance_input(&fx)).unwrap();
        let voided = void_fund_document(&conn, &current, v1.id, "不需要了").unwrap();
        assert_eq!(voided.status, "void");
        assert!(voided.voided_by.is_some() && voided.voided_at.is_some());
        let v2 = create_fund_document(&conn, &current, &settlement_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, v2.id, None).unwrap();
        approve_fund_document(&conn, &current, v2.id, "ok").unwrap();
        assert_eq!(
            void_fund_document(&conn, &current, v2.id, "重复提交")
                .unwrap()
                .status,
            "void"
        );
    }

    /// 禁止路径矩阵：非法来源状态、类型限制、必填意见。
    #[test]
    fn test_fund_document_state_machine_forbidden_transitions() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);

        // --- draft：不能审批/驳回/结算/撤回/进批次/冲正 ---
        let d = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        assert!(approve_fund_document(&conn, &current, d.id, "x")
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(reject_fund_document(&conn, &current, d.id, "x")
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(settle_fund_document(&conn, &current, d.id)
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(withdraw_fund_document(&conn, &current, d.id, None)
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(mark_document_batched(&conn, &current, d.id, 1)
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(reverse_fund_document(
            &conn,
            &current,
            &reverse_input(d.id, "2026-08", "2026-08-20")
        )
        .unwrap_err()
        .to_string()
        .contains("仅已结算"));

        // --- submitted：不能重复提交/结算/进批次/冲正 ---
        let s = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, s.id, None).unwrap();
        assert!(submit_fund_document(&conn, &current, s.id, None)
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(settle_fund_document(&conn, &current, s.id)
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(mark_document_batched(&conn, &current, s.id, 1)
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(reverse_fund_document(
            &conn,
            &current,
            &reverse_input(s.id, "2026-08", "2026-08-20")
        )
        .unwrap_err()
        .to_string()
        .contains("仅已结算"));

        // --- approved：收款单不能进批次；付款单审批后不能直接结算（须经批次）---
        let a = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, a.id, None).unwrap();
        approve_fund_document(&conn, &current, a.id, "ok").unwrap();
        assert!(submit_fund_document(&conn, &current, a.id, None)
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(withdraw_fund_document(&conn, &current, a.id, None)
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(reject_fund_document(&conn, &current, a.id, "x")
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(mark_document_batched(&conn, &current, a.id, 1)
            .unwrap_err()
            .to_string()
            .contains("收款单"));
        let mut pay = payment_input(&fx);
        pay.summary = "付款单直接结算应被拦".into();
        let p = create_fund_document(&conn, &current, &pay).unwrap();
        submit_fund_document(&conn, &current, p.id, None).unwrap();
        approve_fund_document(&conn, &current, p.id, "ok").unwrap();
        let err = settle_fund_document(&conn, &current, p.id)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("付款批次"),
            "付款单审批后直接结算应拦截：{err}"
        );

        // --- batched：不能作废（须批次作废释放）、不能重复进批次 ---
        conn.execute(
            "INSERT INTO payment_batches (batch_no, belong_month, batch_type, status, total_amount, item_count, created_at, updated_at)
             VALUES ('ZF-TEST-2', '2026-08', 'general', 'pending', 500, 1, '2026-08-05', '2026-08-05')",
            [],
        )
        .unwrap();
        let batch_id = conn.last_insert_rowid();
        let b = mark_document_batched(&conn, &current, p.id, batch_id).unwrap();
        assert_eq!(b.status, "batched");
        assert!(void_fund_document(&conn, &current, p.id, "x")
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(mark_document_batched(&conn, &current, p.id, batch_id)
            .unwrap_err()
            .to_string()
            .contains("不允许"));

        // --- settled：不能作废/重复结算；只能冲正 ---
        let st = settle_fund_document(&conn, &current, p.id).unwrap();
        assert_eq!(st.status, "settled");
        assert!(void_fund_document(&conn, &current, p.id, "x")
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(settle_fund_document(&conn, &current, p.id)
            .unwrap_err()
            .to_string()
            .contains("不允许"));

        // --- void / rejected / reversed 终态与半终态 ---
        let v = create_fund_document(&conn, &current, &advance_input(&fx)).unwrap();
        void_fund_document(&conn, &current, v.id, "作废").unwrap();
        assert!(submit_fund_document(&conn, &current, v.id, None)
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(settle_fund_document(&conn, &current, v.id)
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        let rj = create_fund_document(&conn, &current, &settlement_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, rj.id, None).unwrap();
        reject_fund_document(&conn, &current, rj.id, "驳回").unwrap();
        assert!(approve_fund_document(&conn, &current, rj.id, "x")
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(settle_fund_document(&conn, &current, rj.id)
            .unwrap_err()
            .to_string()
            .contains("不允许"));

        // --- 必填意见：approve/reject/void/reverse 空意见拦截 ---
        let c = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, c.id, None).unwrap();
        assert!(approve_fund_document(&conn, &current, c.id, "  ")
            .unwrap_err()
            .to_string()
            .contains("意见"));
        assert!(reject_fund_document(&conn, &current, c.id, "")
            .unwrap_err()
            .to_string()
            .contains("意见"));
        assert!(void_fund_document(&conn, &current, c.id, "")
            .unwrap_err()
            .to_string()
            .contains("意见"));
        let mut no_reason = reverse_input(c.id, "2026-08", "2026-08-20");
        no_reason.comment = " ".into();
        assert!(reverse_fund_document(&conn, &current, &no_reason)
            .unwrap_err()
            .to_string()
            .contains("原因"));

        // --- 进批次引用不存在的批次 ---
        let pb = create_fund_document(&conn, &current, &payment_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, pb.id, None).unwrap();
        approve_fund_document(&conn, &current, pb.id, "ok").unwrap();
        assert!(mark_document_batched(&conn, &current, pb.id, 999)
            .unwrap_err()
            .to_string()
            .contains("不存在"));
    }

    /// 审批事件追加式轨迹：详情按时间升序重放完整历史；失败操作不产生事件；实体间隔离。
    #[test]
    fn test_fund_document_approval_events_replay() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);
        let doc = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        // 创建不产生事件（draft 为初始态）
        assert!(get_fund_document_detail(&conn, doc.id)
            .unwrap()
            .events
            .is_empty());

        submit_fund_document(&conn, &current, doc.id, None).unwrap();
        approve_fund_document(&conn, &current, doc.id, "审批同意").unwrap();
        settle_fund_document(&conn, &current, doc.id).unwrap();

        let detail = get_fund_document_detail(&conn, doc.id).unwrap();
        let actions: Vec<&str> = detail.events.iter().map(|e| e.action.as_str()).collect();
        assert_eq!(actions, vec!["submit", "approve", "settle"]);
        // id 升序 = 时间重放顺序（spec 4.5）
        let ids: Vec<i64> = detail.events.iter().map(|e| e.id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
        // 事件字段：状态迁移、意见、署名
        assert_eq!(detail.events[1].from_status.as_deref(), Some("submitted"));
        assert_eq!(detail.events[1].to_status.as_deref(), Some("approved"));
        assert_eq!(detail.events[1].comment.as_deref(), Some("审批同意"));
        assert_eq!(detail.events[0].comment, None);
        assert_eq!(detail.events[0].entity_type, "fund_document");
        assert!(detail.events.iter().all(|e| e.operator_id.is_some()));

        // 失败的状态操作不产生事件
        assert!(submit_fund_document(&conn, &current, doc.id, None).is_err());
        assert_eq!(
            get_fund_document_detail(&conn, doc.id)
                .unwrap()
                .events
                .len(),
            3
        );

        // 事件按实体隔离
        let other = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, other.id, None).unwrap();
        assert_eq!(
            get_fund_document_detail(&conn, doc.id)
                .unwrap()
                .events
                .len(),
            3
        );
        assert_eq!(
            get_fund_document_detail(&conn, other.id)
                .unwrap()
                .events
                .len(),
            1
        );
    }

    /// 状态更新与审批事件同事务（spec 5.1）：事件写入失败（action 违反 CHECK）
    /// 时已执行的单据状态更新必须一并回滚。
    #[test]
    fn test_fund_document_transition_atomic_on_event_failure() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);
        let doc = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, doc.id, None).unwrap();

        // 非法 action 触发 approval_events CHECK 约束失败（发生在状态 UPDATE 之后）
        let result = transition_fund_document(
            &conn,
            &current,
            doc.id,
            "bogus_action",
            &["submitted"],
            "approved",
            Some("ok"),
            false,
            |_, _, _, _| Ok(()),
        );
        assert!(result.is_err(), "非法 action 应触发约束失败");
        // 状态未被推进、无事件残留（原 submit 事件仍在）
        let after = get_fund_document_detail(&conn, doc.id).unwrap();
        assert_eq!(after.document.status, "submitted");
        assert_eq!(after.events.len(), 1);
        assert_eq!(after.events[0].action, "submit");
    }

    /// maker_checker（spec 2）：默认关闭；开启后审批人 ≠ 提交人；关闭后恢复。
    #[test]
    fn test_fund_document_maker_checker() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);

        // 默认关闭：同一操作人可提交并审批
        let d1 = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, d1.id, None).unwrap();
        approve_fund_document(&conn, &current, d1.id, "自审").unwrap();

        // 开启后：提交人 = 审批人拦截
        set_maker_checker_enabled(&conn, true).unwrap();
        let d2 = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, d2.id, None).unwrap();
        let err = approve_fund_document(&conn, &current, d2.id, "自审")
            .unwrap_err()
            .to_string();
        assert!(err.contains("经办复核"), "maker_checker 冲突应拦截：{err}");
        // 拦截不产生事件、状态保持 submitted
        assert_eq!(
            get_fund_document_detail(&conn, d2.id).unwrap().events.len(),
            1
        );

        // 切换操作人后可审批
        let reviewer =
            save_operator_profile(&conn, &current, &operator_input("李审批", "approver"))
                .unwrap()
                .0;
        set_current_operator(&conn, &current, reviewer.id).unwrap();
        approve_fund_document(&conn, &current, d2.id, "复核通过").unwrap();

        // 关闭后恢复同一人可自审
        set_maker_checker_enabled(&conn, false).unwrap();
        let d3 = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, d3.id, None).unwrap();
        approve_fund_document(&conn, &current, d3.id, "自审").unwrap();
    }

    /// 月结保护：已月结月份的创建/更新/状态变更全部拦截；
    /// 冲正同时受原单月份与冲正月份月结保护。
    #[test]
    fn test_fund_document_month_close_protection() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);

        // 2026-08 已月结：创建拦截
        close_month_direct(&conn, "2026-08");
        let err = create_fund_document(&conn, &current, &receipt_input(&fx))
            .unwrap_err()
            .to_string();
        assert!(err.contains("月结"), "已月结月份创建应拦截：{err}");

        // 2026-07 开放：可创建、可结算
        let mut july = receipt_input(&fx);
        july.belong_month = "2026-07".into();
        july.document_date = "2026-07-10".into();
        let doc = create_fund_document(&conn, &current, &july).unwrap();
        let mut july_draft = advance_input(&fx);
        july_draft.belong_month = "2026-07".into();
        july_draft.document_date = "2026-07-10".into();
        let draft = create_fund_document(&conn, &current, &july_draft).unwrap();
        submit_fund_document(&conn, &current, doc.id, None).unwrap();
        approve_fund_document(&conn, &current, doc.id, "ok").unwrap();
        settle_fund_document(&conn, &current, doc.id).unwrap();

        // 冲正月份已月结 → 拦截（原月份开放）
        let err = reverse_fund_document(
            &conn,
            &current,
            &reverse_input(doc.id, "2026-08", "2026-08-20"),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("月结"), "冲正月份已月结应拦截：{err}");

        // 原单月份已月结后：草稿不能提交/更新；冲正原月份拦截（冲正月份开放）
        close_month_direct(&conn, "2026-07");
        let err = submit_fund_document(&conn, &current, draft.id, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("月结"), "月结后提交应拦截：{err}");
        let mut edit = july_draft.clone();
        edit.id = Some(draft.id);
        assert!(update_fund_document(&conn, &current, &edit)
            .unwrap_err()
            .to_string()
            .contains("月结"));
        let err = reverse_fund_document(
            &conn,
            &current,
            &reverse_input(doc.id, "2026-09", "2026-09-05"),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("月结"), "原单月份已月结冲正应拦截：{err}");
    }

    /// 冲正：创建反向单并引用原单、原单置 reversed、双向审批事件、终态不可再操作；
    /// 冲正单本身已结算，可再次冲正（纠错闭环）。
    #[test]
    fn test_fund_document_reverse_creates_linked_reversal() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);
        let receipt = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, receipt.id, None).unwrap();
        approve_fund_document(&conn, &current, receipt.id, "ok").unwrap();
        settle_fund_document(&conn, &current, receipt.id).unwrap();

        let reversal = reverse_fund_document(
            &conn,
            &current,
            &reverse_input(receipt.id, "2026-08", "2026-08-20"),
        )
        .unwrap();

        // 冲正单：类型 reversal、立即结算、引用原单、账户反向、金额/对象/科目随原单
        assert_eq!(reversal.document_type, "reversal");
        assert_eq!(reversal.status, "settled");
        assert_eq!(reversal.reversal_of_id, Some(receipt.id));
        assert_eq!(reversal.amount, receipt.amount);
        assert_eq!(reversal.source_account_id, receipt.target_account_id);
        assert_eq!(reversal.target_account_id, receipt.source_account_id);
        assert_eq!(reversal.partner_id, receipt.partner_id);
        assert_eq!(reversal.counter_account_code, receipt.counter_account_code);
        assert!(reversal.summary.starts_with("冲正："));
        assert!(reversal
            .remark
            .as_deref()
            .unwrap_or("")
            .contains(&receipt.document_no));
        assert!(reversal.document_no.starts_with("CZ"));
        assert!(reversal.settled_by.is_some() && reversal.settled_at.is_some());

        // 原单置为已冲正（终态）：不可作废/结算/再冲正
        let original = get_fund_document_detail(&conn, receipt.id).unwrap();
        assert_eq!(original.document.status, "reversed");
        assert!(void_fund_document(&conn, &current, receipt.id, "x").is_err());
        assert!(settle_fund_document(&conn, &current, receipt.id).is_err());
        assert!(reverse_fund_document(
            &conn,
            &current,
            &reverse_input(receipt.id, "2026-08", "2026-08-21")
        )
        .is_err());

        // 双向审批事件（原单 settled→reversed；冲正单 →settled，均带原因）
        let orig_actions: Vec<&str> = original.events.iter().map(|e| e.action.as_str()).collect();
        assert_eq!(orig_actions, vec!["submit", "approve", "settle", "reverse"]);
        assert_eq!(original.events[3].from_status.as_deref(), Some("settled"));
        assert_eq!(original.events[3].to_status.as_deref(), Some("reversed"));
        assert_eq!(
            original.events[3].comment.as_deref(),
            Some("收款错误，需冲正")
        );
        let rev_detail = get_fund_document_detail(&conn, reversal.id).unwrap();
        assert_eq!(rev_detail.events.len(), 1);
        assert_eq!(rev_detail.events[0].action, "reverse");
        assert_eq!(rev_detail.events[0].to_status.as_deref(), Some("settled"));
        assert_eq!(
            rev_detail.events[0].comment.as_deref(),
            Some("收款错误，需冲正")
        );

        // 冲正单本身已结算：可再次冲正（冲正的冲正），账户再次反向回到原方向
        let reversal2 = reverse_fund_document(
            &conn,
            &current,
            &reverse_input(reversal.id, "2026-08", "2026-08-25"),
        )
        .unwrap();
        assert_eq!(reversal2.reversal_of_id, Some(reversal.id));
        assert_eq!(reversal2.source_account_id, reversal.target_account_id);
        assert_eq!(reversal2.target_account_id, reversal.source_account_id);
    }

    /// 查询过滤与详情：月份/类型/状态/关键字/往来对象；详情=单据+轨迹；不存在报错。
    #[test]
    fn test_fund_document_query_filters() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);
        let r1 = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        let mut p = payment_input(&fx);
        p.belong_month = "2026-07".into();
        p.document_date = "2026-07-10".into();
        let p1 = create_fund_document(&conn, &current, &p).unwrap();
        submit_fund_document(&conn, &current, r1.id, None).unwrap();

        assert_eq!(
            get_fund_documents(&conn, &FundDocumentQuery::default())
                .unwrap()
                .len(),
            2
        );
        let by_type = get_fund_documents(
            &conn,
            &FundDocumentQuery {
                document_type: Some("receipt".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_type.len(), 1);
        assert_eq!(by_type[0].document_no.starts_with("SK"), true);
        let by_month = get_fund_documents(
            &conn,
            &FundDocumentQuery {
                belong_month: Some("2026-07".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_month.len(), 1);
        assert_eq!(by_month[0].id, p1.id);
        let by_status = get_fund_documents(
            &conn,
            &FundDocumentQuery {
                status: Some("submitted".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_status.len(), 1);
        assert_eq!(by_status[0].id, r1.id);
        let by_kw = get_fund_documents(
            &conn,
            &FundDocumentQuery {
                keyword: Some("测试".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_kw.len(), 2);
        let by_partner = get_fund_documents(
            &conn,
            &FundDocumentQuery {
                partner_id: Some(fx.partner.id),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_partner.len(), 2);

        // 详情 = 单据 + 轨迹；不存在报错
        let detail = get_fund_document_detail(&conn, r1.id).unwrap();
        assert_eq!(detail.document.id, r1.id);
        assert_eq!(detail.events.len(), 1);
        assert!(get_fund_document_detail(&conn, 999).is_err());
    }

    /// 挂账承接（Task 3）：往来单位被资金单据引用后只允许停用，不允许改类型；
    /// 其他字段编辑与停用/启用不受限；解除引用后可改类型。
    #[test]
    fn test_business_partner_type_change_blocked_when_referenced() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);
        let doc = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();

        // 被引用：改类型拦截
        let mut change = partner_input("GYS-001", "供应商甲", Some("91110000X"));
        change.id = Some(fx.partner.id);
        change.partner_type = "customer".into();
        let err = save_business_partner(&conn, &change)
            .unwrap_err()
            .to_string();
        assert!(err.contains("引用"), "被引用改类型应拦截：{err}");

        // 改名称等其他字段仍允许
        let mut rename = partner_input("GYS-001", "供应商甲改", Some("91110000X"));
        rename.id = Some(fx.partner.id);
        let renamed = save_business_partner(&conn, &rename).unwrap();
        assert_eq!(renamed.name, "供应商甲改");

        // 停用/启用允许（spec 4.2：被引用后只允许停用）
        assert_eq!(
            set_active_business_partner(&conn, fx.partner.id, false)
                .unwrap()
                .status,
            "inactive"
        );
        assert_eq!(
            set_active_business_partner(&conn, fx.partner.id, true)
                .unwrap()
                .status,
            "active"
        );

        // 未被引用的单位改类型允许
        let free = save_business_partner(&conn, &partner_input("GYS-002", "客户乙", None)).unwrap();
        let mut change = partner_input("GYS-002", "客户乙", None);
        change.id = Some(free.id);
        change.partner_type = "customer".into();
        assert_eq!(
            save_business_partner(&conn, &change).unwrap().partner_type,
            "customer"
        );

        // 解除引用后可改类型（模拟数据修正：直接删除引用单据）
        conn.execute("DELETE FROM fund_documents WHERE id = ?1", params![doc.id])
            .unwrap();
        let mut change = partner_input("GYS-001", "供应商甲改", Some("91110000X"));
        change.id = Some(fx.partner.id);
        change.partner_type = "other".into();
        assert_eq!(
            save_business_partner(&conn, &change).unwrap().partner_type,
            "other"
        );
    }

    /// 挂账承接（Task 5）：附件 add 路径实体存在性 + 状态门禁；删除门禁覆盖资金单。
    #[test]
    fn test_business_attachment_fund_document_entity_gates() {
        let (conn, current, sec, app_dir) = attachment_env("fund-gates");
        let doc = create_draft_receipt(&conn, &current);
        let src = write_source_file(&app_dir, "pay.pdf", b"pay");
        let att = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src, "fund_document", doc.id),
        )
        .unwrap();

        // 提交后：新挂拦截 + 已挂附件删除拦截（防"不可删死锁"）
        submit_fund_document(&conn, &current, doc.id, None).unwrap();
        let err = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src, "fund_document", doc.id),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("审批流"), "已提交单据新挂附件应拦截: {err}");
        let err = delete_business_attachment(&conn, &current, att.id)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("撤回") || err.contains("驳回"),
            "已提交单据附件删除应拦截: {err}"
        );
        assert!(
            std::path::Path::new(&att.file_path).exists(),
            "拦截后文件必须保留"
        );

        // 撤回回草稿后可删（文件与 DB 行一并清理）
        withdraw_fund_document(&conn, &current, doc.id, None).unwrap();
        assert_eq!(
            delete_business_attachment(&conn, &current, att.id).unwrap(),
            att.file_name
        );

        // 孤儿附件（单据已不存在）允许清理
        let att2 = add_business_attachment(
            &conn,
            &sec,
            &current,
            &app_dir,
            &attachment_input(&src, "fund_document", doc.id),
        )
        .unwrap();
        conn.execute("DELETE FROM fund_documents WHERE id = ?1", params![doc.id])
            .unwrap();
        delete_business_attachment(&conn, &current, att2.id).unwrap();
        assert!(!std::path::Path::new(&att2.file_path).exists());

        let _ = std::fs::remove_dir_all(&app_dir);
    }
}
