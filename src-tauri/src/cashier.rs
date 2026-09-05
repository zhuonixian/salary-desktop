//! 第七阶段出纳领域模块（cashier）：资金账户 / 往来单位 / 操作人基础资料、当前操作人会话、
//! 通用资金单据状态机与追加式审批事件。
//!
//! 模块边界（spec 第 10 节）：
//! - 本模块负责出纳主数据、资金单据（fund_documents）命令驱动状态机与审批轨迹；
//!   后续批次在此追加资金日记账、银行对账与借款核销。
//! - `db.rs` 只保留 schema/迁移与低层通用 helper；凭证引擎与报表归 `accounting.rs`，
//!   资金单分录规则（结算/冲正凭证生成，spec 4.7）在本模块于状态机事务内调用；
//! - `commands.rs` 负责 State 管理、文件对话框参数与日志编排。
//!
//! 基础资料为主数据：不受 `ensure_month_open` 月结保护（spec 4.4 仅资金单据受月结限制）。
//! 金额一律正数存储，比较容差 0.005；更新入参 `Option` 字段为 patch 语义（None=保留原值，
//! `Some("")`=清空）；错误信息统一中文。
//! 资金单据状态只能经命令流转（submit/approve/reject/withdraw/void/mark_batched/settle/reverse），
//! 状态更新、凭证生成与 approval_events 追加同事务；前端不得直接编辑状态字段。

use std::sync::Mutex;

use crate::accounting;
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

pub(crate) fn get_fund_account(conn: &Connection, id: i64) -> AppResult<FundAccount> {
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

// ==================== 历史资金归集向导（Task 10，spec 9） ====================
//
// 迁移已建资金账户框架但历史数据 fund_account_id 全 NULL。归集维度（spec 9.3/9.5）：
// - 旧银行流水按账户归集（可按归属月圈范围），并联动其 active bank_manual 凭证资金分录；
// - 旧付款批次按批次归集，并联动其 active salary/reimbursement_payment 凭证资金分录；
// - 无法唯一确定归属的资金分录保持 NULL（不猜测），计入 unlinked_voucher_lines 供展示。
// 幂等：UPDATE 一律带 `fund_account_id IS NULL` 条件，已归集数据自动跳过；重复归集指定
// 批次时明确报错（优于静默空操作）。写入前逐一 `ensure_month_open`（月结保护），全程
// 单事务 + 归集后刷新 `stage7_migration_*` 计数，操作由命令层记 operation_logs 审计。

/// 归集支持的对象类型
pub const FUND_ASSIGNMENT_ENTITIES: &[&str] = &["bank_transaction", "payment_batch"];

/// 最后一次归集时间键（app_settings）
const STAGE7_FUND_ASSIGNMENT_LAST_APPLIED_KEY: &str = "stage7_fund_assignment_last_applied_at";

/// 待归集资金分录公共条件（口径与 db::build_stage7_report 一致：排除 void 凭证）
fn fund_line_where(alias: &str) -> String {
    let codes = db::STAGE7_FUND_GL_CODES.join("','");
    format!(
        "{alias}.fund_account_id IS NULL
         AND ({alias}.debit_amount > 0 OR {alias}.credit_amount > 0)
         AND {alias}.account_code IN ('{codes}')"
    )
}

/// 归集目标账户校验：须存在且启用（与全应用资金账户选择口径一致），返回账户
fn migration_target_account(conn: &Connection, account_id: i64) -> AppResult<FundAccount> {
    let account = get_fund_account(conn, account_id)?;
    if !account.is_active {
        return Err(AppError::InvalidParam(format!(
            "资金账户 {} {} 已停用，不能作为归集目标",
            account.account_code, account.name
        )));
    }
    Ok(account)
}

/// 校验归集对象类型
fn ensure_assignment_entity(entity_type: &str) -> AppResult<()> {
    if !FUND_ASSIGNMENT_ENTITIES.contains(&entity_type) {
        return Err(AppError::InvalidParam(format!(
            "不支持的归集对象类型 {entity_type}（支持：{}）",
            FUND_ASSIGNMENT_ENTITIES.join(" / ")
        )));
    }
    Ok(())
}

/// 历史归集实时状态：待归集计数（排除 void）、按月分组、待归集批次与独立分录数
pub fn get_fund_migration_status(conn: &Connection) -> AppResult<FundMigrationStatus> {
    let report = db::build_stage7_report(conn)?;

    // 银行流水与资金分录按归属月合并分组（分录月份取凭证归属月）
    let mut months: std::collections::BTreeMap<String, (i64, i64)> =
        std::collections::BTreeMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT belong_month, COUNT(*) FROM bank_transactions
             WHERE fund_account_id IS NULL GROUP BY belong_month ORDER BY belong_month",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (month, count) = row?;
            months.entry(month).or_insert((0, 0)).0 = count;
        }
    }
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT v.belong_month, COUNT(*) FROM voucher_lines vl
             JOIN vouchers v ON v.id = vl.voucher_id
             WHERE {} AND v.status != 'void'
             GROUP BY v.belong_month ORDER BY v.belong_month",
            fund_line_where("vl")
        ))?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (month, count) = row?;
            months.entry(month).or_insert((0, 0)).1 = count;
        }
    }
    let bank_months = months
        .into_iter()
        .map(
            |(belong_month, (bank_transactions, voucher_lines))| FundMigrationMonthStat {
                belong_month,
                bank_transactions,
                voucher_lines,
            },
        )
        .collect();

    // 待归集批次清单（非 void、账户为空）
    let pending_batches = pending_payment_batches(conn, None)?;

    // 独立分录数 = 待归集分录 - 可经批次/流水联动分录（active 凭证且来源对象仍在待归集）
    let linked_voucher_lines: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM voucher_lines vl
             JOIN vouchers v ON v.id = vl.voucher_id
             LEFT JOIN payment_batches pb
               ON v.source_type IN ('salary_payment','reimbursement_payment')
              AND pb.id = v.source_id
             LEFT JOIN bank_transactions bt
               ON v.source_type = 'bank_manual' AND bt.id = v.source_id
             WHERE {fund_lines}
               AND v.status = 'active'
               AND ((pb.id IS NOT NULL AND pb.fund_account_id IS NULL AND pb.status != 'void')
                 OR (bt.id IS NOT NULL AND bt.fund_account_id IS NULL))",
            fund_lines = fund_line_where("vl")
        ),
        [],
        |r| r.get(0),
    )?;
    let unlinked_voucher_lines = (report.unassigned_voucher_lines - linked_voucher_lines).max(0);

    Ok(FundMigrationStatus {
        unassigned_bank_transactions: report.unassigned_bank_transactions,
        unassigned_payment_batches: report.unassigned_payment_batches,
        unassigned_voucher_lines: report.unassigned_voucher_lines,
        pending_count: report.pending_count,
        bank_months,
        pending_batches,
        unlinked_voucher_lines,
        completed_at: db::get_setting(conn, "stage7_migration_completed_at")?,
        last_applied_at: db::get_setting(conn, STAGE7_FUND_ASSIGNMENT_LAST_APPLIED_KEY)?,
    })
}

/// 待归集批次查询（batch_id 为 None 时返回全部；账户为空且非 void，复用批次行映射）
fn pending_payment_batches(
    conn: &Connection,
    batch_id: Option<i64>,
) -> AppResult<Vec<PaymentBatch>> {
    let mut sql = String::from(
        "SELECT b.id, b.batch_no, b.belong_month, b.batch_type, b.status, b.total_amount,
                b.item_count, b.payment_date, b.remark, b.fund_account_id, fa.name,
                b.created_at, b.updated_at
         FROM payment_batches b
         LEFT JOIN fund_accounts fa ON fa.id = b.fund_account_id
         WHERE b.fund_account_id IS NULL AND b.status != 'void'",
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(id) = batch_id {
        sql.push_str(" AND b.id = ?1");
        params_vec.push(Box::new(id));
    }
    sql.push_str(" ORDER BY b.belong_month, b.id");
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), db::row_to_payment_batch)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

/// 联动分录统计：来源对象集合（银行流水 / 付款批次）内 active 凭证的资金分录，
/// 按科目是否等于账户挂接科目分流。返回 (可联动补齐数, 科目不一致保持 NULL 数)。
fn count_linkable_lines(
    conn: &Connection,
    entity_type: &str,
    source_ids: &[i64],
    gl_account_code: &str,
) -> AppResult<(i64, i64)> {
    if source_ids.is_empty() {
        return Ok((0, 0));
    }
    let in_ids = source_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    // 批次付款凭证为 salary_payment / reimbursement_payment（source_id=批次 id），
    // 流水手工凭证为 bank_manual（source_id=流水 id），见 accounting.rs 生成逻辑
    let source_types = match entity_type {
        "bank_transaction" => "bank_manual",
        _ => "salary_payment','reimbursement_payment",
    };
    let codes = db::STAGE7_FUND_GL_CODES.join("','");
    let sql = format!(
        "SELECT
           COALESCE(SUM(CASE WHEN vl.account_code = ?1 THEN 1 ELSE 0 END), 0),
           COALESCE(SUM(CASE WHEN vl.account_code != ?1 THEN 1 ELSE 0 END), 0)
         FROM voucher_lines vl
         JOIN vouchers v ON v.id = vl.voucher_id
         WHERE v.source_type IN ('{source_types}')
           AND v.source_id IN ({in_ids})
           AND v.status = 'active'
           AND vl.fund_account_id IS NULL
           AND (vl.debit_amount > 0 OR vl.credit_amount > 0)
           AND vl.account_code IN ('{codes}')"
    );
    let mut stmt = conn.prepare(&sql)?;
    let (affected, skipped): (i64, i64) =
        stmt.query_row(params![gl_account_code], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok((affected, skipped))
}

/// 归集预览：按指定账户与范围核对将写入的对象数与联动分录数（只读，不写库）。
/// 单账户唯一映射时前端可预填目标账户，但写入必须经 apply 确认（spec 9：不静默写入）。
pub fn preview_fund_assignment(
    conn: &Connection,
    entity_type: &str,
    account_id: i64,
    belong_month: Option<&str>,
    batch_id: Option<i64>,
) -> AppResult<FundAssignmentPreview> {
    ensure_assignment_entity(entity_type)?;
    let account = migration_target_account(conn, account_id)?;

    let (item_count, affected, skipped) = match entity_type {
        "bank_transaction" => {
            let month = belong_month.map(str::trim).filter(|m| !m.is_empty());
            let (condition, params_vec): (String, Vec<Box<dyn rusqlite::ToSql>>) = match month {
                Some(m) => (
                    "fund_account_id IS NULL AND belong_month = ?1".to_string(),
                    vec![Box::new(m.to_string())],
                ),
                None => ("fund_account_id IS NULL".to_string(), vec![]),
            };
            let sql = format!("SELECT id FROM bank_transactions WHERE {condition} ORDER BY id");
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();
            let mut stmt = conn.prepare(&sql)?;
            let ids = stmt
                .query_map(params_refs.as_slice(), |r| r.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            let item_count = ids.len() as i64;
            let (affected, skipped) =
                count_linkable_lines(conn, entity_type, &ids, &account.gl_account_code)?;
            (item_count, affected, skipped)
        }
        _ => {
            let batches = pending_payment_batches(conn, batch_id)?;
            let ids: Vec<i64> = batches.iter().map(|b| b.id).collect();
            let (affected, skipped) =
                count_linkable_lines(conn, entity_type, &ids, &account.gl_account_code)?;
            (ids.len() as i64, affected, skipped)
        }
    };

    Ok(FundAssignmentPreview {
        entity_type: entity_type.to_string(),
        item_count,
        affected_voucher_lines: affected,
        skipped_voucher_lines: skipped,
    })
}

/// 执行历史归集（单事务）：写入对象账户 + 联动 active 凭证资金分录 + 刷新迁移计数。
/// - 月结保护：逐月 `ensure_month_open`，任一月份已正式月结则整体回滚不写入；
/// - 幂等：UPDATE 带 `fund_account_id IS NULL`，已归集对象自动跳过；
/// - void 凭证分录不联动；科目与账户挂接科目不一致的分录保持 NULL（spec 9.5）。
pub fn apply_fund_assignment(
    conn: &Connection,
    input: &FundAssignmentInput,
) -> AppResult<FundAssignmentResult> {
    ensure_assignment_entity(&input.entity_type)?;
    let account = migration_target_account(conn, input.account_id)?;
    let now = Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;

    let (updated_count, linked_updated, skipped_lines) = match input.entity_type.as_str() {
        "bank_transaction" => {
            let month = input
                .belong_month
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty());
            let (condition, params_vec): (String, Vec<Box<dyn rusqlite::ToSql>>) = match month {
                Some(m) => (
                    "fund_account_id IS NULL AND belong_month = ?1".to_string(),
                    vec![Box::new(m.to_string())],
                ),
                None => ("fund_account_id IS NULL".to_string(), vec![]),
            };
            let sql = format!(
                "SELECT id, belong_month FROM bank_transactions WHERE {condition} ORDER BY id"
            );
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();
            let mut stmt = tx.prepare(&sql)?;
            let targets = stmt
                .query_map(params_refs.as_slice(), |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let ids: Vec<i64> = targets.iter().map(|(id, _)| *id).collect();

            // 月结保护：先校验流水月份与联动凭证月份，再写入（任一月结则整体回滚）
            let mut months: Vec<String> = targets.into_iter().map(|(_, m)| m).collect();
            months.extend(linked_voucher_months(&tx, "bank_transaction", &ids)?);
            months.sort();
            months.dedup();
            for m in &months {
                db::ensure_month_open(&tx, m)?;
            }

            let updated = if ids.is_empty() {
                0
            } else {
                let in_ids = in_list(&ids);
                let mut all: Vec<Box<dyn rusqlite::ToSql>> = params_vec;
                all.push(Box::new(account.id));
                all.push(Box::new(now.clone()));
                let all_refs: Vec<&dyn rusqlite::ToSql> = all.iter().map(|p| p.as_ref()).collect();
                tx.execute(
                    &format!(
                        "UPDATE bank_transactions SET fund_account_id = ?{}, updated_at = ?{}
                         WHERE fund_account_id IS NULL AND id IN ({in_ids})",
                        all.len() - 1,
                        all.len()
                    ),
                    all_refs.as_slice(),
                )? as i64
            };
            let (_, skipped) =
                count_linkable_lines(&tx, "bank_transaction", &ids, &account.gl_account_code)?;
            let linked = link_voucher_lines(
                &tx,
                "bank_transaction",
                &ids,
                account.id,
                &account.gl_account_code,
            )?;
            (updated, linked, skipped)
        }
        _ => {
            let batches = pending_payment_batches(&tx, input.batch_id)?;
            // 指定批次归集时必须命中：重复归集明确拦截，优于静默空操作
            if let Some(id) = input.batch_id {
                if batches.is_empty() {
                    return Err(AppError::InvalidParam(format!(
                        "付款批次 id={id} 已归集或不存在，请刷新后重试"
                    )));
                }
            }
            let ids: Vec<i64> = batches.iter().map(|b| b.id).collect();

            let mut months: Vec<String> = batches.into_iter().map(|b| b.belong_month).collect();
            months.extend(linked_voucher_months(&tx, "payment_batch", &ids)?);
            months.sort();
            months.dedup();
            for m in &months {
                db::ensure_month_open(&tx, m)?;
            }

            let updated = if ids.is_empty() {
                0
            } else {
                let n = tx.execute(
                    &format!(
                        "UPDATE payment_batches SET fund_account_id = ?1, updated_at = ?2
                         WHERE fund_account_id IS NULL AND status != 'void' AND id IN ({})",
                        in_list(&ids)
                    ),
                    rusqlite::params![account.id, now],
                )?;
                n as i64
            };
            let (_, skipped) =
                count_linkable_lines(&tx, "payment_batch", &ids, &account.gl_account_code)?;
            let linked = link_voucher_lines(
                &tx,
                "payment_batch",
                &ids,
                account.id,
                &account.gl_account_code,
            )?;
            (updated, linked, skipped)
        }
    };

    // 归集后刷新迁移计数（保持 stage7_migration_* 键实时，首次完成时间戳不覆盖）
    let report = db::build_stage7_report(&tx)?;
    db::record_stage7_state(&tx, &report)?;
    db::set_setting(&tx, STAGE7_FUND_ASSIGNMENT_LAST_APPLIED_KEY, &now)?;
    tx.commit()?;

    Ok(FundAssignmentResult {
        updated_count,
        linked_voucher_lines_updated: linked_updated,
        skipped_voucher_lines: skipped_lines,
    })
}

/// 来源对象集合内 active 凭证的归属月集合（联动前月份校验用）
fn linked_voucher_months(
    conn: &Connection,
    entity_type: &str,
    source_ids: &[i64],
) -> AppResult<Vec<String>> {
    if source_ids.is_empty() {
        return Ok(vec![]);
    }
    let source_types = match entity_type {
        "bank_transaction" => "bank_manual",
        _ => "salary_payment','reimbursement_payment",
    };
    let sql = format!(
        "SELECT DISTINCT v.belong_month FROM vouchers v
         WHERE v.source_type IN ('{source_types}')
           AND v.source_id IN ({})
           AND v.status = 'active'
         ORDER BY v.belong_month",
        in_list(source_ids)
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

/// 联动补齐来源对象集合内 active 凭证的资金分录：
/// 仅补 `fund_account_id IS NULL` 且科目等于账户挂接科目的分录；void 凭证不动。
fn link_voucher_lines(
    conn: &Connection,
    entity_type: &str,
    source_ids: &[i64],
    account_id: i64,
    gl_account_code: &str,
) -> AppResult<i64> {
    if source_ids.is_empty() {
        return Ok(0);
    }
    let source_types = match entity_type {
        "bank_transaction" => "bank_manual",
        _ => "salary_payment','reimbursement_payment",
    };
    let sql = format!(
        "UPDATE voucher_lines SET fund_account_id = ?1
         WHERE fund_account_id IS NULL AND account_code = ?2
           AND voucher_id IN (
             SELECT id FROM vouchers
             WHERE source_type IN ('{source_types}')
               AND source_id IN ({})
               AND status = 'active'
           )",
        in_list(source_ids)
    );
    let n = conn.execute(sql.as_str(), rusqlite::params![account_id, gl_account_code])?;
    Ok(n as i64)
}

/// 整数 id 集合转 SQL IN 列表（id 均为数据库整型主键，无注入面）
fn in_list(ids: &[i64]) -> String {
    ids.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",")
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

/// 资金单据类型（spec 4.4）
const FUND_DOCUMENT_TYPES: &[&str] = &[
    "receipt",
    "payment",
    "transfer",
    "advance",
    "advance_settlement",
    "reversal",
];

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

/// 经办复核开关的 app_settings 键（spec 2：可选 maker_checker_enabled；
/// 启用时提交人与审批人不得相同。单机切换身份属流程约束，不宣称安全隔离）
const MAKER_CHECKER_SETTING: &str = "maker_checker_enabled";

/// 资金单附件可变更状态：仅未进入审批流的单据（与报销单口径一致，spec 4.6/第 8 节）
const FUND_DOCUMENT_ATTACHMENT_EDITABLE_STATUSES: &[&str] = &["draft", "rejected", "void"];

/// 可直接结算的单据类型（spec 5.1：receipt/transfer 直接收支结算；
/// advance_settlement 核销回流走结算；payment/advance 必须经付款批次标记付款）
const DIRECT_SETTLE_TYPES: &[&str] = &["receipt", "transfer", "advance_settlement"];

/// 可进入付款批次的单据类型（spec 5.1：payment/advance）
const BATCHABLE_TYPES: &[&str] = &["payment", "advance"];

const FUND_DOCUMENT_COLS: &str = "id, document_no, document_type, belong_month, document_date, amount, summary, department, expense_type, remark, partner_id, employee_id, source_account_id, target_account_id, counter_account_code, status, payment_batch_id, reversal_of_id, submitted_by, submitted_at, approved_by, approved_at, settled_by, settled_at, voided_by, voided_at, created_by, created_at, updated_at";

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

/// 状态中文名（错误信息与命令层日志描述共用）
pub(crate) fn fund_status_label(status: &str) -> &'static str {
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

/// 单据类型中文名（错误信息与命令层日志描述共用）
pub(crate) fn fund_document_type_label(doc_type: &str) -> &'static str {
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

/// 状态机动作中文名（错误信息与审批事件注释校验提示用）
fn fund_action_label(action: &str) -> &'static str {
    match action {
        "submit" => "提交",
        "approve" => "审批",
        "reject" => "驳回",
        "withdraw" => "撤回",
        "void" => "作废",
        "batch" => "进入付款批次",
        "unbatch" => "移出付款批次",
        "settle" => "结算",
        "reverse" => "冲正",
        _ => "状态变更",
    }
}

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

/// patch 语义可空字符串规范化：去空格，空串归一为 None
fn trimmed_optional(v: Option<&str>) -> Option<String> {
    v.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

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

/// 按条件查询资金单据（月份/类型/状态/往来对象/关键字），按 id 倒序（新单在前）
pub fn get_fund_documents(
    conn: &Connection,
    q: &FundDocumentQuery,
) -> AppResult<Vec<FundDocument>> {
    // 类型/状态入参校验：拼进 SQL 前先拦住非法枚举，避免拼错时静默返回空列表
    if let Some(t) = q
        .document_type
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        ensure_in_list(t, FUND_DOCUMENT_TYPES, "单据类型")?;
    }
    if let Some(s) = q.status.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        ensure_in_list(s, FUND_DOCUMENT_STATUSES, "单据状态")?;
    }
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
    if let Some(a) = q.account_id {
        where_clauses.push(format!(
            "(source_account_id = ?{idx} OR target_account_id = ?{idx})"
        ));
        params_vec.push(Box::new(a));
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

/// 读取经办复核开关（app_settings；缺省关闭）
fn maker_checker_enabled(conn: &Connection) -> AppResult<bool> {
    Ok(db::get_setting(conn, MAKER_CHECKER_SETTING)?
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false))
}

/// 读取经办复核开关（命令层暴露给设置界面；缺省关闭）
pub fn get_maker_checker_enabled(conn: &Connection) -> AppResult<bool> {
    maker_checker_enabled(conn)
}

/// 设置经办复核开关（命令层暴露给设置界面）
pub fn set_maker_checker_enabled(conn: &Connection, enabled: bool) -> AppResult<()> {
    db::set_setting(
        conn,
        MAKER_CHECKER_SETTING,
        if enabled { "true" } else { "false" },
    )
}

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

/// 状态机命令公共骨架：当前操作人署名 → 事务内取单据 → 校验来源状态 →
/// 月结保护 → 单据状态更新 + 追加审批事件（同事务，spec 5.1/4.5）→ 提交。
/// `extra` 在状态更新前执行（maker_checker 校验、署名字段/批次回写等），
/// 任一步失败整体回滚，不留半成品。
#[allow(clippy::too_many_arguments)]
/// 状态机事务内核：在调用方事务（`&Connection`）内完成来源状态校验、月结保护、
/// 附加动作与审批事件落库。供独立命令（自包事务）与批次事务（外部事务）共用，
/// 保证"入批次/释放/结算"与批次状态变更同事务提交（spec 5.3）。
fn transition_fund_document_in_tx<F>(
    tx: &Connection,
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
    let (operator_id, _) = require_current_operator(tx, current)?;
    let trimmed = comment.map(str::trim).unwrap_or("");
    if require_comment && trimmed.is_empty() {
        return Err(AppError::InvalidParam(format!(
            "{}必须填写意见或原因",
            fund_action_label(action)
        )));
    }
    let now = Utc::now().to_rfc3339();
    let doc = get_fund_document(tx, document_id)?;
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
    ensure_month_open(tx, &doc.belong_month)?;
    extra(tx, &doc, operator_id, &now)?;
    tx.execute(
        "UPDATE fund_documents SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![document_id, to_status, now],
    )?;
    insert_approval_event(
        tx,
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
    get_fund_document(tx, document_id)
}

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
    let tx = conn.unchecked_transaction()?;
    transition_fund_document_in_tx(
        &tx,
        current,
        document_id,
        action,
        from_statuses,
        to_status,
        comment,
        require_comment,
        extra,
    )?;
    tx.commit()?;
    get_fund_document(conn, document_id)
}

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

/// 标记进入付款批次（approved → batched；仅付款/借款单）。
/// 无独立 Tauri 命令：由通用付款批次创建事务内调用（spec 5.3），
/// 单据 batched 与批次落库同事务提交，禁止前端直调绕过批次。
pub(crate) fn mark_document_batched_in_tx(
    tx: &Connection,
    current: &CurrentOperatorState,
    document_id: i64,
    batch_id: i64,
) -> AppResult<FundDocument> {
    transition_fund_document_in_tx(
        tx,
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

/// 批次作废释放资金单（batched → approved，spec 5.3 反向）：清空 `payment_batch_id`
/// 并追加审批事件，单据可重新进入其他批次。由通用批次作废事务内调用，无独立命令。
pub(crate) fn release_document_batched_in_tx(
    tx: &Connection,
    current: &CurrentOperatorState,
    document_id: i64,
) -> AppResult<FundDocument> {
    transition_fund_document_in_tx(
        tx,
        current,
        document_id,
        "unbatch",
        &["batched"],
        "approved",
        None,
        false,
        |tx, doc, _operator_id, _now| {
            tx.execute(
                "UPDATE fund_documents SET payment_batch_id = NULL WHERE id = ?1",
                params![doc.id],
            )?;
            Ok(())
        },
    )
}

/// 批次标记付款时的单据结算内核（batched → settled，spec 5.3）：校验批次类型、
/// 落结算署名并同事务生成资金单凭证（spec 4.7）。供 settle 命令与批次 paid 事务共用。
fn settle_batched_document_content(
    tx: &Connection,
    doc: &FundDocument,
    operator_id: i64,
    now: &str,
) -> AppResult<()> {
    if !BATCHABLE_TYPES.contains(&doc.document_type.as_str()) {
        return Err(AppError::General(format!(
            "单据 {} 类型为「{}」，只有付款单/员工借款单可经付款批次结算",
            doc.document_no,
            fund_document_type_label(&doc.document_type)
        )));
    }
    tx.execute(
        "UPDATE fund_documents SET settled_by = ?2, settled_at = ?3 WHERE id = ?1",
        params![doc.id, operator_id, now],
    )?;
    // 结算凭证同事务生成（spec 4.7）：任一分录失败整体回滚，
    // 结算状态、凭证、审批事件保持原子
    generate_fund_document_voucher(tx, doc)?;
    Ok(())
}

/// 批次标记付款事务内的单据结算（batched → settled）：状态机校验 + 结算署名 +
/// 凭证生成 + 审批事件全部在批次事务内提交（spec 5.3 "付款后……将来源单据置 settled"）。
pub(crate) fn settle_batched_document_in_tx(
    tx: &Connection,
    current: &CurrentOperatorState,
    document_id: i64,
) -> AppResult<FundDocument> {
    transition_fund_document_in_tx(
        tx,
        current,
        document_id,
        "settle",
        &["batched"],
        "settled",
        None,
        false,
        settle_batched_document_content,
    )
}

/// 结算（spec 5.1）：收款/内部转账/借款核销单审批后直接结算；
/// 付款/借款单须经付款批次标记付款后从 batched 结算。
/// 结算同时生成资金辅助凭证（source_type='fund_document'，spec 4.7），
/// 状态、凭证与审批事件同事务提交，任何失败整体回滚。
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
                "batched" if BATCHABLE_TYPES.contains(&doc.document_type.as_str()) => {
                    return settle_batched_document_content(tx, doc, operator_id, now);
                }
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
            // 结算凭证同事务生成（spec 4.7）：任一分录失败整体回滚，
            // 结算状态、凭证、审批事件保持原子
            generate_fund_document_voucher(tx, doc)?;
            Ok(())
        },
    )
}

/// 冲正（settled → reversed，spec 5.1）：在开放月份创建相反方向冲正单（立即结算生效），
/// 原单置为已冲正；原单月份与冲正月份均须未月结。
/// 冲正凭证同事务生成：复制原单生效凭证并交换借贷方向（source_id 指向冲正单），
/// 原凭证保留 active，经冲正单 `reversal_of_id` 与凭证备注建立追溯（spec 4.7/5.1）。
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
    // 冲正凭证同事务生成（spec 4.7）：复制原单凭证交换借贷、原凭证保留 active；
    // 任何一步失败（含凭证生成）整体回滚，不留"已冲正但无凭证"的半成品
    let reversal_doc = get_fund_document(&tx, reversal_id)?;
    generate_reversal_document_voucher(&tx, &reversal_doc, &original)?;
    tx.commit()?;

    get_fund_document(conn, reversal_id)
}

// ==================== 资金单凭证联动（spec 4.7） ====================

/// 资金单凭证 source_type（vouchers 表 CHECK 白名单第七阶段新增）
const FUND_VOUCHER_SOURCE_TYPE: &str = "fund_document";

/// 借款/借款核销单未指定对方科目时的默认科目（1221 其他应收款，spec 4.7）
const ADVANCE_DEFAULT_GL: &str = "1221";

/// 读取资金账户挂接的总账科目（建账户时已限 1001/1002/1012，见 `STAGE7_FUND_GL_CODES`）
pub(crate) fn fund_account_gl_code(conn: &Connection, account_id: i64) -> AppResult<String> {
    conn.query_row(
        "SELECT gl_account_code FROM fund_accounts WHERE id = ?1",
        params![account_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("资金账户不存在：id={account_id}")))
}

/// 单据对方科目（去空格；空串视同未填）
fn doc_counter_account(doc: &FundDocument) -> Option<&str> {
    doc.counter_account_code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// 构造一条凭证分录（金额方向为正数，资金行带 fund_account_id、对方行必须为空）
fn fund_voucher_line(
    account_code: String,
    debit: f64,
    credit: f64,
    fund_account_id: Option<i64>,
    summary: &str,
) -> VoucherLineDraft {
    VoucherLineDraft {
        account_code,
        debit_amount: debit,
        credit_amount: credit,
        summary: Some(summary.to_string()),
        fund_account_id,
    }
}

/// 资金分录辅助核算校验（spec 4.7）：资金科目（1001/1002/1012）分录必须带 `fund_account_id`，
/// 对方科目分录必须为空——漏写辅助核算直接报错回滚，不允许生成"无主"资金分录。
/// 付款批次凭证与银行流水手工凭证（accounting.rs）同样复用本校验。
pub(crate) fn ensure_fund_voucher_lines(lines: &[VoucherLineDraft]) -> AppResult<()> {
    for line in lines {
        let is_fund = db::STAGE7_FUND_GL_CODES.contains(&line.account_code.as_str());
        if is_fund && line.fund_account_id.is_none() {
            return Err(AppError::General(format!(
                "科目 {} 为资金科目，分录必须携带资金账户",
                line.account_code
            )));
        }
        if !is_fund && line.fund_account_id.is_some() {
            return Err(AppError::General(format!(
                "科目 {} 非资金科目，分录不允许携带资金账户",
                line.account_code
            )));
        }
    }
    Ok(())
}

/// 生成资金单结算凭证（spec 4.7 分录方向规则；source_type='fund_document'，source_id=单据 id）：
/// - 收款：借目标资金账户 / 贷对方科目；
/// - 付款：借对方科目 / 贷来源资金账户；
/// - 内部转账：借目标账户 / 贷来源账户（两行各带对应 `fund_account_id`）；
/// - 员工借款：借对方科目（默认 1221 其他应收款）/ 贷来源资金账户；
/// - 借款核销：借目标资金账户 / 贷对方科目（默认 1221 其他应收款）。
/// 资金行 `fund_account_id` 必填、对方行必须为空；凭证归属月/日期取单据归属月/单据日期。
/// 必须在状态机事务内调用（settle），凭证与结算状态、审批事件同事务提交。
pub(crate) fn generate_fund_document_voucher(
    conn: &Connection,
    doc: &FundDocument,
) -> AppResult<Voucher> {
    let amount = doc.amount;
    let summary = doc.summary.as_str();
    let require_counter = |label: &str| -> AppResult<String> {
        doc_counter_account(doc)
            .map(str::to_string)
            .ok_or_else(|| AppError::General(format!("单据 {} 缺少对方科目，无法生成凭证", label)))
    };
    let require_account = |id: Option<i64>, label: &str| -> AppResult<i64> {
        id.ok_or_else(|| {
            AppError::General(format!(
                "单据 {} 缺少{}资金账户，无法生成凭证",
                doc.document_no, label
            ))
        })
    };
    let lines: Vec<VoucherLineDraft> = match doc.document_type.as_str() {
        "receipt" => {
            let target = require_account(doc.target_account_id, "目标")?;
            let counter = require_counter("收款单")?;
            vec![
                fund_voucher_line(
                    fund_account_gl_code(conn, target)?,
                    amount,
                    0.0,
                    Some(target),
                    summary,
                ),
                fund_voucher_line(counter, 0.0, amount, None, summary),
            ]
        }
        "payment" => {
            let source = require_account(doc.source_account_id, "来源")?;
            let counter = require_counter("付款单")?;
            vec![
                fund_voucher_line(counter, amount, 0.0, None, summary),
                fund_voucher_line(
                    fund_account_gl_code(conn, source)?,
                    0.0,
                    amount,
                    Some(source),
                    summary,
                ),
            ]
        }
        "transfer" => {
            let source = require_account(doc.source_account_id, "来源")?;
            let target = require_account(doc.target_account_id, "目标")?;
            vec![
                fund_voucher_line(
                    fund_account_gl_code(conn, target)?,
                    amount,
                    0.0,
                    Some(target),
                    summary,
                ),
                fund_voucher_line(
                    fund_account_gl_code(conn, source)?,
                    0.0,
                    amount,
                    Some(source),
                    summary,
                ),
            ]
        }
        "advance" => {
            let source = require_account(doc.source_account_id, "来源")?;
            let counter = doc_counter_account(doc)
                .map(str::to_string)
                .unwrap_or_else(|| ADVANCE_DEFAULT_GL.to_string());
            vec![
                fund_voucher_line(counter, amount, 0.0, None, summary),
                fund_voucher_line(
                    fund_account_gl_code(conn, source)?,
                    0.0,
                    amount,
                    Some(source),
                    summary,
                ),
            ]
        }
        "advance_settlement" => {
            let target = require_account(doc.target_account_id, "目标")?;
            let counter = doc_counter_account(doc)
                .map(str::to_string)
                .unwrap_or_else(|| ADVANCE_DEFAULT_GL.to_string());
            vec![
                fund_voucher_line(
                    fund_account_gl_code(conn, target)?,
                    amount,
                    0.0,
                    Some(target),
                    summary,
                ),
                fund_voucher_line(counter, 0.0, amount, None, summary),
            ]
        }
        other => {
            return Err(AppError::General(format!(
                "单据类型「{other}」不支持生成结算凭证"
            )))
        }
    };
    ensure_fund_voucher_lines(&lines)?;
    accounting::insert_voucher(
        conn,
        &VoucherDraft {
            belong_month: doc.belong_month.clone(),
            voucher_date: doc.document_date.clone(),
            source_type: FUND_VOUCHER_SOURCE_TYPE.into(),
            source_id: doc.id,
            remark: Some(format!("资金单 {}", doc.document_no)),
            lines,
        },
    )
}

/// 生成冲正凭证（spec 4.7）：复制原单生效凭证并交换借贷方向
/// （资金行的 `fund_account_id` 随科目保留），source_id 指向冲正单；
/// 原凭证保留 active，经冲正单 `reversal_of_id` 与凭证备注建立追溯。
/// 必须在冲正事务内调用。
pub(crate) fn generate_reversal_document_voucher(
    conn: &Connection,
    reversal: &FundDocument,
    original: &FundDocument,
) -> AppResult<Voucher> {
    let source_voucher =
        accounting::get_active_voucher_for_source(conn, FUND_VOUCHER_SOURCE_TYPE, original.id)?
            .ok_or_else(|| {
                AppError::General(format!(
                    "原单 {} 没有生效凭证，无法生成冲正凭证",
                    original.document_no
                ))
            })?;
    let lines: Vec<VoucherLineDraft> = source_voucher
        .lines
        .iter()
        .map(|l| VoucherLineDraft {
            account_code: l.account_code.clone(),
            debit_amount: l.credit_amount,
            credit_amount: l.debit_amount,
            summary: l.summary.clone(),
            fund_account_id: l.fund_account_id,
        })
        .collect();
    ensure_fund_voucher_lines(&lines)?;
    accounting::insert_voucher(
        conn,
        &VoucherDraft {
            belong_month: reversal.belong_month.clone(),
            voucher_date: reversal.document_date.clone(),
            source_type: FUND_VOUCHER_SOURCE_TYPE.into(),
            source_id: reversal.id,
            remark: Some(format!("冲正原单 {}", original.document_no)),
            lines,
        },
    )
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

// ==================== 银行流水多对多核销引擎（Task 12，spec 4.9/6.2/6.3） ====================

/// 自动匹配默认日期窗口（spec 6.3「日期在配置窗口内」；可用 app_settings 键
/// `bank_match_date_window_days` 覆盖，缺省 90 天。人工候选预览不设硬窗口，仅作评分因子）
const BANK_MATCH_DATE_WINDOW_DAYS_DEFAULT: i64 = 90;

/// 自动匹配默认置信线：批量确认只写入达到该分数的候选（spec 6.2 不自动写低置信匹配）
const BANK_MATCH_MIN_SCORE_DEFAULT: i32 = 60;

/// 流水收支方向（spec 6.2 硬条件：收流水核借方分录、付流水核贷方分录）。
/// 零金额或收支并存的流水方向不明确，不能核销。
fn bank_tx_direction(income: f64, expense: f64) -> AppResult<(&'static str, f64)> {
    let income_on = income > AMOUNT_TOLERANCE;
    let expense_on = expense > AMOUNT_TOLERANCE;
    match (income_on, expense_on) {
        (true, false) => Ok(("income", income)),
        (false, true) => Ok(("expense", expense)),
        _ => Err(AppError::InvalidParam(
            "流水收支方向不明确（零金额或收支并存），不能核销".into(),
        )),
    }
}

/// 流水核心字段（引擎内部轻量读取，避免全表联表扫描）
struct BankTxCore {
    id: i64,
    belong_month: String,
    transaction_date: String,
    summary: Option<String>,
    counterparty_name: Option<String>,
    counterparty_account: Option<String>,
    income_amount: f64,
    expense_amount: f64,
    status: String,
    fund_account_id: Option<i64>,
    fund_account_name: Option<String>,
}

fn get_bank_tx_core(conn: &Connection, id: i64) -> AppResult<BankTxCore> {
    conn.query_row(
        "SELECT t.id, t.belong_month, t.transaction_date, t.summary, t.counterparty_name,
                t.counterparty_account, t.income_amount, t.expense_amount, t.status,
                t.fund_account_id, fa.name
         FROM bank_transactions t
         LEFT JOIN fund_accounts fa ON fa.id = t.fund_account_id
         WHERE t.id = ?1",
        params![id],
        |r| {
            Ok(BankTxCore {
                id: r.get(0)?,
                belong_month: r.get(1)?,
                transaction_date: r.get(2)?,
                summary: r.get(3)?,
                counterparty_name: r.get(4)?,
                counterparty_account: r.get(5)?,
                income_amount: r.get(6)?,
                expense_amount: r.get(7)?,
                status: r.get(8)?,
                fund_account_id: r.get(9)?,
                fund_account_name: r.get(10)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("银行流水 ID={id} 不存在")))
}

/// 流水侧已核销额 = active allocation 合计 + 未迁移的 active 旧式批次匹配合计
/// （旧匹配金额=批次额，confirm_bank_transaction_match 校验与流水支出相等）。
/// 已迁移的旧匹配由其 allocation 接管计量，不再重复计入，保证迁移过渡期金额守恒。
/// `pub(crate)` 供 db.rs 月结部分核销检查与旧 confirm 退役拦截复用（Task 13）。
pub(crate) fn bank_tx_allocated(conn: &Connection, transaction_id: i64) -> AppResult<f64> {
    let allocations: f64 = conn.query_row(
        "SELECT COALESCE(SUM(allocated_amount),0) FROM bank_reconciliation_allocations
         WHERE transaction_id = ?1 AND status = 'active'",
        params![transaction_id],
        |r| r.get(0),
    )?;
    let legacy: f64 = conn.query_row(
        "SELECT COALESCE(SUM(b.total_amount),0) FROM bank_transaction_matches m
         JOIN payment_batches b ON b.id = m.payment_batch_id
         WHERE m.transaction_id = ?1 AND m.status = 'active'
           AND NOT EXISTS (SELECT 1 FROM bank_reconciliation_allocations a
                           WHERE a.legacy_match_id = m.id)",
        params![transaction_id],
        |r| r.get(0),
    )?;
    Ok(allocations + legacy)
}

/// 分录侧已核销额（仅 active allocation；旧式匹配不指向具体分录，不参与分录侧计量）
fn bank_line_allocated(conn: &Connection, voucher_line_id: i64) -> AppResult<f64> {
    let sum: f64 = conn.query_row(
        "SELECT COALESCE(SUM(allocated_amount),0) FROM bank_reconciliation_allocations
         WHERE voucher_line_id = ?1 AND status = 'active'",
        params![voucher_line_id],
        |r| r.get(0),
    )?;
    Ok(sum)
}

/// 候选资金分录核心字段
struct BankLineCore {
    voucher_line_id: i64,
    voucher_id: i64,
    voucher_no: String,
    voucher_date: String,
    belong_month: String,
    source_type: String,
    source_id: i64,
    account_code: String,
    line_summary: Option<String>,
    debit_amount: f64,
    credit_amount: f64,
}

/// 查询某账户某方向的 active 资金分录（借方=钱进来，贷方=钱出去）
fn query_bank_fund_lines(
    conn: &Connection,
    account_id: i64,
    direction: &str,
) -> AppResult<Vec<BankLineCore>> {
    let side_cond = if direction == "income" {
        "vl.debit_amount > 0"
    } else {
        "vl.credit_amount > 0"
    };
    let sql = format!(
        "SELECT vl.id, v.id, v.voucher_no, v.voucher_date, v.belong_month, v.source_type,
                v.source_id, vl.account_code, vl.summary, vl.debit_amount, vl.credit_amount
         FROM voucher_lines vl
         JOIN vouchers v ON v.id = vl.voucher_id
         WHERE v.status = 'active' AND vl.fund_account_id = ?1 AND {side_cond}
         ORDER BY v.voucher_date, v.id, vl.line_order"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![account_id], |r| {
            Ok(BankLineCore {
                voucher_line_id: r.get(0)?,
                voucher_id: r.get(1)?,
                voucher_no: r.get(2)?,
                voucher_date: r.get(3)?,
                belong_month: r.get(4)?,
                source_type: r.get(5)?,
                source_id: r.get(6)?,
                account_code: r.get(7)?,
                line_summary: r.get(8)?,
                debit_amount: r.get(9)?,
                credit_amount: r.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 文本 2 字滑窗交集（中英混排的宽松摘要关键词命中）
fn bigram_overlap(a: &str, b: &str) -> bool {
    let a_chars: Vec<char> = a.chars().collect();
    let grams: std::collections::HashSet<Vec<char>> =
        a_chars.windows(2).map(|w| w.to_vec()).collect();
    if grams.is_empty() {
        return false;
    }
    let b_chars: Vec<char> = b.chars().collect();
    b_chars.windows(2).any(|w| grams.contains(&w.to_vec()))
}

/// 日期距离（自然日；解析失败返回 None，不参与日期评分）
fn date_distance(a: &str, b: &str) -> Option<i64> {
    let pa = NaiveDate::parse_from_str(a, "%Y-%m-%d").ok()?;
    let pb = NaiveDate::parse_from_str(b, "%Y-%m-%d").ok()?;
    Some((pa - pb).num_days().abs())
}

/// 自动匹配日期窗口（spec 6.3「配置窗口」，app_settings 可覆盖）
fn bank_match_date_window_days(conn: &Connection) -> i64 {
    db::get_setting(conn, "bank_match_date_window_days")
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|d| *d > 0)
        .unwrap_or(BANK_MATCH_DATE_WINDOW_DAYS_DEFAULT)
}

/// spec 6.3 评分（0-100）：金额一致 / 凭证号·单号 / 对方户名 / 账号尾号 / 摘要关键词 / 日期距离。
/// 返回 (分数, 因子说明)，说明供前端解释展示。
fn score_bank_candidate(
    tx: &BankTxCore,
    tx_remaining: f64,
    line: &BankLineCore,
    line_remaining: f64,
) -> (i32, Vec<String>) {
    let mut score = 0i32;
    let mut reasons: Vec<String> = Vec::new();

    if (tx_remaining - line_remaining).abs() <= AMOUNT_TOLERANCE {
        score += 40;
        reasons.push("金额完全一致 +40".into());
    } else {
        score += 10;
        reasons.push("金额部分吻合 +10".into());
    }

    let tx_text = format!(
        "{} {}",
        tx.summary.as_deref().unwrap_or(""),
        tx.counterparty_name.as_deref().unwrap_or("")
    );
    if tx_text.trim().chars().count() >= 4 && tx_text.contains(&line.voucher_no) {
        score += 20;
        reasons.push("凭证号出现在流水摘要 +20".into());
    }

    if let Some(cp) = tx
        .counterparty_name
        .as_deref()
        .map(str::trim)
        .filter(|s| s.chars().count() >= 2)
    {
        if line
            .line_summary
            .as_deref()
            .map(|s| s.contains(cp))
            .unwrap_or(false)
        {
            score += 15;
            reasons.push("对方户名命中分录摘要 +15".into());
        }
    }

    if let Some(acc) = tx
        .counterparty_account
        .as_deref()
        .map(str::trim)
        .filter(|s| s.chars().count() >= 4)
    {
        let tail: String = acc
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if line
            .line_summary
            .as_deref()
            .map(|s| s.contains(&tail))
            .unwrap_or(false)
        {
            score += 10;
            reasons.push("账号尾号命中分录摘要 +10".into());
        }
    }

    if let (Some(a), Some(b)) = (tx.summary.as_deref(), line.line_summary.as_deref()) {
        if bigram_overlap(a, b) {
            score += 10;
            reasons.push("摘要关键词命中 +10".into());
        }
    }

    match date_distance(&tx.transaction_date, &line.voucher_date) {
        Some(0) => {
            score += 5;
            reasons.push("同日 +5".into());
        }
        Some(d) if d <= 7 => {
            score += 3;
            reasons.push("日期相差 7 日内 +3".into());
        }
        Some(d) => reasons.push(format!("日期相差 {d} 日 +0")),
        None => {}
    }

    (score.min(100), reasons)
}

/// 构建单条流水的核销预览（候选 = active 凭证、同账户、方向相符且有未核销余额的分录）
fn build_bank_preview_item(
    conn: &Connection,
    tx: BankTxCore,
) -> AppResult<BankAutoMatchPreviewItem> {
    let account_id = tx.fund_account_id.ok_or_else(|| {
        AppError::InvalidParam(format!(
            "银行流水 ID={} 未归集资金账户，请先完成历史归集",
            tx.id
        ))
    })?;
    let (direction, side_amount) = bank_tx_direction(tx.income_amount, tx.expense_amount)?;
    let tx_remaining = (side_amount - bank_tx_allocated(conn, tx.id)?).max(0.0);

    let mut candidates: Vec<BankAllocationCandidate> = Vec::new();
    if tx_remaining > AMOUNT_TOLERANCE && tx.status != "ignored" {
        for line in query_bank_fund_lines(conn, account_id, direction)? {
            let side = if direction == "income" {
                line.debit_amount
            } else {
                line.credit_amount
            };
            let remaining = (side - bank_line_allocated(conn, line.voucher_line_id)?).max(0.0);
            if remaining <= AMOUNT_TOLERANCE {
                continue;
            }
            let (score, score_reasons) = score_bank_candidate(&tx, tx_remaining, &line, remaining);
            candidates.push(BankAllocationCandidate {
                voucher_line_id: line.voucher_line_id,
                voucher_id: line.voucher_id,
                voucher_no: line.voucher_no,
                voucher_date: line.voucher_date,
                belong_month: line.belong_month,
                source_type: line.source_type,
                source_id: line.source_id,
                account_code: line.account_code,
                line_summary: line.line_summary,
                debit_amount: line.debit_amount,
                credit_amount: line.credit_amount,
                remaining_amount: remaining,
                score,
                score_reasons,
            });
        }
        candidates.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.voucher_line_id.cmp(&b.voucher_line_id))
        });
    }

    Ok(BankAutoMatchPreviewItem {
        transaction_id: tx.id,
        transaction_date: tx.transaction_date,
        belong_month: tx.belong_month,
        summary: tx.summary,
        counterparty_name: tx.counterparty_name,
        counterparty_account: tx.counterparty_account,
        income_amount: tx.income_amount,
        expense_amount: tx.expense_amount,
        remaining_amount: tx_remaining,
        fund_account_id: account_id,
        fund_account_name: tx.fund_account_name,
        candidates,
    })
}

/// 单条流水的候选预览（人工核销用，不设日期硬窗口，窗口仅作评分因子）
pub fn preview_bank_allocation_candidates(
    conn: &Connection,
    transaction_id: i64,
) -> AppResult<BankAutoMatchPreviewItem> {
    let tx = get_bank_tx_core(conn, transaction_id)?;
    build_bank_preview_item(conn, tx)
}

/// 自动匹配预览（spec 6.2/6.3）：只返回候选与 score，绝不写库。
/// 在人工候选硬条件之上追加自动窗口：日期距离 ≤ 配置窗口（spec 6.3 硬条件）。
pub fn preview_bank_auto_matches(
    conn: &Connection,
    month: &str,
) -> AppResult<Vec<BankAutoMatchPreviewItem>> {
    let window = bank_match_date_window_days(conn);
    let tx_ids: Vec<i64> = conn
        .prepare(
            "SELECT id FROM bank_transactions
             WHERE belong_month = ?1 AND status != 'ignored' AND fund_account_id IS NOT NULL
             ORDER BY transaction_date, id",
        )?
        .query_map(params![month], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut items: Vec<BankAutoMatchPreviewItem> = Vec::new();
    for id in tx_ids {
        let tx = get_bank_tx_core(conn, id)?;
        let mut item = build_bank_preview_item(conn, tx)?;
        item.candidates.retain(|c| {
            date_distance(&item.transaction_date, &c.voucher_date)
                .map(|d| d <= window)
                .unwrap_or(false)
        });
        if !item.candidates.is_empty() {
            items.push(item);
        }
    }
    items.sort_by(|a, b| {
        let best_a = a.candidates.first().map(|c| c.score).unwrap_or(0);
        let best_b = b.candidates.first().map(|c| c.score).unwrap_or(0);
        best_b
            .cmp(&best_a)
            .then_with(|| a.transaction_id.cmp(&b.transaction_id))
    });
    Ok(items)
}

/// 单条核销校验与写入（调用方保证在事务内）。
/// 硬条件（spec 4.9/6.2）：流水未忽略且已归集；分录属 active 凭证且挂资金账户；
/// 同账户；方向相符；双方累计分配不超额。余额在事务内实时计算。
fn confirm_one_allocation_in_tx(
    conn: &Connection,
    item: &BankAllocationInput,
    match_method: &str,
    operator: &str,
) -> AppResult<i64> {
    if item.allocated_amount <= 0.0 {
        return Err(AppError::InvalidParam("核销金额必须为正数".into()));
    }
    let t = get_bank_tx_core(conn, item.transaction_id)?;
    if t.status == "ignored" {
        return Err(AppError::InvalidParam("已忽略流水不能核销".into()));
    }
    let account_id = t
        .fund_account_id
        .ok_or_else(|| AppError::InvalidParam("流水未归集资金账户，请先完成历史归集".into()))?;
    // 月结保护按银行流水月份控制（跨月差异规则，plan Task 12）
    ensure_month_open(conn, &t.belong_month)?;
    let (direction, side_amount) = bank_tx_direction(t.income_amount, t.expense_amount)?;

    let line = conn
        .query_row(
            "SELECT vl.id, vl.debit_amount, vl.credit_amount, vl.fund_account_id,
                    v.status, v.voucher_no
             FROM voucher_lines vl JOIN vouchers v ON v.id = vl.voucher_id
             WHERE vl.id = ?1",
            params![item.voucher_line_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            AppError::NotFound(format!("凭证分录 ID={} 不存在", item.voucher_line_id))
        })?;
    let (_line_id, line_debit, line_credit, line_account, voucher_status, voucher_no) = line;
    if voucher_status != "active" {
        return Err(AppError::InvalidParam(format!(
            "分录所在凭证 {voucher_no} 已作废，不能核销"
        )));
    }
    let Some(line_account) = line_account else {
        return Err(AppError::InvalidParam(
            "分录未挂资金账户辅助核算，不能核销".into(),
        ));
    };
    if line_account != account_id {
        return Err(AppError::InvalidParam(format!(
            "跨账户核销拒绝：流水账户({account_id})与分录账户({line_account})不一致"
        )));
    }
    let (line_side_amount, direction_ok) = if direction == "income" {
        (line_debit, line_debit > 0.0)
    } else {
        (line_credit, line_credit > 0.0)
    };
    if !direction_ok {
        return Err(AppError::InvalidParam(format!(
            "方向不符：{}流水只能核销{}资金分录（凭证 {voucher_no}）",
            if direction == "income" {
                "收入"
            } else {
                "支出"
            },
            if direction == "income" {
                "借方"
            } else {
                "贷方"
            }
        )));
    }

    // 两侧余额守恒校验：累计分配不得超过任一侧可核销余额（spec 4.9）。
    // 连接由 Mutex 串行化 + 单事务内计算写入，消除并发读写下余额漂移。
    let tx_remaining = side_amount - bank_tx_allocated(conn, item.transaction_id)?;
    if tx_remaining + AMOUNT_TOLERANCE < item.allocated_amount {
        return Err(AppError::InvalidParam(format!(
            "超出流水可核销余额：剩余 {tx_remaining:.2}，本次 {:.2}",
            item.allocated_amount
        )));
    }
    let line_remaining = line_side_amount - bank_line_allocated(conn, item.voucher_line_id)?;
    if line_remaining + AMOUNT_TOLERANCE < item.allocated_amount {
        return Err(AppError::InvalidParam(format!(
            "超出分录可核销余额：剩余 {line_remaining:.2}，本次 {:.2}",
            item.allocated_amount
        )));
    }

    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO bank_reconciliation_allocations
            (transaction_id, voucher_line_id, allocated_amount, status, match_method,
             score, remark, operator_name, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            item.transaction_id,
            item.voucher_line_id,
            item.allocated_amount,
            match_method,
            item.score,
            item.remark,
            operator,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 批量确认核销（manual/auto）：整体单事务提交，逐项校验互不阻塞，
/// 失败项跳过并记入 errors（spec 6.2 批量确认语义：只写入能通过全部硬条件的项）。
pub fn confirm_bank_allocations(
    conn: &Connection,
    items: &[BankAllocationInput],
    match_method: &str,
    operator: &str,
) -> AppResult<BankAllocationBatchResult> {
    if !matches!(match_method, "manual" | "auto") {
        return Err(AppError::InvalidParam(
            "核销方式只允许 manual 或 auto".into(),
        ));
    }
    let tx = conn.unchecked_transaction()?;
    let mut result = BankAllocationBatchResult {
        confirmed: 0,
        skipped: 0,
        errors: Vec::new(),
        allocation_ids: Vec::new(),
    };
    for item in items {
        match confirm_one_allocation_in_tx(&tx, item, match_method, operator) {
            Ok(id) => {
                result.confirmed += 1;
                result.allocation_ids.push(id);
            }
            Err(e) => {
                result.skipped += 1;
                result.errors.push(format!(
                    "流水{}→分录{}：{e}",
                    item.transaction_id, item.voucher_line_id
                ));
            }
        }
    }
    tx.commit()?;
    Ok(result)
}

/// 取消核销（spec 4.9）：状态标记 cancelled（原记录与金额保留可追溯，不物理删除），
/// 两侧余额随之释放可再核销；月结保护按银行流水月份。
pub fn cancel_bank_allocation(
    conn: &Connection,
    allocation_id: i64,
    operator: &str,
) -> AppResult<bool> {
    let (status, tx_month): (String, String) = conn
        .query_row(
            "SELECT a.status, t.belong_month FROM bank_reconciliation_allocations a
             JOIN bank_transactions t ON t.id = a.transaction_id
             WHERE a.id = ?1",
            params![allocation_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("核销记录 ID={allocation_id} 不存在")))?;
    if status != "active" {
        return Ok(false);
    }
    ensure_month_open(conn, &tx_month)?;
    let now = Utc::now().to_rfc3339();
    let updated = conn.execute(
        "UPDATE bank_reconciliation_allocations
         SET status='cancelled', operator_name=?1, updated_at=?2
         WHERE id=?3 AND status='active'",
        params![operator, now, allocation_id],
    )?;
    Ok(updated > 0)
}

/// 核销明细查询（对账页展示与追溯；belong_month 按流水月份过滤）
pub fn list_bank_allocations(
    conn: &Connection,
    query: &BankAllocationQuery,
) -> AppResult<Vec<BankReconciliationAllocation>> {
    let mut where_clauses = vec!["1=1".to_string()];
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let mut idx = 1;
    if let Some(v) = query.transaction_id {
        where_clauses.push(format!("a.transaction_id = ?{idx}"));
        params_vec.push(Box::new(v));
        idx += 1;
    }
    if let Some(v) = query.voucher_line_id {
        where_clauses.push(format!("a.voucher_line_id = ?{idx}"));
        params_vec.push(Box::new(v));
        idx += 1;
    }
    if let Some(m) = query
        .belong_month
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        where_clauses.push(format!("t.belong_month = ?{idx}"));
        params_vec.push(Box::new(m.to_string()));
        idx += 1;
    }
    if let Some(s) = query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        where_clauses.push(format!("a.status = ?{idx}"));
        params_vec.push(Box::new(s.to_string()));
    }

    let sql = format!(
        "SELECT a.id, a.transaction_id, a.voucher_line_id, a.allocated_amount, a.status,
                a.match_method, a.score, a.remark, a.operator_name, a.legacy_match_id,
                a.created_at, a.updated_at,
                v.id, v.voucher_no, v.voucher_date, v.belong_month, v.status,
                vl.account_code, vl.debit_amount, vl.credit_amount, vl.summary,
                vl.fund_account_id
         FROM bank_reconciliation_allocations a
         JOIN voucher_lines vl ON vl.id = a.voucher_line_id
         JOIN vouchers v ON v.id = vl.voucher_id
         JOIN bank_transactions t ON t.id = a.transaction_id
         WHERE {}
         ORDER BY a.id DESC",
        where_clauses.join(" AND ")
    );
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_refs.as_slice(), |r| {
            Ok(BankReconciliationAllocation {
                id: r.get(0)?,
                transaction_id: r.get(1)?,
                voucher_line_id: r.get(2)?,
                allocated_amount: r.get(3)?,
                status: r.get(4)?,
                match_method: r.get(5)?,
                score: r.get(6)?,
                remark: r.get(7)?,
                operator_name: r.get(8)?,
                legacy_match_id: r.get(9)?,
                created_at: r.get(10)?,
                updated_at: r.get(11)?,
                voucher_id: r.get(12)?,
                voucher_no: r.get(13)?,
                voucher_date: r.get(14)?,
                voucher_belong_month: r.get(15)?,
                voucher_status: r.get(16)?,
                account_code: r.get(17)?,
                line_debit_amount: r.get(18)?,
                line_credit_amount: r.get(19)?,
                line_summary: r.get(20)?,
                fund_account_id: r.get(21)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 批量确认自动匹配（spec 6.2/6.3）：只处理「高置信（score ≥ 阈值）且金额两侧相等」
/// 的最佳候选；候选按分数降序逐条走完整校验，余额被抢先消费的（冲突）自动跳过，
/// 金额守恒由 confirm_bank_allocations 的余额校验兜底。min_score <= 0 时用默认置信线。
pub fn batch_confirm_bank_auto_matches(
    conn: &Connection,
    month: &str,
    min_score: i32,
    operator: &str,
) -> AppResult<BankAllocationBatchResult> {
    ensure_month_open(conn, month)?;
    let threshold = if min_score > 0 {
        min_score
    } else {
        BANK_MATCH_MIN_SCORE_DEFAULT
    };
    let items = preview_bank_auto_matches(conn, month)?;
    let candidate_count = items.len();
    let mut inputs: Vec<BankAllocationInput> = Vec::new();
    for item in items {
        let Some(best) = item.candidates.first() else {
            continue;
        };
        if best.score < threshold {
            continue;
        }
        // 保守策略：只自动确认「流水剩余 = 分录剩余」的全额等量核销，
        // 部分核销留给人工判断，避免自动写入拆分口径
        if (item.remaining_amount - best.remaining_amount).abs() > AMOUNT_TOLERANCE {
            continue;
        }
        inputs.push(BankAllocationInput {
            transaction_id: item.transaction_id,
            voucher_line_id: best.voucher_line_id,
            allocated_amount: item.remaining_amount,
            remark: Some(format!("自动匹配 score={}", best.score)),
            score: Some(best.score),
        });
    }
    // 预筛阶段跳过（低置信/金额不等）与写入阶段跳过（余额冲突）合并报告
    let preview_skipped = candidate_count - inputs.len();
    let mut result = confirm_bank_allocations(conn, &inputs, "auto", operator)?;
    result.skipped += preview_skipped as i32;
    Ok(result)
}

// ==================== 资金日记账（Task 13，spec 6.1） ====================

/// 月份入参规整：去空白，空串视为 None
fn clean_month_param(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
}

/// 资金日记账（spec 6.1）：按账户查询带资金辅助核算的 active 凭证分录，
/// 输出日期、凭证号、来源单号、摘要、对方单位、收入、支出、滚动余额、对账状态。
/// 余额从账户期初开始（from_month 之前月份净发生滚入期初），按 日期+凭证号+凭证 id+分录顺序
/// 稳定排序累计 借−贷（收入−支出）。
pub fn get_fund_journal(conn: &Connection, query: &FundJournalQuery) -> AppResult<FundJournal> {
    let account = get_fund_account(conn, query.fund_account_id)?;
    let from = clean_month_param(query.from_month.as_deref());
    let to = clean_month_param(query.to_month.as_deref());

    // 期初滚入：账户期初 + from 之前月份净发生（跨月衔接，spec 6.1「从账户期初开始」）
    let mut opening_balance = account.opening_balance;
    if let Some(fm) = &from {
        let carry: f64 = conn.query_row(
            "SELECT COALESCE(SUM(vl.debit_amount - vl.credit_amount),0)
             FROM voucher_lines vl
             JOIN vouchers v ON v.id = vl.voucher_id
             WHERE v.status = 'active' AND vl.fund_account_id = ?1 AND v.belong_month < ?2",
            params![account.id, fm],
            |r| r.get(0),
        )?;
        opening_balance += carry;
    }

    let mut where_clauses = vec![
        "v.status = 'active'".to_string(),
        "vl.fund_account_id = ?1".to_string(),
    ];
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(account.id)];
    let mut idx = 2usize;
    if let Some(fm) = &from {
        where_clauses.push(format!("v.belong_month >= ?{idx}"));
        params_vec.push(Box::new(fm.clone()));
        idx += 1;
    }
    if let Some(tm) = &to {
        where_clauses.push(format!("v.belong_month <= ?{idx}"));
        params_vec.push(Box::new(tm.clone()));
    }

    // 分录侧已核销额仅统计 active allocation（旧式匹配不指向分录，不参与分录侧计量）；
    // 对方单位在来源单据为资金单且挂往来单位时回显（spec 6.1「对方单位」）
    let sql = format!(
        "SELECT vl.id, v.id, v.voucher_date, v.belong_month, v.voucher_no, v.source_type,
                v.source_id, vl.account_code, vl.summary, vl.debit_amount, vl.credit_amount,
                COALESCE(al.s, 0), bp.name
         FROM voucher_lines vl
         JOIN vouchers v ON v.id = vl.voucher_id
         LEFT JOIN (SELECT voucher_line_id, SUM(allocated_amount) AS s
                    FROM bank_reconciliation_allocations WHERE status = 'active'
                    GROUP BY voucher_line_id) al ON al.voucher_line_id = vl.id
         LEFT JOIN fund_documents fd ON v.source_type = 'fund_document' AND fd.id = v.source_id
         LEFT JOIN business_partners bp ON bp.id = fd.partner_id
         WHERE {}
         ORDER BY v.voucher_date, v.voucher_no, v.id, vl.line_order",
        where_clauses.join(" AND ")
    );
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let mapped = stmt.query_map(params_refs.as_slice(), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, Option<String>>(8)?,
            r.get::<_, f64>(9)?,
            r.get::<_, f64>(10)?,
            r.get::<_, f64>(11)?,
            r.get::<_, Option<String>>(12)?,
        ))
    })?;

    let mut balance = opening_balance;
    let mut rows = Vec::new();
    let mut total_income = 0.0;
    let mut total_expense = 0.0;
    for item in mapped {
        let (
            line_id,
            voucher_id,
            voucher_date,
            belong_month,
            voucher_no,
            source_type,
            source_id,
            account_code,
            summary,
            debit,
            credit,
            allocated,
            partner_name,
        ) = item?;
        balance += debit - credit;
        total_income += debit;
        total_expense += credit;
        // 对账状态：方向侧金额对比已核销额（unallocated / partial / allocated）
        let side = if debit > AMOUNT_TOLERANCE {
            debit
        } else {
            credit
        };
        let remaining = side - allocated;
        let reconcile_status = if allocated <= AMOUNT_TOLERANCE {
            "unallocated"
        } else if remaining <= AMOUNT_TOLERANCE {
            "allocated"
        } else {
            "partial"
        };
        rows.push(FundJournalRow {
            voucher_line_id: line_id,
            voucher_id,
            voucher_date,
            belong_month,
            voucher_no,
            source_type,
            source_id,
            account_code,
            summary,
            partner_name,
            income_amount: debit,
            expense_amount: credit,
            balance,
            allocated_amount: allocated,
            reconcile_status: reconcile_status.to_string(),
        });
    }

    Ok(FundJournal {
        fund_account_id: account.id,
        fund_account_name: account.name,
        account_type: account.account_type,
        from_month: from,
        to_month: to,
        opening_balance,
        closing_balance: balance,
        total_income,
        total_expense,
        rows,
    })
}

// ==================== 银行余额调节表（Task 13，spec 4.10） ====================

/// 对账单余额解析优先级：人工录入 > 当月流水余额列推算 > 上期确认结转 > 0（无任何来源）
fn resolve_statement_balance(
    manual: Option<f64>,
    derived: Option<f64>,
    carried: Option<f64>,
) -> (f64, &'static str) {
    if let Some(v) = manual {
        return (v, "manual");
    }
    if let Some(v) = derived {
        return (v, "derived");
    }
    if let Some(v) = carried {
        return (v, "carried");
    }
    (0.0, "empty")
}

/// 生成（或重新生成）账户某月的余额调节表快照：账面期末、对账单期初/期末、
/// 未达项（未核销流水/分录）、调节后两侧余额与差额。重新生成覆盖旧快照并回到 draft。
///
/// 对账单余额来源：入参覆盖（导入流水无余额列或期初衔接修正时由前端传入）优先；
/// 否则按 (交易日期, id) 排序用流水 `balance` 列推算（期初=首行余额−首行收入+首行支出，
/// 期末=末行余额）；当月无流水时结转上一确认期期末；再无则 0。
///
/// 月结保护：已正式月结月份拦截生成（重新生成会把 confirmed 快照拉回 draft，属数据改写）。
pub fn generate_bank_reconciliation_period(
    conn: &Connection,
    fund_account_id: i64,
    month: &str,
    statement_opening: Option<f64>,
    statement_closing: Option<f64>,
) -> AppResult<BankReconciliationPeriod> {
    let month = month.trim();
    if month.len() != 7 || month.as_bytes().get(4) != Some(&b'-') {
        return Err(AppError::InvalidParam("月份格式应为 YYYY-MM".into()));
    }
    ensure_month_open(conn, month)?;
    let account = get_fund_account(conn, fund_account_id)?;

    // 账面期末：账户期初 + ≤当月 active 资金分录净额（与日记账口径一致，可复算）
    let book_net: f64 = conn.query_row(
        "SELECT COALESCE(SUM(vl.debit_amount - vl.credit_amount),0)
         FROM voucher_lines vl
         JOIN vouchers v ON v.id = vl.voucher_id
         WHERE v.status = 'active' AND vl.fund_account_id = ?1 AND v.belong_month <= ?2",
        params![account.id, month],
        |r| r.get(0),
    )?;
    let book_closing_balance = account.opening_balance + book_net;

    // 对账单期初/期末：全部流水（含 ignored，余额列反映真实银行余额）按日期排序推算
    let tx_balances: Vec<(f64, f64, Option<f64>)> = conn
        .prepare(
            "SELECT income_amount, expense_amount, balance FROM bank_transactions
             WHERE fund_account_id = ?1 AND belong_month = ?2
             ORDER BY transaction_date, id",
        )?
        .query_map(params![account.id, month], |r| {
            Ok((
                r.get::<_, f64>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, Option<f64>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let (derived_opening, derived_closing) = match tx_balances.first().zip(tx_balances.last()) {
        Some((first, last)) => (first.2.map(|b| b - first.0 + first.1), last.2),
        None => (None, None),
    };
    let carried: Option<f64> = conn
        .query_row(
            "SELECT statement_closing_balance FROM bank_reconciliation_periods
             WHERE fund_account_id = ?1 AND belong_month < ?2 AND status = 'confirmed'
             ORDER BY belong_month DESC LIMIT 1",
            params![account.id, month],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let (statement_opening_balance, opening_src) =
        resolve_statement_balance(statement_opening, derived_opening, carried);
    let (statement_closing_balance, closing_src) =
        resolve_statement_balance(statement_closing, derived_closing, carried);
    let statement_source = if statement_opening.is_some() || statement_closing.is_some() {
        "manual"
    } else if opening_src == "derived" || closing_src == "derived" {
        "derived"
    } else if opening_src == "carried" || closing_src == "carried" {
        "carried"
    } else {
        "empty"
    };

    // 未达项一：银行已收付、账面未对应的流水（当月本账户未核销部分；已忽略流水不参与）
    let mut detail = BankReconciliationDetail::default();
    let mut outstanding_tx_amount = 0.0;
    let tx_sides: Vec<(i64, String, Option<String>, Option<String>, f64, f64)> = conn
        .prepare(
            "SELECT id, transaction_date, summary, counterparty_name, income_amount, expense_amount
             FROM bank_transactions
             WHERE fund_account_id = ?1 AND belong_month = ?2 AND status != 'ignored'
             ORDER BY transaction_date, id",
        )?
        .query_map(params![account.id, month], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, f64>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, date, summary, counterparty, income, expense) in tx_sides {
        let income_on = income > AMOUNT_TOLERANCE;
        let expense_on = expense > AMOUNT_TOLERANCE;
        if !income_on && !expense_on {
            continue;
        }
        let (direction, side_amount) = bank_tx_direction(income, expense)?;
        let remaining = (side_amount - bank_tx_allocated(conn, id)?).max(0.0);
        if remaining <= AMOUNT_TOLERANCE {
            continue;
        }
        outstanding_tx_amount += if direction == "income" {
            remaining
        } else {
            -remaining
        };
        detail
            .unallocated_transactions
            .push(BankReconciliationOutstandingTx {
                transaction_id: id,
                transaction_date: date,
                summary,
                counterparty_name: counterparty,
                direction: direction.to_string(),
                remaining_amount: remaining,
            });
    }

    // 未达项二：账面已记账、银行未对应的资金分录（≤当月全部未核销部分，跨月未达同样列出）
    let lines: Vec<(i64, String, String, String, String, Option<String>, f64, f64, f64)> = conn
        .prepare(
            "SELECT vl.id, v.voucher_no, v.voucher_date, v.belong_month, vl.account_code,
                    vl.summary, vl.debit_amount, vl.credit_amount,
                    (SELECT COALESCE(SUM(a.allocated_amount),0) FROM bank_reconciliation_allocations a
                     WHERE a.voucher_line_id = vl.id AND a.status = 'active')
             FROM voucher_lines vl
             JOIN vouchers v ON v.id = vl.voucher_id
             WHERE v.status = 'active' AND vl.fund_account_id = ?1 AND v.belong_month <= ?2
             ORDER BY v.voucher_date, v.voucher_no, v.id, vl.line_order",
        )?
        .query_map(params![account.id, month], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, f64>(6)?,
                r.get::<_, f64>(7)?,
                r.get::<_, f64>(8)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut outstanding_line_amount = 0.0;
    for (
        line_id,
        voucher_no,
        voucher_date,
        belong_month,
        account_code,
        summary,
        debit,
        credit,
        allocated,
    ) in lines
    {
        let debit_on = debit > AMOUNT_TOLERANCE;
        let credit_on = credit > AMOUNT_TOLERANCE;
        if !debit_on && !credit_on {
            continue;
        }
        let (side, direction) = if debit_on {
            (debit, "debit")
        } else {
            (credit, "credit")
        };
        let remaining = (side - allocated).max(0.0);
        if remaining <= AMOUNT_TOLERANCE {
            continue;
        }
        outstanding_line_amount += if direction == "debit" {
            remaining
        } else {
            -remaining
        };
        detail
            .unallocated_lines
            .push(BankReconciliationOutstandingLine {
                voucher_line_id: line_id,
                voucher_no,
                voucher_date,
                belong_month,
                account_code,
                summary,
                direction: direction.to_string(),
                remaining_amount: remaining,
            });
    }

    let adjusted_book_balance = book_closing_balance + outstanding_tx_amount;
    let adjusted_bank_balance = statement_closing_balance + outstanding_line_amount;
    let difference = adjusted_bank_balance - adjusted_book_balance;
    let detail_json = serde_json::to_string(&detail)?;

    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO bank_reconciliation_periods
            (fund_account_id, belong_month, statement_opening_balance, statement_closing_balance,
             statement_source, book_closing_balance, outstanding_tx_amount, outstanding_line_amount,
             adjusted_book_balance, adjusted_bank_balance, difference, status, detail_json,
             created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'draft', ?12, ?13, ?13)
         ON CONFLICT(fund_account_id, belong_month) DO UPDATE SET
            statement_opening_balance = excluded.statement_opening_balance,
            statement_closing_balance = excluded.statement_closing_balance,
            statement_source = excluded.statement_source,
            book_closing_balance = excluded.book_closing_balance,
            outstanding_tx_amount = excluded.outstanding_tx_amount,
            outstanding_line_amount = excluded.outstanding_line_amount,
            adjusted_book_balance = excluded.adjusted_book_balance,
            adjusted_bank_balance = excluded.adjusted_bank_balance,
            difference = excluded.difference,
            status = 'draft',
            detail_json = excluded.detail_json,
            confirmed_by = NULL,
            confirmed_at = NULL,
            updated_at = excluded.updated_at",
        params![
            account.id,
            month,
            statement_opening_balance,
            statement_closing_balance,
            statement_source,
            book_closing_balance,
            outstanding_tx_amount,
            outstanding_line_amount,
            adjusted_book_balance,
            adjusted_bank_balance,
            difference,
            detail_json,
            now
        ],
    )?;
    // upsert 走 DO UPDATE 分支时 last_insert_rowid 不可靠（保留的是连接上一次成功 INSERT 的
    // rowid，可能是其他表的），按唯一键回查
    let period_id: i64 = conn.query_row(
        "SELECT id FROM bank_reconciliation_periods
         WHERE fund_account_id = ?1 AND belong_month = ?2",
        params![account.id, month],
        |r| r.get(0),
    )?;
    get_bank_reconciliation_period(conn, period_id)
}

/// 读取单个调节表快照（含账户名回显）
pub fn get_bank_reconciliation_period(
    conn: &Connection,
    id: i64,
) -> AppResult<BankReconciliationPeriod> {
    conn.query_row(
        "SELECT p.id, p.fund_account_id, fa.name, p.belong_month, p.statement_opening_balance,
                p.statement_closing_balance, p.statement_source, p.book_closing_balance,
                p.outstanding_tx_amount, p.outstanding_line_amount, p.adjusted_book_balance,
                p.adjusted_bank_balance, p.difference, p.status, p.detail_json,
                p.confirmed_by, p.confirmed_at, p.created_at, p.updated_at
         FROM bank_reconciliation_periods p
         JOIN fund_accounts fa ON fa.id = p.fund_account_id
         WHERE p.id = ?1",
        params![id],
        row_to_reconciliation_period,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("余额调节表 ID={id} 不存在")))
}

fn row_to_reconciliation_period(
    r: &rusqlite::Row<'_>,
) -> rusqlite::Result<BankReconciliationPeriod> {
    Ok(BankReconciliationPeriod {
        id: r.get(0)?,
        fund_account_id: r.get(1)?,
        fund_account_name: r.get(2)?,
        belong_month: r.get(3)?,
        statement_opening_balance: r.get(4)?,
        statement_closing_balance: r.get(5)?,
        statement_source: r.get(6)?,
        book_closing_balance: r.get(7)?,
        outstanding_tx_amount: r.get(8)?,
        outstanding_line_amount: r.get(9)?,
        adjusted_book_balance: r.get(10)?,
        adjusted_bank_balance: r.get(11)?,
        difference: r.get(12)?,
        status: r.get(13)?,
        detail_json: r.get(14)?,
        confirmed_by: r.get(15)?,
        confirmed_at: r.get(16)?,
        created_at: r.get(17)?,
        updated_at: r.get(18)?,
    })
}

/// 调节表快照列表（按账户/月份过滤，月份降序）
pub fn list_bank_reconciliation_periods(
    conn: &Connection,
    fund_account_id: Option<i64>,
    month: Option<&str>,
) -> AppResult<Vec<BankReconciliationPeriod>> {
    let mut where_clauses = vec!["1=1".to_string()];
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let mut idx = 1usize;
    if let Some(account_id) = fund_account_id {
        where_clauses.push(format!("p.fund_account_id = ?{idx}"));
        params_vec.push(Box::new(account_id));
        idx += 1;
    }
    if let Some(m) = clean_month_param(month) {
        where_clauses.push(format!("p.belong_month = ?{idx}"));
        params_vec.push(Box::new(m));
    }
    let sql = format!(
        "SELECT p.id, p.fund_account_id, fa.name, p.belong_month, p.statement_opening_balance,
                p.statement_closing_balance, p.statement_source, p.book_closing_balance,
                p.outstanding_tx_amount, p.outstanding_line_amount, p.adjusted_book_balance,
                p.adjusted_bank_balance, p.difference, p.status, p.detail_json,
                p.confirmed_by, p.confirmed_at, p.created_at, p.updated_at
         FROM bank_reconciliation_periods p
         JOIN fund_accounts fa ON fa.id = p.fund_account_id
         WHERE {}
         ORDER BY p.belong_month DESC, p.fund_account_id",
        where_clauses.join(" AND ")
    );
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_refs.as_slice(), row_to_reconciliation_period)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 确认余额调节表（spec 4.10 确认条件，全部通过才落 confirmed）：
/// 1) 调节后两侧差额 < 0.005；2) 对账单期初与上一确认期期末衔接；3) 当月无待归集流水。
/// 已确认期间重复确认幂等返回原快照，不重复写确认信息。
/// 月结保护：已正式月结月份拦截 draft→confirmed（幂等回读不受影响，参照核销取消的用法）。
pub fn confirm_bank_reconciliation_period(
    conn: &Connection,
    id: i64,
    operator: &str,
) -> AppResult<BankReconciliationPeriod> {
    let period = get_bank_reconciliation_period(conn, id)?;
    if period.status == "confirmed" {
        return Ok(period);
    }
    // 月结保护按调节表归属月份控制（幂等回读已提前返回，不影响已确认快照）
    ensure_month_open(conn, &period.belong_month)?;

    // 门槛 1：调节后差额
    if period.difference.abs() > AMOUNT_TOLERANCE {
        return Err(AppError::InvalidParam(format!(
            "调节后差额 {:.2} 超过容差 0.005，请先处理未达项或修正对账单余额（详见未达项清单）",
            period.difference
        )));
    }

    // 门槛 2：期初衔接（与上一确认期对账单期末比对；无上一确认期视为首期跳过）
    let prev: Option<(String, f64)> = conn
        .query_row(
            "SELECT belong_month, statement_closing_balance FROM bank_reconciliation_periods
             WHERE fund_account_id = ?1 AND belong_month < ?2 AND status = 'confirmed'
             ORDER BY belong_month DESC LIMIT 1",
            params![period.fund_account_id, period.belong_month],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)),
        )
        .optional()?;
    if let Some((prev_month, prev_closing)) = prev {
        if (prev_closing - period.statement_opening_balance).abs() > AMOUNT_TOLERANCE {
            return Err(AppError::InvalidParam(format!(
                "对账单期初不衔接：{prev_month} 确认期末 {prev_closing:.2}，本期期初 {:.2}",
                period.statement_opening_balance
            )));
        }
    }

    // 门槛 3：当月无待归集流水（未归集账户的流水无法与账面勾稽，spec 4.10）
    let unassigned: i64 = conn.query_row(
        "SELECT COUNT(*) FROM bank_transactions
         WHERE belong_month = ?1 AND status != 'ignored' AND fund_account_id IS NULL",
        params![period.belong_month],
        |r| r.get(0),
    )?;
    if unassigned > 0 {
        return Err(AppError::InvalidParam(format!(
            "当月存在 {unassigned} 条待归集银行流水，请先完成历史归集再确认调节表"
        )));
    }

    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE bank_reconciliation_periods
         SET status = 'confirmed', confirmed_by = ?1, confirmed_at = ?2, updated_at = ?2
         WHERE id = ?3 AND status = 'draft'",
        params![operator, now, id],
    )?;
    get_bank_reconciliation_period(conn, id)
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

    /// 测试专用：独立事务执行 mark_document_batched_in_tx（生产路径在批次事务内调用，语义一致）
    fn mark_batched_standalone(
        conn: &Connection,
        current: &CurrentOperatorState,
        document_id: i64,
        batch_id: i64,
    ) -> AppResult<FundDocument> {
        let tx = conn.unchecked_transaction()?;
        let doc = mark_document_batched_in_tx(&tx, current, document_id, batch_id)?;
        tx.commit()?;
        Ok(doc)
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
        let batched = mark_batched_standalone(&conn, &current, payment.id, batch_id).unwrap();
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
        assert!(mark_batched_standalone(&conn, &current, d.id, 1)
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
        assert!(mark_batched_standalone(&conn, &current, s.id, 1)
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
        assert!(mark_batched_standalone(&conn, &current, a.id, 1)
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
        let b = mark_batched_standalone(&conn, &current, p.id, batch_id).unwrap();
        assert_eq!(b.status, "batched");
        assert!(void_fund_document(&conn, &current, p.id, "x")
            .unwrap_err()
            .to_string()
            .contains("不允许"));
        assert!(mark_batched_standalone(&conn, &current, p.id, batch_id)
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
        assert!(mark_batched_standalone(&conn, &current, pb.id, 999)
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

    // ---------- 资金单凭证联动（Task 8，spec 4.7） ----------

    /// 走完 创建→提交→审批→结算 全流程。
    /// 付款/借款单按 spec 5.1/5.3 经付款批次流转（直插批次行 + 状态机标记 batched）。
    fn settled_document(
        conn: &Connection,
        current: &CurrentOperatorState,
        input: &FundDocumentInput,
    ) -> FundDocument {
        let doc = create_fund_document(conn, current, input).unwrap();
        submit_fund_document(conn, current, doc.id, None).unwrap();
        approve_fund_document(conn, current, doc.id, "ok").unwrap();
        if BATCHABLE_TYPES.contains(&doc.document_type.as_str()) {
            let batch_no = format!("PAY-T8-{}", doc.id);
            conn.execute(
                "INSERT INTO payment_batches
                    (batch_no, belong_month, batch_type, status, created_at, updated_at)
                 VALUES (?1, ?2, 'general', 'approved', '2026-08-05', '2026-08-05')",
                params![batch_no, doc.belong_month],
            )
            .unwrap();
            let batch_id = conn.last_insert_rowid();
            mark_batched_standalone(conn, current, doc.id, batch_id).unwrap();
        }
        settle_fund_document(conn, current, doc.id).unwrap()
    }

    /// 取某资金单当前生效凭证（无则 panic）
    fn active_fund_voucher(conn: &Connection, doc_id: i64) -> Voucher {
        accounting::get_active_voucher_for_source(conn, "fund_document", doc_id)
            .unwrap()
            .unwrap_or_else(|| panic!("单据 {doc_id} 应有生效凭证"))
    }

    /// fund_document 源凭证总数
    fn fund_voucher_count(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM vouchers WHERE source_type = 'fund_document'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// 五类单据结算凭证：分录方向、资金行 fund_account_id、对方行为空、借贷平衡（spec 4.7）。
    #[test]
    fn test_settle_generates_vouchers_for_all_document_types() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);

        // 收款：借 1002 目标账户（带辅助）/ 贷 1122 对方科目
        let receipt = settled_document(&conn, &current, &receipt_input(&fx));
        let v = active_fund_voucher(&conn, receipt.id);
        assert_eq!(v.source_type, "fund_document");
        assert_eq!(v.source_id, receipt.id);
        assert_eq!(v.belong_month, "2026-08");
        assert_eq!(v.voucher_date, "2026-08-05");
        assert_eq!(v.lines.len(), 2);
        assert_eq!(v.lines[0].account_code, "1002");
        assert_eq!(
            (v.lines[0].debit_amount, v.lines[0].credit_amount),
            (500.0, 0.0)
        );
        assert_eq!(v.lines[0].fund_account_id, Some(fx.bank.id));
        assert_eq!(v.lines[1].account_code, "1122");
        assert_eq!(
            (v.lines[1].debit_amount, v.lines[1].credit_amount),
            (0.0, 500.0)
        );
        assert_eq!(v.lines[1].fund_account_id, None);

        // 付款：借 2202 对方科目 / 贷 1002 来源账户（带辅助）
        let payment = settled_document(&conn, &current, &payment_input(&fx));
        let v = active_fund_voucher(&conn, payment.id);
        assert_eq!(v.lines[0].account_code, "2202");
        assert_eq!(v.lines[0].fund_account_id, None);
        assert_eq!(v.lines[1].account_code, "1002");
        assert_eq!(v.lines[1].credit_amount, payment.amount);
        assert_eq!(v.lines[1].fund_account_id, Some(fx.bank.id));

        // 内部转账：借目标(1001 现金) / 贷来源(1002 银行)，两行各带对应账户
        let transfer = settled_document(&conn, &current, &transfer_input(&fx));
        let v = active_fund_voucher(&conn, transfer.id);
        assert_eq!(v.lines[0].account_code, "1001");
        assert_eq!(v.lines[0].fund_account_id, Some(fx.cash.id));
        assert_eq!(v.lines[1].account_code, "1002");
        assert_eq!(v.lines[1].fund_account_id, Some(fx.bank.id));

        // 员工借款：借 1221 其他应收款（默认对方科目）/ 贷 1002 来源账户
        let advance = settled_document(&conn, &current, &advance_input(&fx));
        let v = active_fund_voucher(&conn, advance.id);
        assert_eq!(v.lines[0].account_code, "1221");
        assert_eq!(v.lines[0].fund_account_id, None);
        assert_eq!(v.lines[1].account_code, "1002");
        assert_eq!(v.lines[1].fund_account_id, Some(fx.bank.id));

        // 借款核销：借 1001 目标账户（资金回流）/ 贷 1221
        let settlement = settled_document(&conn, &current, &settlement_input(&fx));
        let v = active_fund_voucher(&conn, settlement.id);
        assert_eq!(v.lines[0].account_code, "1001");
        assert_eq!(v.lines[0].fund_account_id, Some(fx.cash.id));
        assert_eq!(v.lines[1].account_code, "1221");
        assert_eq!(v.lines[1].fund_account_id, None);

        // 全部凭证借贷平衡且金额等于单据金额
        for doc_id in [
            receipt.id,
            payment.id,
            transfer.id,
            advance.id,
            settlement.id,
        ] {
            let v = active_fund_voucher(&conn, doc_id);
            let debit: f64 = v.lines.iter().map(|l| l.debit_amount).sum();
            let credit: f64 = v.lines.iter().map(|l| l.credit_amount).sum();
            assert!((debit - credit).abs() < AMOUNT_TOLERANCE, "凭证应借贷平衡");
            assert_eq!(v.total_amount, 500.0);
        }
    }

    /// 冲正凭证（spec 4.7）：复制原凭证交换借贷、source_id 指向冲正单；
    /// 原凭证保留 active（红字冲销口径，两单并存账面净影响归零）；冲正的冲正回到原方向。
    #[test]
    fn test_reverse_generates_swapped_voucher_and_keeps_original_active() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);
        let receipt = settled_document(&conn, &current, &receipt_input(&fx));
        let original_voucher = active_fund_voucher(&conn, receipt.id);

        let reversal = reverse_fund_document(
            &conn,
            &current,
            &reverse_input(receipt.id, "2026-08", "2026-08-20"),
        )
        .unwrap();
        let rev_voucher = active_fund_voucher(&conn, reversal.id);

        // 冲正凭证：借贷互换（行序随原凭证）、资金行辅助账户随科目保留、备注追溯原单
        // 原凭证 [1002 借, 1122 贷] → 冲正凭证 [1002 贷, 1122 借]
        assert_eq!(rev_voucher.lines.len(), 2);
        assert_eq!(rev_voucher.lines[0].account_code, "1002");
        assert_eq!(
            (
                rev_voucher.lines[0].debit_amount,
                rev_voucher.lines[0].credit_amount
            ),
            (0.0, 500.0)
        );
        assert_eq!(rev_voucher.lines[0].fund_account_id, Some(fx.bank.id));
        assert_eq!(rev_voucher.lines[1].account_code, "1122");
        assert_eq!(
            (
                rev_voucher.lines[1].debit_amount,
                rev_voucher.lines[1].credit_amount
            ),
            (500.0, 0.0)
        );
        assert_eq!(rev_voucher.lines[1].fund_account_id, None);
        assert!(rev_voucher
            .remark
            .as_deref()
            .unwrap_or("")
            .contains(&receipt.document_no));

        // 原凭证保留 active 且未被改动
        let still = accounting::get_active_voucher_for_source(&conn, "fund_document", receipt.id)
            .unwrap()
            .expect("原凭证应保留 active");
        assert_eq!(still.id, original_voucher.id);
        assert_eq!(still.status, "active");

        // 原凭证 + 冲正凭证并存：借方合计 = 贷方合计（账面净影响归零）
        let (mut debit, mut credit) = (0.0, 0.0);
        for v in [&still, &rev_voucher] {
            for l in &v.lines {
                debit += l.debit_amount;
                credit += l.credit_amount;
            }
        }
        assert!((debit - credit).abs() < AMOUNT_TOLERANCE);

        // 冲正的冲正：生成回冲凭证回到原方向（借 1002 带辅助账户 / 贷 1122）
        let reversal2 = reverse_fund_document(
            &conn,
            &current,
            &reverse_input(reversal.id, "2026-08", "2026-08-25"),
        )
        .unwrap();
        let v2 = active_fund_voucher(&conn, reversal2.id);
        assert_eq!(v2.lines[0].account_code, "1002");
        assert_eq!(
            (v2.lines[0].debit_amount, v2.lines[0].credit_amount),
            (500.0, 0.0)
        );
        assert_eq!(v2.lines[0].fund_account_id, Some(fx.bank.id));
        assert_eq!(v2.lines[1].account_code, "1122");
        assert_eq!(
            (v2.lines[1].debit_amount, v2.lines[1].credit_amount),
            (0.0, 500.0)
        );
    }

    /// 防重复：结算前撤回不产生凭证；结算/撤回重走/重复结算各路径后同源凭证唯一，
    /// 部分唯一索引兜底拦截同源 active 重复凭证。
    #[test]
    fn test_settle_voucher_not_duplicated_on_retry_or_resettle() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);
        let doc = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, doc.id, None).unwrap();
        // 撤回（结算前）：不产生凭证
        withdraw_fund_document(&conn, &current, doc.id, None).unwrap();
        assert_eq!(fund_voucher_count(&conn), 0);

        // 重走 提交→审批→结算：恰好一张凭证
        submit_fund_document(&conn, &current, doc.id, None).unwrap();
        approve_fund_document(&conn, &current, doc.id, "ok").unwrap();
        settle_fund_document(&conn, &current, doc.id).unwrap();
        assert_eq!(fund_voucher_count(&conn), 1);

        // 重复结算被状态机拦截，不产生第二张凭证
        assert!(settle_fund_document(&conn, &current, doc.id).is_err());
        assert_eq!(fund_voucher_count(&conn), 1);

        // 兜底：同源（source_type, source_id）active 凭证被部分唯一索引拦截
        let dup = conn.execute(
            "INSERT INTO vouchers
                (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status)
             VALUES ('V-DUP-001', '2026-08-05', '2026-08', 'fund_document', ?1, 500, 'active')",
            params![doc.id],
        );
        assert!(dup.is_err(), "同源 active 凭证应被部分唯一索引拦截");
    }

    /// 未结算作废（draft/approved → void）不产生任何凭证。
    #[test]
    fn test_void_unsettled_document_generates_no_voucher() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);
        let draft = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        void_fund_document(&conn, &current, draft.id, "不需要了").unwrap();
        assert_eq!(fund_voucher_count(&conn), 0);

        let approved = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, approved.id, None).unwrap();
        approve_fund_document(&conn, &current, approved.id, "ok").unwrap();
        void_fund_document(&conn, &current, approved.id, "审批后作废").unwrap();
        assert_eq!(fund_voucher_count(&conn), 0);
    }

    /// 事务原子性：结算中凭证生成失败（对方科目缺失）时，
    /// 结算状态、凭证、审批事件整体回滚，单据停留在 approved。
    #[test]
    fn test_settle_rolls_back_voucher_and_status_on_failure() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);
        let doc = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        submit_fund_document(&conn, &current, doc.id, None).unwrap();
        approve_fund_document(&conn, &current, doc.id, "ok").unwrap();

        // 模拟数据漂移：对方科目被外部直改 SQL 清空 → 凭证生成失败
        conn.execute(
            "UPDATE fund_documents SET counter_account_code = NULL WHERE id = ?1",
            params![doc.id],
        )
        .unwrap();
        let err = settle_fund_document(&conn, &current, doc.id)
            .unwrap_err()
            .to_string();
        assert!(err.contains("对方科目"), "应因缺少对方科目失败：{err}");

        // 状态、凭证、审批事件均无残留（同事务回滚）
        assert_eq!(get_fund_document(&conn, doc.id).unwrap().status, "approved");
        assert_eq!(fund_voucher_count(&conn), 0);
        let detail = get_fund_document_detail(&conn, doc.id).unwrap();
        let actions: Vec<&str> = detail.events.iter().map(|e| e.action.as_str()).collect();
        assert_eq!(actions, vec!["submit", "approve"]);

        // 修复数据后重试结算成功：一张凭证，状态 settled
        conn.execute(
            "UPDATE fund_documents SET counter_account_code = '1122' WHERE id = ?1",
            params![doc.id],
        )
        .unwrap();
        settle_fund_document(&conn, &current, doc.id).unwrap();
        assert_eq!(fund_voucher_count(&conn), 1);
        assert_eq!(get_fund_document(&conn, doc.id).unwrap().status, "settled");
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

    /// 查询条件：account_id 命中来源或目标任一侧；非法类型/状态入参在拼 SQL 前报错
    /// （Task 7：命令层账户筛选与枚举前置校验）
    #[test]
    fn test_get_fund_documents_account_filter_and_enum_guard() {
        let (conn, current) = fund_doc_env();
        let fx = setup_doc_fixtures(&conn);
        let receipt = create_fund_document(&conn, &current, &receipt_input(&fx)).unwrap();
        let payment = create_fund_document(&conn, &current, &payment_input(&fx)).unwrap();

        // bank 同时是 receipt 的目标、payment 的来源 → 两条全命中
        let by_bank = get_fund_documents(
            &conn,
            &FundDocumentQuery {
                account_id: Some(fx.bank.id),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_bank.len(), 2);

        // cash 未被任何单据引用 → 空列表
        let by_cash = get_fund_documents(
            &conn,
            &FundDocumentQuery {
                account_id: Some(fx.cash.id),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(by_cash.is_empty());

        // 账户 + 类型组合：bank + payment 只命中付款单
        let by_bank_payment = get_fund_documents(
            &conn,
            &FundDocumentQuery {
                account_id: Some(fx.bank.id),
                document_type: Some("payment".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_bank_payment.len(), 1);
        assert_eq!(by_bank_payment[0].id, payment.id);

        // 非法状态/类型入参报错而非静默空列表
        let bad_status = get_fund_documents(
            &conn,
            &FundDocumentQuery {
                status: Some("no_such_status".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(bad_status.to_string().contains("单据状态"));
        let bad_type = get_fund_documents(
            &conn,
            &FundDocumentQuery {
                document_type: Some("no_such_type".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(bad_type.to_string().contains("单据类型"));

        // 合法状态入参仍可正常过滤
        let drafts = get_fund_documents(
            &conn,
            &FundDocumentQuery {
                status: Some("draft".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(drafts.len(), 2);
        assert!(drafts.iter().any(|d| d.id == receipt.id));
    }

    // ---------- 历史资金归集向导（Task 10，spec 9） ----------

    /// 插入一条历史（无账户）银行流水，返回 id
    fn legacy_tx(conn: &Connection, date: &str, month: &str, income: f64, expense: f64) -> i64 {
        conn.execute(
            "INSERT INTO bank_transactions
                (transaction_date, belong_month, summary, income_amount, expense_amount, status, created_at, updated_at)
             VALUES (?1, ?2, '历史流水', ?3, ?4, 'unmatched', '2026-08-01', '2026-08-01')",
            params![date, month, income, expense],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// 插入历史付款批次（fund_account_id NULL），返回 id
    fn legacy_batch(
        conn: &Connection,
        no: &str,
        month: &str,
        batch_type: &str,
        status: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO payment_batches
                (batch_no, belong_month, batch_type, status, total_amount, item_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 500, 1, '2026-08-01', '2026-08-01')",
            params![no, month, batch_type, status],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// 插入一张历史凭证：资金行（fund_code，金额 500，fund_account_id NULL）+ 对方行 6602，
    /// 返回凭证 id
    fn legacy_voucher(
        conn: &Connection,
        no: &str,
        date: &str,
        month: &str,
        source_type: &str,
        source_id: i64,
        status: &str,
        fund_code: &str,
        fund_debit: bool,
    ) -> i64 {
        conn.execute(
            "INSERT INTO vouchers
                (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 500, ?6, '2026-08-01', '2026-08-01')",
            params![no, date, month, source_type, source_id, status],
        )
        .unwrap();
        let vid = conn.last_insert_rowid();
        let (d1, c1, d2, c2) = if fund_debit {
            (500.0, 0.0, 0.0, 500.0)
        } else {
            (0.0, 500.0, 500.0, 0.0)
        };
        conn.execute(
            "INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount, line_order)
             VALUES (?1, ?2, ?3, ?4, 1), (?1, '6602', ?5, ?6, 2)",
            params![vid, fund_code, d1, c1, d2, c2],
        )
        .unwrap();
        vid
    }

    fn tx_account(conn: &Connection, tx_id: i64) -> Option<i64> {
        conn.query_row(
            "SELECT fund_account_id FROM bank_transactions WHERE id = ?1",
            params![tx_id],
            |r| r.get::<_, Option<i64>>(0),
        )
        .unwrap()
    }

    fn batch_account(conn: &Connection, batch_id: i64) -> Option<i64> {
        conn.query_row(
            "SELECT fund_account_id FROM payment_batches WHERE id = ?1",
            params![batch_id],
            |r| r.get::<_, Option<i64>>(0),
        )
        .unwrap()
    }

    /// 取凭证资金行（1001/1002/1012）的 fund_account_id（fixture 中每张凭证恰有一条资金行）
    fn voucher_fund_line_account(conn: &Connection, voucher_id: i64) -> Option<i64> {
        conn.query_row(
            "SELECT fund_account_id FROM voucher_lines
             WHERE voucher_id = ?1 AND account_code IN ('1001','1002','1012')",
            params![voucher_id],
            |r| r.get::<_, Option<i64>>(0),
        )
        .unwrap()
    }

    fn third_party_input(code: &str, name: &str) -> FundAccountInput {
        FundAccountInput {
            account_type: "third_party".into(),
            bank_name: Some("微信".into()),
            gl_account_code: "1012".into(),
            ..bank_input(code, name, "")
        }
    }

    fn migration_input(entity_type: &str, account_id: i64) -> FundAssignmentInput {
        FundAssignmentInput {
            entity_type: entity_type.into(),
            account_id,
            belong_month: None,
            batch_id: None,
        }
    }

    #[test]
    fn test_fund_migration_status_counts_and_grouping() {
        let conn = setup_financial_db();
        let tx1 = legacy_tx(&conn, "2026-07-05", "2026-07", 0.0, 500.0);
        let tx2 = legacy_tx(&conn, "2026-08-05", "2026-08", 300.0, 0.0);
        let batch1 = legacy_batch(&conn, "GZ202607001", "2026-07", "salary", "paid");
        let _batch_void = legacy_batch(&conn, "GZ202608001", "2026-08", "salary", "void");
        // tx1 的生效 bank_manual 凭证（资金行待归集，可通过流水归集联动补齐）
        legacy_voucher(
            &conn,
            "V1",
            "2026-07-05",
            "2026-07",
            "bank_manual",
            tx1,
            "active",
            "1002",
            true,
        );
        // 独立凭证（source_id 无对应流水，无法联动）：资金行只能保持 NULL 或人工处理
        legacy_voucher(
            &conn,
            "V2",
            "2026-07-06",
            "2026-07",
            "bank_manual",
            9999,
            "active",
            "1002",
            true,
        );
        // void 凭证资金行不计入待归集
        legacy_voucher(
            &conn,
            "V3",
            "2026-08-05",
            "2026-08",
            "bank_manual",
            tx2,
            "void",
            "1002",
            true,
        );

        let status = get_fund_migration_status(&conn).unwrap();
        assert_eq!(status.unassigned_bank_transactions, 2);
        assert_eq!(
            status.unassigned_payment_batches, 1,
            "void 批次不计入待归集批次"
        );
        assert_eq!(
            status.unassigned_voucher_lines, 2,
            "void 凭证分录不计入；有效分录 = 联动 1 + 独立 1"
        );
        assert_eq!(status.pending_count, 5);
        assert_eq!(
            status.unlinked_voucher_lines, 1,
            "仅独立凭证分录无法通过批次/流水联动"
        );
        assert_eq!(status.pending_batches.len(), 1);
        assert_eq!(status.pending_batches[0].id, batch1);

        // 按月分组：7 月（1 流水 + 2 分录）、8 月（1 流水 + 0 分录）
        assert_eq!(status.bank_months.len(), 2);
        assert_eq!(status.bank_months[0].belong_month, "2026-07");
        assert_eq!(status.bank_months[0].bank_transactions, 1);
        assert_eq!(status.bank_months[0].voucher_lines, 2);
        assert_eq!(status.bank_months[1].belong_month, "2026-08");
        assert_eq!(status.bank_months[1].bank_transactions, 1);
        assert_eq!(status.bank_months[1].voucher_lines, 0);
    }

    #[test]
    fn test_apply_bank_transaction_assignment_links_bank_manual_vouchers() {
        let conn = setup_financial_db();
        let account =
            save_fund_account(&conn, &bank_input("BANK-001", "基本户", "622200001")).unwrap();
        let tx1 = legacy_tx(&conn, "2026-07-05", "2026-07", 0.0, 500.0);
        let tx2 = legacy_tx(&conn, "2026-07-06", "2026-07", 300.0, 0.0);
        let v1 = legacy_voucher(
            &conn,
            "V1",
            "2026-07-05",
            "2026-07",
            "bank_manual",
            tx1,
            "active",
            "1002",
            true,
        );
        let v2 = legacy_voucher(
            &conn,
            "V2",
            "2026-07-06",
            "2026-07",
            "bank_manual",
            tx2,
            "void",
            "1002",
            true,
        );

        // 预览：单账户唯一候选可预填，但写入必须经 apply（先预览核对数量）
        let preview =
            preview_fund_assignment(&conn, "bank_transaction", account.id, None, None).unwrap();
        assert_eq!(preview.item_count, 2);
        assert_eq!(preview.affected_voucher_lines, 1, "void 凭证分录不联动");
        assert_eq!(preview.skipped_voucher_lines, 0);

        let result = apply_fund_assignment(
            &conn,
            &FundAssignmentInput {
                entity_type: "bank_transaction".into(),
                account_id: account.id,
                belong_month: None,
                batch_id: None,
            },
        )
        .unwrap();
        assert_eq!(result.updated_count, 2);
        assert_eq!(result.linked_voucher_lines_updated, 1);
        assert_eq!(result.skipped_voucher_lines, 0);

        assert_eq!(tx_account(&conn, tx1), Some(account.id));
        assert_eq!(tx_account(&conn, tx2), Some(account.id));
        assert_eq!(
            voucher_fund_line_account(&conn, v1),
            Some(account.id),
            "生效 bank_manual 凭证资金行应联动补账户"
        );
        assert_eq!(
            voucher_fund_line_account(&conn, v2),
            None,
            "void 凭证资金行保持不动"
        );

        // 归集后 app_settings 计数刷新：待归集清零（void 已被口径排除）
        let pending: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'stage7_migration_pending_count'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pending, "0", "归集后待归集计数应归零");
        let last_applied: Option<String> = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'stage7_fund_assignment_last_applied_at'",
                [],
                |r| r.get::<_, Option<String>>(0),
            )
            .unwrap();
        assert!(last_applied.is_some(), "归集时间应写入 app_settings");

        // 状态归零：无待归集月份、无待归集批次
        let status = get_fund_migration_status(&conn).unwrap();
        assert_eq!(status.pending_count, 0);
        assert!(status.bank_months.is_empty());
        assert!(status.pending_batches.is_empty());

        // 重复归集幂等：已归集流水自动跳过，不再重复写
        let again = apply_fund_assignment(
            &conn,
            &FundAssignmentInput {
                entity_type: "bank_transaction".into(),
                account_id: account.id,
                belong_month: None,
                batch_id: None,
            },
        )
        .unwrap();
        assert_eq!(again.updated_count, 0);
        assert_eq!(again.linked_voucher_lines_updated, 0);
    }

    #[test]
    fn test_apply_payment_batch_assignment_links_active_payment_vouchers_and_rejects_repeat() {
        let conn = setup_financial_db();
        let account =
            save_fund_account(&conn, &bank_input("BANK-001", "基本户", "622200001")).unwrap();
        let batch1 = legacy_batch(&conn, "GZ202607001", "2026-07", "salary", "paid");
        let batch2 = legacy_batch(&conn, "GZ202607002", "2026-07", "reimbursement", "paid");
        // 批次 2 先归集到同一账户，验证按批次粒度互不影响
        conn.execute(
            "UPDATE payment_batches SET fund_account_id = ?1 WHERE id = ?2",
            params![account.id, batch2],
        )
        .unwrap();
        // 批次 1 的生效工资付款凭证（贷 1002 资金行 NULL）+ void 报销付款凭证
        let v1 = legacy_voucher(
            &conn,
            "V1",
            "2026-07-28",
            "2026-07",
            "salary_payment",
            batch1,
            "active",
            "1002",
            false,
        );
        let v2 = legacy_voucher(
            &conn,
            "V2",
            "2026-07-28",
            "2026-07",
            "reimbursement_payment",
            batch1,
            "void",
            "1002",
            false,
        );

        let preview =
            preview_fund_assignment(&conn, "payment_batch", account.id, None, Some(batch1))
                .unwrap();
        assert_eq!(preview.item_count, 1);
        assert_eq!(preview.affected_voucher_lines, 1);
        assert_eq!(preview.skipped_voucher_lines, 0);

        let result = apply_fund_assignment(
            &conn,
            &FundAssignmentInput {
                entity_type: "payment_batch".into(),
                account_id: account.id,
                belong_month: None,
                batch_id: Some(batch1),
            },
        )
        .unwrap();
        assert_eq!(result.updated_count, 1);
        assert_eq!(result.linked_voucher_lines_updated, 1);
        assert_eq!(batch_account(&conn, batch1), Some(account.id));
        assert_eq!(
            voucher_fund_line_account(&conn, v1),
            Some(account.id),
            "生效付款凭证资金行应联动补账户"
        );
        assert_eq!(
            voucher_fund_line_account(&conn, v2),
            None,
            "void 凭证资金行保持不动"
        );

        // 重复归集同一批次：拦截并提示（明确报错优于静默空操作）
        let repeat = apply_fund_assignment(
            &conn,
            &FundAssignmentInput {
                entity_type: "payment_batch".into(),
                account_id: account.id,
                belong_month: None,
                batch_id: Some(batch1),
            },
        );
        let err = repeat.unwrap_err();
        assert!(
            err.to_string().contains("已归集"),
            "重复归集批次应拦截：{err}"
        );
    }

    #[test]
    fn test_apply_fund_assignment_skips_gl_mismatch_lines() {
        let conn = setup_financial_db();
        // 第三方账户挂 1012，而历史 bank_manual 凭证资金行科目为 1002：科目不一致不能强改
        let account = save_fund_account(&conn, &third_party_input("WX-001", "微信商户")).unwrap();
        let tx = legacy_tx(&conn, "2026-07-05", "2026-07", 0.0, 500.0);
        let v = legacy_voucher(
            &conn,
            "V1",
            "2026-07-05",
            "2026-07",
            "bank_manual",
            tx,
            "active",
            "1002",
            true,
        );

        let preview =
            preview_fund_assignment(&conn, "bank_transaction", account.id, None, None).unwrap();
        assert_eq!(preview.item_count, 1);
        assert_eq!(preview.affected_voucher_lines, 0, "科目不一致不计入可联动");
        assert_eq!(preview.skipped_voucher_lines, 1);

        let result =
            apply_fund_assignment(&conn, &migration_input("bank_transaction", account.id)).unwrap();
        assert_eq!(result.updated_count, 1, "流水本体仍应归集到指定账户");
        assert_eq!(result.linked_voucher_lines_updated, 0);
        assert_eq!(result.skipped_voucher_lines, 1);
        assert_eq!(
            voucher_fund_line_account(&conn, v),
            None,
            "科目不匹配的分录保持 NULL（spec 9.5：不猜测）"
        );
    }

    #[test]
    fn test_apply_fund_assignment_blocks_closed_month() {
        let conn = setup_financial_db();
        let account =
            save_fund_account(&conn, &bank_input("BANK-001", "基本户", "622200001")).unwrap();
        let tx = legacy_tx(&conn, "2026-07-05", "2026-07", 0.0, 500.0);
        legacy_voucher(
            &conn,
            "V1",
            "2026-07-05",
            "2026-07",
            "bank_manual",
            tx,
            "active",
            "1002",
            true,
        );
        conn.execute(
            "INSERT INTO month_closes (month, status, created_at, updated_at)
             VALUES ('2026-07', 'closed', '2026-08-01', '2026-08-01')",
            [],
        )
        .unwrap();

        let err = apply_fund_assignment(&conn, &migration_input("bank_transaction", account.id))
            .unwrap_err();
        assert!(err.to_string().contains("月结"), "已月结月份应拦截：{err}");
        assert_eq!(tx_account(&conn, tx), None, "拦截后不得写入");
        let status = get_fund_migration_status(&conn).unwrap();
        assert_eq!(status.pending_count, 2, "拦截后待归集计数不变");
    }

    #[test]
    fn test_apply_fund_assignment_validates_account_and_entity() {
        let conn = setup_financial_db();
        let mut inactive = bank_input("BANK-001", "基本户", "622200001");
        inactive.is_active = Some(false);
        let account = save_fund_account(&conn, &inactive).unwrap();

        // 账户不存在
        let missing =
            apply_fund_assignment(&conn, &migration_input("bank_transaction", 9999)).unwrap_err();
        assert!(missing.to_string().contains("资金账户不存在"));

        // 停用账户不可作为归集目标（与全应用资金账户选择口径一致）
        let disabled =
            apply_fund_assignment(&conn, &migration_input("bank_transaction", account.id))
                .unwrap_err();
        assert!(disabled.to_string().contains("停用"));

        // 不支持的归集对象类型
        let account2 =
            save_fund_account(&conn, &bank_input("BANK-002", "一般户", "622200002")).unwrap();
        let bad_type = apply_fund_assignment(&conn, &migration_input("voucher_line", account2.id))
            .unwrap_err();
        assert!(bad_type.to_string().contains("归集对象类型"));

        // 预览同样校验账户与类型
        assert!(preview_fund_assignment(&conn, "bank_transaction", 9999, None, None).is_err());
        assert!(
            preview_fund_assignment(&conn, "payment_batch", account.id, None, None).is_err(),
            "停用账户不可预览归集"
        );
    }

    // ==================== 银行流水多对多核销（Task 12，spec 4.9/6.2/6.3） ====================

    /// 直插一张 active/void 凭证 + 一条资金分录（挂指定资金账户），返回分录 id
    #[allow(clippy::too_many_arguments)]
    fn insert_fund_line(
        conn: &Connection,
        voucher_no: &str,
        voucher_date: &str,
        month: &str,
        source_type: &str,
        source_id: i64,
        account_id: i64,
        debit: f64,
        credit: f64,
        summary: &str,
        status: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id,
                total_amount, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '2026-08-05', '2026-08-05')",
            params![
                voucher_no,
                voucher_date,
                month,
                source_type,
                source_id,
                debit + credit,
                status
            ],
        )
        .unwrap();
        let voucher_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount,
                summary, fund_account_id, line_order)
             VALUES (?1, '1002', ?2, ?3, ?4, ?5, 1)",
            params![voucher_id, debit, credit, summary, account_id],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// 直插一条银行流水，返回流水 id
    #[allow(clippy::too_many_arguments)]
    fn insert_tx(
        conn: &Connection,
        date: &str,
        month: &str,
        summary: &str,
        counterparty: &str,
        counter_account: &str,
        income: f64,
        expense: f64,
        account_id: Option<i64>,
        status: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO bank_transactions (transaction_date, belong_month, summary,
                counterparty_name, counterparty_account, income_amount, expense_amount, balance,
                status, fund_account_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1000, ?8, ?9, '2026-08-05', '2026-08-05')",
            params![
                date,
                month,
                summary,
                counterparty,
                counter_account,
                income,
                expense,
                status,
                account_id
            ],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn alloc_input(tx: i64, line: i64, amount: f64) -> BankAllocationInput {
        BankAllocationInput {
            transaction_id: tx,
            voucher_line_id: line,
            allocated_amount: amount,
            score: None,
            remark: None,
        }
    }

    /// 单项核销的失败断言：引擎逐项返回结果，单条失败时应 confirmed=0 且错误可读
    fn confirm_expect_error(conn: &Connection, item: &BankAllocationInput) -> String {
        let mut r =
            confirm_bank_allocations(conn, std::slice::from_ref(item), "manual", "出纳").unwrap();
        assert_eq!(r.confirmed, 0, "不应有成功项：{:?}", r.allocation_ids);
        assert_eq!(r.errors.len(), 1, "错误明细应有一条：{:?}", r.errors);
        r.errors.remove(0)
    }

    fn alloc_env() -> (Connection, i64) {
        let conn = setup_financial_db();
        let account =
            save_fund_account(&conn, &bank_input("BANK-AL", "对账户", "62220088")).unwrap();
        (conn, account.id)
    }

    /// 流水剩余可核销额（经单条候选预览回读；无候选时为 0）
    fn tx_remaining_via_preview(conn: &Connection, tx: i64) -> f64 {
        let item = preview_bank_allocation_candidates(conn, tx).unwrap();
        item.candidates
            .first()
            .map(|c| c.remaining_amount)
            .unwrap_or(0.0)
    }

    /// 一对一部分核销 + 流水侧/分录侧累计超额拦截（spec 4.9）
    #[test]
    fn test_allocation_partial_and_overrun_both_sides() {
        let (conn, acc) = alloc_env();
        let line = insert_fund_line(
            &conn,
            "JZ-AL-001",
            "2026-08-05",
            "2026-08",
            "bank_manual",
            901,
            acc,
            0.0,
            100.0,
            "采购付款",
            "active",
        );
        let tx = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "采购付款",
            "供应商甲",
            "62220001",
            0.0,
            100.0,
            Some(acc),
            "unmatched",
        );

        // 部分核销 40：流水侧剩余 60
        let r = confirm_bank_allocations(&conn, &[alloc_input(tx, line, 40.0)], "manual", "出纳")
            .unwrap();
        assert_eq!(r.confirmed, 1);
        assert_eq!(r.allocation_ids.len(), 1);
        let remaining = tx_remaining_via_preview(&conn, tx);
        assert!(
            (remaining - 60.0).abs() < 0.005,
            "流水侧剩余应为 60，实际 {remaining}"
        );

        // 累计超额（流水侧）：40 + 61 > 100 → 拒绝
        let err = confirm_expect_error(&conn, &alloc_input(tx, line, 61.0));
        assert!(err.contains("超出"), "流水侧超额应拦截：{err}");

        // 分录侧超额：新流水 200 核销只有 100 余额的分录
        let tx2 = insert_tx(
            &conn,
            "2026-08-07",
            "2026-08",
            "大额付款",
            "供应商甲",
            "62220001",
            0.0,
            200.0,
            Some(acc),
            "unmatched",
        );
        let err = confirm_expect_error(&conn, &alloc_input(tx2, line, 101.0));
        assert!(err.contains("超出"), "分录侧超额应拦截：{err}");

        // 合法补足剩余 60 后，两侧余额归零，再核销 1 元被拦
        confirm_bank_allocations(&conn, &[alloc_input(tx, line, 60.0)], "manual", "出纳").unwrap();
        assert!(
            (tx_remaining_via_preview(&conn, tx)).abs() < 0.005,
            "核销完成后流水侧应无剩余"
        );
        let err = confirm_expect_error(&conn, &alloc_input(tx, line, 1.0));
        assert!(err.contains("超出"), "核销完成后继续核销应拦截：{err}");
    }

    /// 一对多（一条流水核多条分录）与多对一（多条流水核一条分录）
    #[test]
    fn test_allocation_one_to_many_and_many_to_one() {
        let (conn, acc) = alloc_env();
        let line_a = insert_fund_line(
            &conn,
            "JZ-AL-011",
            "2026-08-05",
            "2026-08",
            "fund_document",
            11,
            acc,
            0.0,
            60.0,
            "付款A",
            "active",
        );
        let line_b = insert_fund_line(
            &conn,
            "JZ-AL-012",
            "2026-08-06",
            "2026-08",
            "fund_document",
            12,
            acc,
            0.0,
            40.0,
            "付款B",
            "active",
        );
        let tx = insert_tx(
            &conn,
            "2026-08-07",
            "2026-08",
            "合并付款",
            "供应商乙",
            "62220002",
            0.0,
            100.0,
            Some(acc),
            "unmatched",
        );

        // 一对多：一条 100 的流水拆核 60 + 40
        let r = confirm_bank_allocations(
            &conn,
            &[alloc_input(tx, line_a, 60.0), alloc_input(tx, line_b, 40.0)],
            "manual",
            "出纳",
        )
        .unwrap();
        assert_eq!(r.confirmed, 2);
        assert!(
            tx_remaining_via_preview(&conn, tx).abs() < 0.005,
            "一对多核销后流水侧应无剩余"
        );

        // 多对一：两条流水 30 + 70 合核一条 100 的分录
        let line_c = insert_fund_line(
            &conn,
            "JZ-AL-013",
            "2026-08-06",
            "2026-08",
            "fund_document",
            13,
            acc,
            0.0,
            100.0,
            "付款C",
            "active",
        );
        let tx1 = insert_tx(
            &conn,
            "2026-08-07",
            "2026-08",
            "付款C-1",
            "供应商丙",
            "62220003",
            0.0,
            30.0,
            Some(acc),
            "unmatched",
        );
        let tx2 = insert_tx(
            &conn,
            "2026-08-08",
            "2026-08",
            "付款C-2",
            "供应商丙",
            "62220003",
            0.0,
            70.0,
            Some(acc),
            "unmatched",
        );
        let r = confirm_bank_allocations(
            &conn,
            &[
                alloc_input(tx1, line_c, 30.0),
                alloc_input(tx2, line_c, 70.0),
            ],
            "manual",
            "出纳",
        )
        .unwrap();
        assert_eq!(r.confirmed, 2);
        // 分录侧余额归零：第三条流水再来核销 1 元被拦
        let tx3 = insert_tx(
            &conn,
            "2026-08-08",
            "2026-08",
            "付款C-3",
            "供应商丙",
            "62220003",
            0.0,
            1.0,
            Some(acc),
            "unmatched",
        );
        let err = confirm_expect_error(&conn, &alloc_input(tx3, line_c, 1.0));
        assert!(err.contains("超出"), "分录侧余额耗尽后应拦截：{err}");
    }

    /// 方向拦截（收核贷/付核借拒绝）+ 跨账户拦截 + 收入流水核借方分录正向通过
    #[test]
    fn test_allocation_direction_and_cross_account_blocked() {
        let (conn, acc) = alloc_env();
        let other =
            save_fund_account(&conn, &bank_input("BANK-OTHER", "他行户", "62220099")).unwrap();

        let credit_line = insert_fund_line(
            &conn,
            "JZ-AL-021",
            "2026-08-05",
            "2026-08",
            "fund_document",
            21,
            acc,
            0.0,
            100.0,
            "支出分录",
            "active",
        );
        let debit_line = insert_fund_line(
            &conn,
            "JZ-AL-022",
            "2026-08-05",
            "2026-08",
            "bank_manual",
            22,
            acc,
            100.0,
            0.0,
            "收入分录",
            "active",
        );
        let other_line = insert_fund_line(
            &conn,
            "JZ-AL-023",
            "2026-08-05",
            "2026-08",
            "fund_document",
            23,
            other.id,
            0.0,
            100.0,
            "他行支出分录",
            "active",
        );

        // 付流水核借方分录 → 反方向拒绝
        let pay_tx = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "付款",
            "供应商丁",
            "62220004",
            0.0,
            100.0,
            Some(acc),
            "unmatched",
        );
        let err = confirm_expect_error(&conn, &alloc_input(pay_tx, debit_line, 100.0));
        assert!(err.contains("方向"), "付流水核借方分录应拒绝：{err}");

        // 收流水核贷方分录 → 反方向拒绝
        let income_tx = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "收款",
            "客户甲",
            "62220005",
            100.0,
            0.0,
            Some(acc),
            "unmatched",
        );
        let err = confirm_expect_error(&conn, &alloc_input(income_tx, credit_line, 100.0));
        assert!(err.contains("方向"), "收流水核贷方分录应拒绝：{err}");

        // 付流水核其他账户贷方分录 → 跨账户拒绝
        let err = confirm_expect_error(&conn, &alloc_input(pay_tx, other_line, 100.0));
        assert!(err.contains("账户"), "跨账户核销应拒绝：{err}");

        // 收流水核借方分录 → 正向通过
        confirm_bank_allocations(
            &conn,
            &[alloc_input(income_tx, debit_line, 100.0)],
            "manual",
            "出纳",
        )
        .unwrap();
        assert!(tx_remaining_via_preview(&conn, income_tx).abs() < 0.005);
    }

    /// 取消核销释放余额、保留原记录可追溯；重复取消幂等、不存在报错
    #[test]
    fn test_allocation_cancel_releases_balance_and_keeps_history() {
        let (conn, acc) = alloc_env();
        let line = insert_fund_line(
            &conn,
            "JZ-AL-031",
            "2026-08-05",
            "2026-08",
            "fund_document",
            31,
            acc,
            0.0,
            100.0,
            "付款D",
            "active",
        );
        let tx = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "付款D",
            "供应商戊",
            "62220006",
            0.0,
            100.0,
            Some(acc),
            "unmatched",
        );

        let r = confirm_bank_allocations(&conn, &[alloc_input(tx, line, 100.0)], "manual", "出纳")
            .unwrap();
        let allocation_id = r.allocation_ids[0];

        // 取消：状态标记而非物理删除，原金额保留可追溯
        assert!(cancel_bank_allocation(&conn, allocation_id, "复核人").unwrap());
        let rows = list_bank_allocations(&conn, &BankAllocationQuery::default()).unwrap();
        assert_eq!(rows.len(), 1, "取消后原记录应保留");
        assert_eq!(rows[0].status, "cancelled");
        assert!(
            (rows[0].allocated_amount - 100.0).abs() < 0.005,
            "取消不得篡改原金额"
        );
        assert_eq!(rows[0].operator_name.as_deref(), Some("复核人"));

        // 余额释放：流水侧与分录侧均恢复可核销
        assert!((tx_remaining_via_preview(&conn, tx) - 100.0).abs() < 0.005);
        let r2 = confirm_bank_allocations(&conn, &[alloc_input(tx, line, 100.0)], "manual", "出纳")
            .unwrap();
        assert_eq!(r2.confirmed, 1, "取消后应可重新核销");

        // 重复取消幂等返回 false；不存在报 NotFound
        assert!(!cancel_bank_allocation(&conn, allocation_id, "复核人").unwrap());
        assert!(cancel_bank_allocation(&conn, 99999, "复核人").is_err());
    }

    /// 月结保护：已月结月份流水禁止核销/取消；跨月差异按银行流水月份控制（分录月已结不拦）
    #[test]
    fn test_allocation_month_close_protection() {
        let (conn, acc) = alloc_env();
        // 分录属于已月结的 2026-07，流水属于未月结的 2026-08 → 按流水月份放行
        let july_line = insert_fund_line(
            &conn,
            "JZ-AL-041",
            "2026-07-20",
            "2026-07",
            "fund_document",
            41,
            acc,
            0.0,
            50.0,
            "七月付款",
            "active",
        );
        let tx = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "补核销七月付款",
            "供应商己",
            "62220007",
            0.0,
            50.0,
            Some(acc),
            "unmatched",
        );
        confirm_bank_allocations(&conn, &[alloc_input(tx, july_line, 50.0)], "manual", "出纳")
            .unwrap();

        close_month_direct(&conn, "2026-08");
        // 已月结月份：新增核销与取消核销都拦截
        let err = confirm_expect_error(&conn, &alloc_input(tx, july_line, 1.0));
        assert!(err.contains("月结"), "月结后新增核销应拦截：{err}");
        let rows = list_bank_allocations(&conn, &BankAllocationQuery::default()).unwrap();
        let err = cancel_bank_allocation(&conn, rows[0].id, "出纳")
            .unwrap_err()
            .to_string();
        assert!(err.contains("月结"), "月结后取消核销应拦截：{err}");
    }

    /// 候选过滤（active/同账户/方向相符/有剩余）与评分排序（spec 6.3 因子）
    #[test]
    fn test_allocation_candidates_filtering_and_score_order() {
        let (conn, acc) = alloc_env();

        // 最佳候选：金额与流水完全一致 + 凭证号出现在流水摘要
        let best = insert_fund_line(
            &conn,
            "JZ-AL-051",
            "2026-08-06",
            "2026-08",
            "fund_document",
            51,
            acc,
            0.0,
            80.0,
            "货款 供应商庚 尾号8888",
            "active",
        );
        // 次候选：同账户同方向有剩余但金额不一致、无文本因子
        let worse = insert_fund_line(
            &conn,
            "JZ-AL-052",
            "2026-08-06",
            "2026-08",
            "fund_document",
            52,
            acc,
            0.0,
            55.0,
            "无关摘要",
            "active",
        );
        // 排除项：借方分录（方向不符）、他账户分录（跨账户）、void 凭证分录
        let _debit_only = insert_fund_line(
            &conn,
            "JZ-AL-053",
            "2026-08-06",
            "2026-08",
            "bank_manual",
            53,
            acc,
            100.0,
            0.0,
            "收入分录",
            "active",
        );
        let other_line = insert_fund_line(
            &conn,
            "JZ-AL-054",
            "2026-08-06",
            "2026-08",
            "fund_document",
            54,
            save_fund_line_account(&conn),
            0.0,
            80.0,
            "他行分录",
            "active",
        );
        let _void_line = insert_fund_line(
            &conn,
            "JZ-AL-055",
            "2026-08-06",
            "2026-08",
            "fund_document",
            55,
            acc,
            0.0,
            80.0,
            "已作废凭证分录",
            "void",
        );

        let tx = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "支付 JZ-AL-051 货款 尾号8888",
            "供应商庚",
            "62228888",
            0.0,
            80.0,
            Some(acc),
            "unmatched",
        );

        let item = preview_bank_allocation_candidates(&conn, tx).unwrap();
        let ids: Vec<i64> = item.candidates.iter().map(|c| c.voucher_line_id).collect();
        assert!(ids.contains(&best), "金额一致+凭证号命中应为候选");
        assert!(ids.contains(&worse), "同账户同方向有剩余应为候选");
        assert!(!ids.contains(&_debit_only), "方向不符不得入候选");
        assert!(!ids.contains(&other_line), "跨账户分录不得入候选");
        assert!(!ids.contains(&_void_line), "void 凭证分录不得入候选");

        // 排序：最佳候选第一，评分降序；因子说明非空
        assert_eq!(
            item.candidates[0].voucher_line_id, best,
            "评分最高者应排第一"
        );
        let scores: Vec<i32> = item.candidates.iter().map(|c| c.score).collect();
        let mut sorted = scores.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(scores, sorted, "候选应按分数降序");
        let top = &item.candidates[0];
        assert!(
            top.score > item.candidates.last().unwrap().score,
            "文本/金额因子应拉开分差"
        );
        assert!(!top.score_reasons.is_empty(), "评分因子应可解释");
        assert!(
            top.score_reasons.iter().any(|r| r.contains("金额")),
            "金额一致因子应体现"
        );
        assert!((top.remaining_amount - 80.0).abs() < 0.005);

        // 已核销耗尽的分录不再出现在候选中
        confirm_bank_allocations(&conn, &[alloc_input(tx, best, 80.0)], "manual", "出纳").unwrap();
        let item = preview_bank_allocation_candidates(&conn, tx).unwrap();
        assert!(
            !item.candidates.iter().any(|c| c.voucher_line_id == best),
            "无剩余余额的分录不得再入候选"
        );
    }

    /// 自动匹配只预览不写入；批量确认只处理高置信且无冲突项目（spec 6.2/6.3）
    #[test]
    fn test_auto_match_preview_and_batch_confirm() {
        let (conn, acc) = alloc_env();
        let line1 = insert_fund_line(
            &conn,
            "JZ-AL-061",
            "2026-08-06",
            "2026-08",
            "fund_document",
            61,
            acc,
            0.0,
            100.0,
            "货款一",
            "active",
        );
        let line2 = insert_fund_line(
            &conn,
            "JZ-AL-062",
            "2026-08-06",
            "2026-08",
            "fund_document",
            62,
            acc,
            0.0,
            50.0,
            "货款二",
            "active",
        );
        let tx1 = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "支付 JZ-AL-061",
            "供应商辛",
            "62220008",
            0.0,
            100.0,
            Some(acc),
            "unmatched",
        );
        let tx2 = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "支付 JZ-AL-062",
            "供应商壬",
            "62220009",
            0.0,
            50.0,
            Some(acc),
            "unmatched",
        );

        // 预览只读：返回候选与 score，不写 allocation
        let preview = preview_bank_auto_matches(&conn, "2026-08").unwrap();
        assert_eq!(preview.len(), 2, "两条流水都应有候选");
        assert!(preview.iter().all(|i| !i.candidates.is_empty()));
        let by_tx: Vec<(i64, i64)> = preview
            .iter()
            .map(|i| (i.transaction_id, i.candidates[0].voucher_line_id))
            .collect();
        assert!(
            by_tx.contains(&(tx1, line1)) && by_tx.contains(&(tx2, line2)),
            "最佳候选应各自对应：{by_tx:?}"
        );
        let best_scores: Vec<i32> = preview.iter().map(|i| i.candidates[0].score).collect();
        let mut sorted = best_scores.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(best_scores, sorted, "预览项应按最高分降序");
        assert!(
            list_bank_allocations(&conn, &BankAllocationQuery::default())
                .unwrap()
                .is_empty(),
            "预览不得写入"
        );

        // 批量确认：高置信（金额一致+文本因子）全部确认
        let r = batch_confirm_bank_auto_matches(&conn, "2026-08", 60, "出纳").unwrap();
        assert_eq!(r.confirmed, 2, "两条高置信流水应确认：{:?}", r.errors);
        let rows = list_bank_allocations(&conn, &BankAllocationQuery::default()).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter().all(|a| a.match_method == "auto"),
            "批量确认应记 auto"
        );
        assert!(
            rows.iter().all(|a| a.score.unwrap_or(0) >= 60),
            "批量确认应记录 score"
        );
    }

    /// 批量确认冲突消解：同一分录被多条流水争用时不重复核销（金额守恒）
    #[test]
    fn test_batch_confirm_resolves_line_conflicts() {
        let (conn, acc) = alloc_env();
        let line = insert_fund_line(
            &conn,
            "JZ-AL-071",
            "2026-08-06",
            "2026-08",
            "fund_document",
            71,
            acc,
            0.0,
            80.0,
            "争用分录",
            "active",
        );
        let tx_high = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "支付 JZ-AL-071 采购",
            "供应商A",
            "62220010",
            0.0,
            80.0,
            Some(acc),
            "unmatched",
        );
        let tx_low = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "支付 JZ-AL-071 备用",
            "供应商B",
            "62220011",
            0.0,
            80.0,
            Some(acc),
            "unmatched",
        );

        let r = batch_confirm_bank_auto_matches(&conn, "2026-08", 60, "出纳").unwrap();
        assert_eq!(r.confirmed, 1, "争用分录只能确认一条：{:?}", r.errors);
        assert_eq!(r.skipped, 1, "落败方应跳过");
        let rows = list_bank_allocations(
            &conn,
            &BankAllocationQuery {
                voucher_line_id: Some(line),
                ..Default::default()
            },
        )
        .unwrap();
        let total: f64 = rows
            .iter()
            .filter(|a| a.status == "active")
            .map(|a| a.allocated_amount)
            .sum();
        assert!(
            (total - 80.0).abs() < 0.005,
            "核销总额不得超过分录余额，实际 {total}"
        );
        assert_eq!(rows[0].transaction_id, tx_high, "应保留得分高的流水");
        let _ = tx_low;
    }

    /// 低置信（金额不一致且无文本因子）不自动写入，只留在候选预览（spec 6.2）
    #[test]
    fn test_batch_confirm_skips_low_confidence() {
        let (conn, acc) = alloc_env();
        let _line = insert_fund_line(
            &conn,
            "JZ-AL-081",
            "2026-08-06",
            "2026-08",
            "fund_document",
            81,
            acc,
            0.0,
            90.0,
            "无文本关联",
            "active",
        );
        let _tx = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "无关联付款",
            "供应商C",
            "62220012",
            0.0,
            100.0,
            Some(acc),
            "unmatched",
        );

        let preview = preview_bank_auto_matches(&conn, "2026-08").unwrap();
        assert_eq!(preview.len(), 1, "低置信仍是候选");
        let r = batch_confirm_bank_auto_matches(&conn, "2026-08", 60, "出纳").unwrap();
        assert_eq!(r.confirmed, 0, "低置信不得自动写入");
        assert_eq!(r.skipped, 1);
        assert!(
            list_bank_allocations(&conn, &BankAllocationQuery::default())
                .unwrap()
                .is_empty()
        );
    }

    /// 忽略流水/待归集流水/零金额流水禁止核销
    #[test]
    fn test_allocation_blocks_ignored_unassigned_and_zero_amount() {
        let (conn, acc) = alloc_env();
        let line = insert_fund_line(
            &conn,
            "JZ-AL-091",
            "2026-08-06",
            "2026-08",
            "fund_document",
            91,
            acc,
            0.0,
            100.0,
            "付款E",
            "active",
        );

        let ignored = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "忽略的流水",
            "供应商D",
            "62220013",
            0.0,
            50.0,
            Some(acc),
            "ignored",
        );
        let err = confirm_expect_error(&conn, &alloc_input(ignored, line, 50.0));
        assert!(err.contains("忽略"), "已忽略流水应拦截：{err}");

        let unassigned = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "待归集流水",
            "供应商E",
            "62220014",
            0.0,
            50.0,
            None,
            "unmatched",
        );
        assert!(
            preview_bank_allocation_candidates(&conn, unassigned).is_err(),
            "待归集流水不可预览候选"
        );
        let err = confirm_expect_error(&conn, &alloc_input(unassigned, line, 50.0));
        assert!(err.contains("归集"), "待归集流水应拦截并提示归集：{err}");

        let zero = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "零金额流水",
            "供应商F",
            "62220015",
            0.0,
            0.0,
            Some(acc),
            "unmatched",
        );
        let err = confirm_expect_error(&conn, &alloc_input(zero, line, 10.0));
        assert!(err.contains("方向"), "零金额/方向不明流水应拦截：{err}");
    }

    /// 辅助：再建一个资金账户（跨账户用例）
    fn save_fund_line_account(conn: &Connection) -> i64 {
        save_fund_account(
            conn,
            &bank_input(
                &format!(
                    "BANK-X{}",
                    conn.query_row("SELECT COUNT(*) FROM fund_accounts", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap()
                ),
                "异户",
                &format!(
                    "62223{}",
                    conn.query_row("SELECT COUNT(*) FROM fund_accounts", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap()
                ),
            ),
        )
        .unwrap()
        .id
    }

    // ==================== 资金日记账（Task 13，spec 6.1） ====================

    /// 跨月滚入与稳定排序：期初=账户期初+区间前月份合计；同日按凭证号排序；void 凭证排除
    #[test]
    fn test_fund_journal_rolling_balance_cross_month() {
        let (conn, acc) = alloc_env();
        // 7 月：收入 400
        insert_fund_line(
            &conn,
            "JZ-07-001",
            "2026-07-10",
            "2026-07",
            "fund_document",
            1,
            acc,
            400.0,
            0.0,
            "7月收款",
            "active",
        );
        // 8 月：同日两笔，凭证号决定顺序；再加一笔 void 凭证应被排除
        insert_fund_line(
            &conn,
            "JZ-08-002",
            "2026-08-05",
            "2026-08",
            "bank_manual",
            2,
            acc,
            600.0,
            0.0,
            "8月收款",
            "active",
        );
        insert_fund_line(
            &conn,
            "JZ-08-001",
            "2026-08-05",
            "2026-08",
            "bank_manual",
            3,
            acc,
            0.0,
            250.0,
            "8月付款",
            "active",
        );
        insert_fund_line(
            &conn,
            "JZ-08-003",
            "2026-08-06",
            "2026-08",
            "bank_manual",
            4,
            acc,
            999.0,
            0.0,
            "作废凭证",
            "void",
        );

        // 8 月区间：期初滚入 7 月（1000 + 400 = 1400）
        let journal = get_fund_journal(
            &conn,
            &FundJournalQuery {
                fund_account_id: acc,
                from_month: Some("2026-08".into()),
                to_month: Some("2026-08".into()),
            },
        )
        .unwrap();
        assert_eq!(journal.rows.len(), 2, "void 凭证不入日记账");
        assert!(
            (journal.opening_balance - 1400.0).abs() < 0.005,
            "期初应滚入 7 月：{journal:?}"
        );
        assert_eq!(
            journal.rows[0].voucher_no, "JZ-08-001",
            "同日按凭证号稳定排序"
        );
        assert!((journal.rows[0].balance - 1150.0).abs() < 0.005);
        assert_eq!(journal.rows[1].voucher_no, "JZ-08-002");
        assert!((journal.rows[1].balance - 1750.0).abs() < 0.005);
        assert!((journal.closing_balance - 1750.0).abs() < 0.005);
        assert!((journal.total_income - 600.0).abs() < 0.005);
        assert!((journal.total_expense - 250.0).abs() < 0.005);
        assert_eq!(journal.rows[0].reconcile_status, "unallocated");

        // 全区间：期初即账户期初
        let all = get_fund_journal(
            &conn,
            &FundJournalQuery {
                fund_account_id: acc,
                from_month: None,
                to_month: Some("2026-08".into()),
            },
        )
        .unwrap();
        assert_eq!(all.rows.len(), 3);
        assert!((all.opening_balance - 1000.0).abs() < 0.005);
        assert!((all.closing_balance - 1750.0).abs() < 0.005);

        // 部分核销状态：对 8 月收入行核销 300（借方 600 剩 300）
        let income_line_id: i64 = all
            .rows
            .iter()
            .find(|r| r.voucher_no == "JZ-08-002")
            .unwrap()
            .voucher_line_id;
        let tx = insert_tx(
            &conn,
            "2026-08-05",
            "2026-08",
            "收款300",
            "客户甲",
            "62220099",
            300.0,
            0.0,
            Some(acc),
            "unmatched",
        );
        confirm_bank_allocations(
            &conn,
            &[BankAllocationInput {
                transaction_id: tx,
                voucher_line_id: income_line_id,
                allocated_amount: 300.0,
                score: None,
                remark: None,
            }],
            "manual",
            "出纳",
        )
        .unwrap();
        let after = get_fund_journal(
            &conn,
            &FundJournalQuery {
                fund_account_id: acc,
                from_month: Some("2026-08".into()),
                to_month: Some("2026-08".into()),
            },
        )
        .unwrap();
        let row = after
            .rows
            .iter()
            .find(|r| r.voucher_line_id == income_line_id)
            .unwrap();
        assert_eq!(row.reconcile_status, "partial", "部分核销应标记 partial");
        assert!((row.allocated_amount - 300.0).abs() < 0.005);
    }

    // ==================== 银行余额调节表（Task 13，spec 4.10） ====================

    /// 生成→确认→次月期初衔接：调节后两方勾稽、差额 0、确认链可用
    #[test]
    fn test_bank_reconciliation_period_generate_confirm_chain() {
        let (conn, acc) = alloc_env();
        // 7 月：收款 400 全额核销（账面 1400 = 对账单 1400）
        let line7 = insert_fund_line(
            &conn,
            "JZ-07-001",
            "2026-07-10",
            "2026-07",
            "bank_manual",
            1,
            acc,
            400.0,
            0.0,
            "7月收款",
            "active",
        );
        let tx7 = insert_tx(
            &conn,
            "2026-07-10",
            "2026-07",
            "收款400",
            "客户甲",
            "62220001",
            400.0,
            0.0,
            Some(acc),
            "unmatched",
        );
        insert_tx_balance(&conn, tx7, 1400.0);
        confirm_bank_allocations(&conn, &[alloc_input(tx7, line7, 400.0)], "manual", "出纳")
            .unwrap();

        let july = generate_bank_reconciliation_period(&conn, acc, "2026-07", None, None).unwrap();
        assert!(
            (july.book_closing_balance - 1400.0).abs() < 0.005,
            "{july:?}"
        );
        assert!((july.statement_opening_balance - 1000.0).abs() < 0.005);
        assert!((july.statement_closing_balance - 1400.0).abs() < 0.005);
        assert_eq!(july.statement_source, "derived");
        assert!((july.difference).abs() < 0.005, "无未达项时调节后应相等");
        assert_eq!(july.status, "draft");
        let july = confirm_bank_reconciliation_period(&conn, july.id, "出纳").unwrap();
        assert_eq!(july.status, "confirmed");
        assert_eq!(july.confirmed_by.as_deref(), Some("出纳"));

        // 8 月：收款 300 全额核销 + 支出 500 未核销（银行已付账面未对上）
        let line8 = insert_fund_line(
            &conn,
            "JZ-08-001",
            "2026-08-05",
            "2026-08",
            "bank_manual",
            2,
            acc,
            300.0,
            0.0,
            "8月收款",
            "active",
        );
        let tx8a = insert_tx(
            &conn,
            "2026-08-05",
            "2026-08",
            "收款300",
            "客户甲",
            "62220001",
            300.0,
            0.0,
            Some(acc),
            "unmatched",
        );
        insert_tx_balance(&conn, tx8a, 1700.0);
        confirm_bank_allocations(&conn, &[alloc_input(tx8a, line8, 300.0)], "manual", "出纳")
            .unwrap();
        let tx8b = insert_tx(
            &conn,
            "2026-08-20",
            "2026-08",
            "银行已付未入账",
            "供应商乙",
            "62220002",
            0.0,
            500.0,
            Some(acc),
            "unmatched",
        );
        insert_tx_balance(&conn, tx8b, 1200.0);

        let aug = generate_bank_reconciliation_period(&conn, acc, "2026-08", None, None).unwrap();
        assert!(
            (aug.statement_opening_balance - 1400.0).abs() < 0.005,
            "期初取首行流水推算"
        );
        assert!(
            (aug.statement_closing_balance - 1200.0).abs() < 0.005,
            "期末取末行流水余额"
        );
        assert!((aug.book_closing_balance - 1700.0).abs() < 0.005);
        assert!(
            (aug.outstanding_tx_amount + 500.0).abs() < 0.005,
            "未核销流水净额 -500"
        );
        assert!((aug.outstanding_line_amount).abs() < 0.005);
        assert!((aug.adjusted_book_balance - 1200.0).abs() < 0.005);
        assert!((aug.adjusted_bank_balance - 1200.0).abs() < 0.005);
        assert!(aug.difference.abs() < 0.005);
        // 未达项明细包含未核销流水
        let detail = aug.detail_json.clone().unwrap_or_default();
        assert!(
            detail.contains("银行已付未入账"),
            "未达项明细应含未核销流水：{detail}"
        );
        let confirmed_aug = confirm_bank_reconciliation_period(&conn, aug.id, "出纳").unwrap();
        assert_eq!(confirmed_aug.status, "confirmed");

        // 确认幂等：已确认期间重复确认返回原快照，不重复写确认信息
        let again = confirm_bank_reconciliation_period(&conn, aug.id, "出纳").unwrap();
        assert_eq!(again.id, aug.id);
        assert_eq!(again.status, "confirmed");
        assert_eq!(again.confirmed_at, confirmed_aug.confirmed_at);

        // 查询：按账户两期
        let periods = list_bank_reconciliation_periods(&conn, Some(acc), None).unwrap();
        assert_eq!(periods.len(), 2);
        let by_month =
            list_bank_reconciliation_periods(&conn, Some(acc), Some("2026-08".into())).unwrap();
        assert_eq!(by_month.len(), 1);
        assert_eq!(by_month[0].status, "confirmed");
    }

    /// 确认门槛：调节差额 > 0.005、期初不衔接、存在待归集流水均阻断确认（spec 4.10）
    #[test]
    fn test_bank_reconciliation_confirm_gates() {
        let (conn, acc) = alloc_env();
        let line7 = insert_fund_line(
            &conn,
            "JZ-07-001",
            "2026-07-10",
            "2026-07",
            "bank_manual",
            1,
            acc,
            400.0,
            0.0,
            "7月收款",
            "active",
        );
        let tx7 = insert_tx(
            &conn,
            "2026-07-10",
            "2026-07",
            "收款400",
            "客户甲",
            "62220001",
            400.0,
            0.0,
            Some(acc),
            "unmatched",
        );
        insert_tx_balance(&conn, tx7, 1400.0);
        confirm_bank_allocations(&conn, &[alloc_input(tx7, line7, 400.0)], "manual", "出纳")
            .unwrap();
        confirm_bank_reconciliation_period(
            &conn,
            generate_bank_reconciliation_period(&conn, acc, "2026-07", None, None)
                .unwrap()
                .id,
            "出纳",
        )
        .unwrap();

        // 8 月：支出 500 未核销 + 账面在途收入 100（有分录无流水）
        let tx8 = insert_tx(
            &conn,
            "2026-08-20",
            "2026-08",
            "银行已付未入账",
            "供应商乙",
            "62220002",
            0.0,
            500.0,
            Some(acc),
            "unmatched",
        );
        insert_tx_balance(&conn, tx8, 900.0);
        insert_fund_line(
            &conn,
            "JZ-08-901",
            "2026-08-21",
            "2026-08",
            "bank_manual",
            9,
            acc,
            100.0,
            0.0,
            "在途收款",
            "active",
        );

        // 门槛 1：手工改对账单期末制造差额 → 拒绝确认
        let drifted =
            generate_bank_reconciliation_period(&conn, acc, "2026-08", None, Some(800.0)).unwrap();
        assert_eq!(drifted.statement_source, "manual");
        assert!(
            drifted.difference.abs() > 0.005,
            "差额应反映对账单改动：{drifted:?}"
        );
        let err = confirm_bank_reconciliation_period(&conn, drifted.id, "出纳")
            .unwrap_err()
            .to_string();
        assert!(err.contains("差额"), "差额超容差应拦截：{err}");

        // 门槛 2：对账单期初与上月确认期末不衔接 → 拒绝确认
        //   （期末取流水余额 900 → 期初 = 900+500 = 1400 正确衔接；改期末为期初让衔接检查失败）
        let mismatched =
            generate_bank_reconciliation_period(&conn, acc, "2026-08", Some(1300.0), Some(900.0))
                .unwrap();
        assert!((mismatched.difference).abs() < 0.005, "两侧调节应勾稽平衡");
        let err = confirm_bank_reconciliation_period(&conn, mismatched.id, "出纳")
            .unwrap_err()
            .to_string();
        assert!(err.contains("期初"), "期初不衔接应拦截：{err}");

        // 门槛 3：当月存在待归集流水 → 拒绝确认（流水金额不影响本账户余额也要先归集）
        let _ = insert_tx(
            &conn,
            "2026-08-25",
            "2026-08",
            "待归集流水",
            "未知户",
            "62220003",
            0.0,
            10.0,
            None,
            "unmatched",
        );
        let ok_numbers =
            generate_bank_reconciliation_period(&conn, acc, "2026-08", None, None).unwrap();
        let err = confirm_bank_reconciliation_period(&conn, ok_numbers.id, "出纳")
            .unwrap_err()
            .to_string();
        assert!(err.contains("待归集"), "存在待归集流水应拦截：{err}");

        // 重新生成把 confirmed 期间拉回 draft（数据可能已变化）
        let regenerated =
            generate_bank_reconciliation_period(&conn, acc, "2026-07", None, None).unwrap();
        assert_eq!(regenerated.status, "draft", "重新生成应回到 draft");
    }

    /// 已正式月结月份：生成/重新生成与确认调节表均被拦截，快照状态不被改动（Fix Round 1）
    #[test]
    fn test_bank_reconciliation_period_month_close_guard() {
        let (conn, acc) = alloc_env();

        // 月结前先生成一份 draft 快照，供确认路径拦截验证
        let draft = generate_bank_reconciliation_period(&conn, acc, "2026-08", None, None).unwrap();
        assert_eq!(draft.status, "draft");

        close_month_direct(&conn, "2026-08");

        let err = generate_bank_reconciliation_period(&conn, acc, "2026-08", None, None)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("已正式月结"),
            "已月结月份生成调节表应拦截：{err}"
        );

        let err = confirm_bank_reconciliation_period(&conn, draft.id, "出纳")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("已正式月结"),
            "已月结月份确认调节表应拦截：{err}"
        );
        let still = get_bank_reconciliation_period(&conn, draft.id).unwrap();
        assert_eq!(still.status, "draft", "被拦截的确认不应改动快照状态");
    }

    // ==================== 旧匹配命令退役（Task 13，spec 4.9 双路径防重） ====================

    /// 流水已在新对账引擎核销（allocation>0）时，旧 confirm_bank_transaction_match 必须拦截，
    /// 否则旧引擎不感知 allocation 会造成流水侧已核销虚高（双向双路径）
    #[test]
    fn test_legacy_confirm_intercepted_when_tx_allocated() {
        let (conn, acc) = alloc_env();
        // source_id 用大数避开与流水自增 id 撞车（旧 confirm 会拦截已有 bank_manual 凭证的流水）
        let line = insert_fund_line(
            &conn,
            "JZ-LG-001",
            "2026-08-05",
            "2026-08",
            "bank_manual",
            99001,
            acc,
            0.0,
            500.0,
            "付款500",
            "active",
        );
        let tx = insert_tx(
            &conn,
            "2026-08-05",
            "2026-08",
            "付款500",
            "供应商丙",
            "62220011",
            0.0,
            500.0,
            Some(acc),
            "unmatched",
        );
        conn.execute(
            "INSERT INTO payment_batches (batch_no, belong_month, batch_type, status,
                total_amount, item_count, fund_account_id, created_at, updated_at)
             VALUES ('PB-LEG-1', '2026-08', 'general', 'paid', 500, 1, ?1,
                     '2026-08-05', '2026-08-05')",
            params![acc],
        )
        .unwrap();
        let batch_id = conn.last_insert_rowid();

        // 未核销时旧路径仍可用（历史兼容：旧匹配只读保留一个版本周期）
        crate::db::confirm_bank_transaction_match(
            &conn,
            &BankTransactionMatchInput {
                transaction_id: tx,
                payment_batch_id: batch_id,
                remark: None,
            },
            100,
        )
        .unwrap();
        // 旧匹配占用后流水侧已有核销额，再走新引擎核销同流水属双路径，同样拦截
        let err = confirm_expect_error(&conn, &alloc_input(tx, line, 500.0));
        assert!(
            err.contains("可核销余额"),
            "流水侧已被旧匹配占用应拦截：{err}"
        );

        // 反向：新引擎核销在前，旧 confirm 必须拦截（防 200% 虚高的核心场景）
        let (conn2, acc2) = alloc_env();
        let line2 = insert_fund_line(
            &conn2,
            "JZ-LG-002",
            "2026-08-05",
            "2026-08",
            "bank_manual",
            99002,
            acc2,
            0.0,
            500.0,
            "付款500",
            "active",
        );
        let tx2 = insert_tx(
            &conn2,
            "2026-08-05",
            "2026-08",
            "付款500",
            "供应商丙",
            "62220011",
            0.0,
            500.0,
            Some(acc2),
            "unmatched",
        );
        confirm_bank_allocations(&conn2, &[alloc_input(tx2, line2, 500.0)], "manual", "出纳")
            .unwrap();
        conn2
            .execute(
                "INSERT INTO payment_batches (batch_no, belong_month, batch_type, status,
                total_amount, item_count, fund_account_id, created_at, updated_at)
             VALUES ('PB-LEG-2', '2026-08', 'general', 'paid', 500, 1, ?1,
                     '2026-08-05', '2026-08-05')",
                params![acc2],
            )
            .unwrap();
        let batch2 = conn2.last_insert_rowid();
        let err = crate::db::confirm_bank_transaction_match(
            &conn2,
            &BankTransactionMatchInput {
                transaction_id: tx2,
                payment_batch_id: batch2,
                remark: None,
            },
            100,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("银行对账"),
            "新引擎核销后旧 confirm 应拦截：{err}"
        );
        // 自动匹配（内部调用旧 confirm）同样不得写入
        let result = crate::db::auto_match_bank_transactions(&conn2, "2026-08").unwrap();
        assert_eq!(result.matched, 0, "旧自动匹配不得再写已核销流水");
        let legacy_rows: i64 = conn2
            .query_row("SELECT COUNT(*) FROM bank_transaction_matches", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(legacy_rows, 0, "旧匹配表不得新增行");
    }

    // ==================== 月结检查项（Task 13，spec 8/9.6） ====================

    /// 部分核销流水/分录 → warning；待归集流水 → 迁移完成前 warning、完成后 blocking
    #[test]
    fn test_month_close_bank_partial_allocation_and_unassigned_checks() {
        let (conn, acc) = alloc_env();
        let line = insert_fund_line(
            &conn,
            "JZ-MC-001",
            "2026-08-05",
            "2026-08",
            "bank_manual",
            1,
            acc,
            1000.0,
            0.0,
            "收款1000",
            "active",
        );
        let tx = insert_tx(
            &conn,
            "2026-08-05",
            "2026-08",
            "收款1000",
            "客户甲",
            "62220001",
            1000.0,
            0.0,
            Some(acc),
            "unmatched",
        );
        // 部分核销 400/1000
        confirm_bank_allocations(&conn, &[alloc_input(tx, line, 400.0)], "manual", "出纳").unwrap();

        let workbench = crate::db::get_month_close_workbench(&conn, "2026-08").unwrap();
        let partial = workbench
            .checks
            .iter()
            .find(|c| c.key == "bank_partial_allocation")
            .expect("月结检查应包含部分核销项");
        assert_eq!(
            partial.status, "warning",
            "部分核销为 warning（严格阻塞归 7D）"
        );
        assert!(
            partial.count >= 1,
            "应检出部分核销的流水与分录：{}",
            partial.count
        );

        // 待归集流水：迁移完成前 warning
        let _unassigned = insert_tx(
            &conn,
            "2026-08-06",
            "2026-08",
            "待归集流水",
            "未知户",
            "62220003",
            0.0,
            50.0,
            None,
            "unmatched",
        );
        let workbench = crate::db::get_month_close_workbench(&conn, "2026-08").unwrap();
        let unassigned = workbench
            .checks
            .iter()
            .find(|c| c.key == "fund_unassigned_bank_tx")
            .expect("月结检查应包含待归集流水项");
        assert_eq!(unassigned.status, "warning", "归集向导未执行时应为 warning");
        assert_eq!(unassigned.count, 1);

        // 归集向导执行后（用户确认信号）→ blocking
        conn.execute(
            "INSERT OR REPLACE INTO app_settings (key, value) VALUES
                ('stage7_fund_assignment_last_applied_at', '2026-09-05T00:00:00+00:00')",
            [],
        )
        .unwrap();
        let workbench = crate::db::get_month_close_workbench(&conn, "2026-08").unwrap();
        let unassigned = workbench
            .checks
            .iter()
            .find(|c| c.key == "fund_unassigned_bank_tx")
            .unwrap();
        assert_eq!(
            unassigned.status, "blocking",
            "归集完成后应为 blocking（spec 9.6）"
        );
    }

    /// 直插流水的对账单余额列（调节表推算期初/期末用）
    fn insert_tx_balance(conn: &Connection, tx_id: i64, balance: f64) {
        conn.execute(
            "UPDATE bank_transactions SET balance = ?1 WHERE id = ?2",
            params![balance, tx_id],
        )
        .unwrap();
    }

    /// 日记账与余额调节表 Excel 冒烟：文件生成且非空
    #[test]
    fn test_fund_journal_and_reconciliation_excel_export_smoke() {
        let (conn, acc) = alloc_env();
        let line = insert_fund_line(
            &conn,
            "JZ-EX-001",
            "2026-08-05",
            "2026-08",
            "bank_manual",
            1,
            acc,
            300.0,
            0.0,
            "收款300",
            "active",
        );
        let tx = insert_tx(
            &conn,
            "2026-08-05",
            "2026-08",
            "收款300",
            "客户甲",
            "62220001",
            300.0,
            0.0,
            Some(acc),
            "unmatched",
        );
        insert_tx_balance(&conn, tx, 1300.0);
        confirm_bank_allocations(&conn, &[alloc_input(tx, line, 300.0)], "manual", "出纳").unwrap();

        let journal = get_fund_journal(
            &conn,
            &FundJournalQuery {
                fund_account_id: acc,
                from_month: Some("2026-08".into()),
                to_month: Some("2026-08".into()),
            },
        )
        .unwrap();
        let journal_path =
            std::env::temp_dir().join(format!("fund-journal-{}.xlsx", std::process::id()));
        crate::excel::export_fund_journal_excel(&journal, journal_path.to_str().unwrap()).unwrap();
        assert!(
            journal_path.metadata().unwrap().len() > 0,
            "日记账导出文件非空"
        );

        generate_bank_reconciliation_period(&conn, acc, "2026-08", None, None).unwrap();
        let period_path =
            std::env::temp_dir().join(format!("bank-recon-{}.xlsx", std::process::id()));
        crate::excel::export_bank_reconciliation_excel(
            &list_bank_reconciliation_periods(&conn, Some(acc), Some("2026-08".into()))
                .unwrap()
                .remove(0),
            period_path.to_str().unwrap(),
        )
        .unwrap();
        assert!(
            period_path.metadata().unwrap().len() > 0,
            "调节表导出文件非空"
        );
    }
}
