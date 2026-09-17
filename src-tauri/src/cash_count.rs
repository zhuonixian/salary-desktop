// ==================== 第八阶段：现金盘点单（Task 6，spec 6） ====================
// 流程：新建（限 cash 账户）→ 账面余额自动快照 → 录实存（快速总额 / 面额明细自动合计）→
// 差异自动算（实存 − 账面）→ 确认：盘亏 借 1901 待处理财产损溢 / 贷账户挂接科目（带 fund_account_id），
// 盘盈 借账户挂接科目 / 贷 1901，差异 0 免凭证；draft 可改可作废，confirmed 不可改，
// 作废在有差异凭证时走红字冲正（原凭证保留 active + 借贷互换反向凭证，净影响 0）。
// 账面余额派生复用 cashier::fund_account_balance（voucher_lines 同源，与资金日记账同口径）。
// 凭证归属：差异凭证 source_type='cash_count'、source_id=盘点单 id；冲正凭证 source_id=−盘点单 id
//（vouchers.source_type CHECK 白名单内无第二现金盘点类取值，负 id 段为冲正凭证专用锚点，
// 与差异凭证天然不冲突 idx_vouchers_source_active 唯一索引，反查盘点单凭证恒用正 id）。
//
use crate::accounting;
use crate::cashier::{ensure_fund_voucher_lines, fund_account_balance, fund_account_gl_code};
use crate::db::ensure_month_open;
use crate::errors::{AppError, AppResult};
use crate::models::{
    CashCountCreateInput, CashCountDenomination, CashCountDenominationInput, CashCountSheet,
    VoucherDraft, VoucherLineDraft, CASH_COUNT_STATUS_CONFIRMED, CASH_COUNT_STATUS_DRAFT,
    CASH_COUNT_STATUS_VOID,
};
use chrono::{NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};

/// 金额比较容差（与 cashier::AMOUNT_TOLERANCE 同值，彼处模块私有）
const AMOUNT_TOLERANCE: f64 = 0.005;

/// 盘点凭证 source_type：差异凭证（source_id = 盘点单 id）与冲正凭证（source_id = −盘点单 id）
const VOUCHER_SOURCE_CASH_COUNT: &str = "cash_count";

/// 科目 1901 待处理财产损溢（盘亏借方 / 盘盈贷方，spec 6）
const GL_PENDING_ASSETS: &str = "1901";

// ==================== 入参 / 出参模型 ====================

/// 盘点单修改入参（update_count_sheet，仅 draft）：字段整体替换，
/// 面额明细整表替换（None 视同清空，快速模式传 None/空）；账面余额与差异由后端重算
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CashCountUpdateInput {
    pub count_date: String,
    /// 限 account_type='cash' 的资金账户
    pub fund_account_id: i64,
    /// 实存金额（>= 0）
    pub counted_amount: f64,
    /// 差异原因（差异≠0 时必填）
    pub difference_reason: Option<String>,
    pub remark: Option<String>,
    /// 面额明细整表替换（None 视同清空）
    pub denominations: Option<Vec<CashCountDenominationInput>>,
}

/// 盘点单查询条件（get_count_sheets）：状态/所属月/账户，全部可选，命中即返回
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct CashCountQuery {
    pub status: Option<String>,
    /// 盘点月（YYYY-MM，按 count_date 所在月过滤）
    pub belong_month: Option<String>,
    pub fund_account_id: Option<i64>,
}

/// 盘点单详情：盘点单 + 面额明细（面额降序）
#[derive(Debug, Clone, serde::Serialize)]
pub struct CashCountSheetDetail {
    pub sheet: CashCountSheet,
    pub denominations: Vec<CashCountDenomination>,
}

/// 盘点单状态中文标签（报错展示与命令层操作日志用）
pub fn status_label(status: &str) -> &str {
    match status {
        CASH_COUNT_STATUS_DRAFT => "草稿",
        CASH_COUNT_STATUS_CONFIRMED => "已确认",
        CASH_COUNT_STATUS_VOID => "已作废",
        other => other,
    }
}

// ==================== 查询 ====================

/// 盘点单列表（get_count_sheets）：按 id 降序（新建在前）
pub fn get_count_sheets(
    conn: &Connection,
    query: &CashCountQuery,
) -> AppResult<Vec<CashCountSheet>> {
    let mut conditions: Vec<String> = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(status) = query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        params_vec.push(Box::new(status.to_string()));
        conditions.push(format!("status = ?{}", params_vec.len()));
    }
    if let Some(month) = query
        .belong_month
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        params_vec.push(Box::new(month.to_string()));
        conditions.push(format!("substr(count_date, 1, 7) = ?{}", params_vec.len()));
    }
    if let Some(account) = query.fund_account_id {
        params_vec.push(Box::new(account));
        conditions.push(format!("fund_account_id = ?{}", params_vec.len()));
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "SELECT id, count_date, belong_month, fund_account_id, book_balance, counted_amount,
                difference, difference_reason, status, voucher_id, remark, created_by,
                created_at, updated_at
         FROM cash_count_sheets {where_clause} ORDER BY id DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(
            rusqlite::params_from_iter(params_vec.iter().map(|p| p.as_ref())),
            map_sheet_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 盘点单详情（get_count_sheet_detail）：盘点单 + 面额明细（面额降序）
pub fn get_count_sheet_detail(conn: &Connection, id: i64) -> AppResult<CashCountSheetDetail> {
    let sheet = get_sheet(conn, id)?;
    Ok(CashCountSheetDetail {
        denominations: load_denominations(conn, id)?,
        sheet,
    })
}

// ==================== 通用 helper ====================

/// 校验 ISO 日期（YYYY-MM-DD），返回解析后的日期
fn validate_date(value: &str, label: &str) -> AppResult<NaiveDate> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidParam(format!("{label}必填")));
    }
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").map_err(|_| {
        AppError::InvalidParam(format!("{label}格式无效：{trimmed}（应为 YYYY-MM-DD）"))
    })
}

/// 月份字符串（YYYY-MM）
fn month_of(date: NaiveDate) -> String {
    date.format("%Y-%m").to_string()
}

/// 校验操作人姓名：须为在册且启用的操作人（created_by 署名用）
fn resolve_operator(conn: &Connection, operator: &str) -> AppResult<String> {
    let name = operator.trim();
    if name.is_empty() {
        return Err(AppError::InvalidParam(
            "操作人必填：请先选择当前操作人".into(),
        ));
    }
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM operator_profiles WHERE name = ?1 AND is_active = 1",
        params![name],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Err(AppError::General(format!(
            "操作人「{name}」不存在或已停用，请先选择有效操作人"
        )));
    }
    Ok(name.to_string())
}

/// 读取盘点单（14 列 → CashCountSheet），不存在报 NotFound
fn get_sheet(conn: &Connection, id: i64) -> AppResult<CashCountSheet> {
    conn.query_row(
        "SELECT id, count_date, belong_month, fund_account_id, book_balance, counted_amount,
                difference, difference_reason, status, voucher_id, remark, created_by,
                created_at, updated_at
         FROM cash_count_sheets WHERE id = ?1",
        params![id],
        map_sheet_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("盘点单不存在：id={id}")))
}

/// 盘点单行映射（查询与单条读取共用）
fn map_sheet_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CashCountSheet> {
    Ok(CashCountSheet {
        id: r.get(0)?,
        count_date: r.get(1)?,
        belong_month: r.get(2)?,
        fund_account_id: r.get(3)?,
        book_balance: r.get(4)?,
        counted_amount: r.get(5)?,
        difference: r.get(6)?,
        difference_reason: r.get(7)?,
        status: r.get(8)?,
        voucher_id: r.get(9)?,
        remark: r.get(10)?,
        created_by: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
    })
}

/// 读取盘点单面额明细（面额降序）
fn load_denominations(conn: &Connection, sheet_id: i64) -> AppResult<Vec<CashCountDenomination>> {
    let mut stmt = conn.prepare(
        "SELECT id, sheet_id, denomination, quantity, subtotal
         FROM cash_count_denominations WHERE sheet_id = ?1 ORDER BY denomination DESC",
    )?;
    let rows = stmt
        .query_map(params![sheet_id], |r| {
            Ok(CashCountDenomination {
                id: r.get(0)?,
                sheet_id: r.get(1)?,
                denomination: r.get(2)?,
                quantity: r.get(3)?,
                subtotal: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 校验现金账户：必须存在且 account_type='cash'（spec 6 约束），返回账户名供摘要用
fn require_cash_account(conn: &Connection, fund_account_id: i64) -> AppResult<String> {
    let (name, account_type): (String, String) = conn
        .query_row(
            "SELECT name, account_type FROM fund_accounts WHERE id = ?1",
            params![fund_account_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("资金账户不存在：id={fund_account_id}")))?;
    if account_type != "cash" {
        return Err(AppError::InvalidParam(format!(
            "现金盘点仅支持现金类账户，账户「{name}」类型为 {account_type}"
        )));
    }
    Ok(name)
}

/// 校验面额明细行：面额 > 0、张数 >= 0、同面额不重复；返回规范副本
fn normalize_denominations(
    input: &[CashCountDenominationInput],
) -> AppResult<Vec<CashCountDenominationInput>> {
    let mut seen: Vec<i64> = Vec::new();
    let mut rows = Vec::with_capacity(input.len());
    for d in input {
        if d.denomination <= AMOUNT_TOLERANCE {
            return Err(AppError::InvalidParam(format!(
                "面额必须大于 0：{}",
                d.denomination
            )));
        }
        if d.quantity < 0 {
            return Err(AppError::InvalidParam(format!(
                "面额 {} 张数不能为负：{}",
                d.denomination, d.quantity
            )));
        }
        // 同面额去重（1e-6 内视同重复）：避免合计口径含糊
        let key = (d.denomination * 1_000_000.0).round() as i64;
        if seen.contains(&key) {
            return Err(AppError::InvalidParam(format!(
                "面额 {} 重复录入，请合并为一条",
                d.denomination
            )));
        }
        seen.push(key);
        rows.push(CashCountDenominationInput {
            denomination: d.denomination,
            quantity: d.quantity,
        });
    }
    Ok(rows)
}

/// 面额合计校验（spec 6：提供面额明细时合计必须等于实存金额；空明细=快速模式跳过）
fn validate_denomination_total(
    counted_amount: f64,
    denominations: &[CashCountDenominationInput],
) -> AppResult<f64> {
    if denominations.is_empty() {
        return Ok(0.0);
    }
    let total: f64 = denominations
        .iter()
        .map(|d| d.denomination * d.quantity as f64)
        .sum();
    if (total - counted_amount).abs() > AMOUNT_TOLERANCE {
        return Err(AppError::InvalidParam(format!(
            "面额明细合计 {:.2} 与实存金额 {:.2} 不一致，请核对后保存",
            total, counted_amount
        )));
    }
    Ok(total)
}

/// 面额明细整表替换（draft 保存用）：先删后插，subtotal = denomination × quantity 入库校验。
/// 必须与盘点单更新同事务调用（ON DELETE CASCADE 依赖外键开启，此处显式删除兼容各环境）。
fn replace_denominations(
    conn: &Connection,
    sheet_id: i64,
    denominations: &[CashCountDenominationInput],
) -> AppResult<()> {
    conn.execute(
        "DELETE FROM cash_count_denominations WHERE sheet_id = ?1",
        params![sheet_id],
    )?;
    for d in denominations {
        let subtotal = d.denomination * d.quantity as f64;
        conn.execute(
            "INSERT INTO cash_count_denominations (sheet_id, denomination, quantity, subtotal)
             VALUES (?1, ?2, ?3, ?4)",
            params![sheet_id, d.denomination, d.quantity, subtotal],
        )?;
    }
    Ok(())
}

/// 差异原因门禁：差异≠0 时原因必填（spec 6）
fn require_reason_if_difference(
    difference: f64,
    reason: Option<&str>,
    context: &str,
) -> AppResult<()> {
    if difference.abs() > AMOUNT_TOLERANCE
        && reason.map(str::trim).filter(|s| !s.is_empty()).is_none()
    {
        return Err(AppError::InvalidParam(format!(
            "{context}差异 {:.2} 不为 0，必须填写差异原因",
            difference
        )));
    }
    Ok(())
}

/// 构造一条凭证分录（金额方向为正数；资金行带 fund_account_id、对方行必须为空）
fn gl_line(
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

// ==================== 新建（draft） ====================

/// 新建盘点单（draft）：账面余额按创建时点自动快照（voucher_lines 派生，与日记账同源），
/// 差异 = 实存 − 账面自动算；差异≠0 原因必填；面额明细可选，提供时合计必须等于实存金额。
pub fn create_count_sheet(
    conn: &mut Connection,
    input: &CashCountCreateInput,
    operator: &str,
) -> AppResult<CashCountSheet> {
    let operator = resolve_operator(conn, operator)?;
    let count_date = validate_date(&input.count_date, "盘点日期")?;
    let belong_month = month_of(count_date);
    if input.counted_amount < 0.0 {
        return Err(AppError::InvalidParam(
            "实存金额不能为负（无现金请填 0）".into(),
        ));
    }
    let denominations = normalize_denominations(input.denominations.as_deref().unwrap_or(&[]))?;
    validate_denomination_total(input.counted_amount, &denominations)?;
    require_cash_account(conn, input.fund_account_id)?;

    let tx = conn.unchecked_transaction()?;
    ensure_month_open(&tx, &belong_month)?;
    let book_balance = fund_account_balance(&tx, input.fund_account_id)?;
    let difference = input.counted_amount - book_balance;
    require_reason_if_difference(difference, input.difference_reason.as_deref(), "新建盘点单")?;

    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO cash_count_sheets
            (count_date, belong_month, fund_account_id, book_balance, counted_amount,
             difference, difference_reason, status, remark, created_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            input.count_date.trim(),
            belong_month,
            input.fund_account_id,
            book_balance,
            input.counted_amount,
            difference,
            input
                .difference_reason
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            CASH_COUNT_STATUS_DRAFT,
            input
                .remark
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            operator,
            now,
        ],
    )?;
    let id = tx.last_insert_rowid();
    replace_denominations(&tx, id, &denominations)?;
    tx.commit()?;
    get_sheet(conn, id)
}

// ==================== 修改（draft） ====================

/// 修改盘点单（仅 draft）：字段整体替换、面额明细整表替换；账面余额按保存时点重快照、
/// 差异重算；原所属月与新所属月均须未月结（已确认/已作废盘点单一律拒绝修改）。
pub fn update_count_sheet(
    conn: &mut Connection,
    id: i64,
    input: &CashCountUpdateInput,
    operator: &str,
) -> AppResult<CashCountSheet> {
    resolve_operator(conn, operator)?;
    let count_date = validate_date(&input.count_date, "盘点日期")?;
    let belong_month = month_of(count_date);
    if input.counted_amount < 0.0 {
        return Err(AppError::InvalidParam(
            "实存金额不能为负（无现金请填 0）".into(),
        ));
    }
    let denominations = normalize_denominations(input.denominations.as_deref().unwrap_or(&[]))?;
    validate_denomination_total(input.counted_amount, &denominations)?;
    require_cash_account(conn, input.fund_account_id)?;

    let tx = conn.unchecked_transaction()?;
    let sheet = get_sheet(&tx, id)?;
    if sheet.status != CASH_COUNT_STATUS_DRAFT {
        return Err(AppError::General(format!(
            "盘点单当前状态「{}」，不允许修改（仅草稿可修改）",
            status_label(&sheet.status)
        )));
    }
    // 月结双查：原所属月（改期逃月禁令）与新所属月均须未月结
    ensure_month_open(&tx, &sheet.belong_month)?;
    ensure_month_open(&tx, &belong_month)?;
    let book_balance = fund_account_balance(&tx, input.fund_account_id)?;
    let difference = input.counted_amount - book_balance;
    require_reason_if_difference(difference, input.difference_reason.as_deref(), "修改盘点单")?;

    let now = Utc::now().to_rfc3339();
    tx.execute(
        "UPDATE cash_count_sheets SET count_date = ?2, belong_month = ?3, fund_account_id = ?4,
            book_balance = ?5, counted_amount = ?6, difference = ?7, difference_reason = ?8,
            remark = ?9, updated_at = ?10
         WHERE id = ?1",
        params![
            id,
            input.count_date.trim(),
            belong_month,
            input.fund_account_id,
            book_balance,
            input.counted_amount,
            difference,
            input
                .difference_reason
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            input
                .remark
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            now,
        ],
    )?;
    replace_denominations(&tx, id, &denominations)?;
    tx.commit()?;
    get_sheet(conn, id)
}

// ==================== 确认（生成差异凭证） ====================

/// 确认盘点单（draft → confirmed）：账面余额重快照为确认时点账户余额（spec 6），
/// 差异重算后：盘亏 借 1901 / 贷账户挂接科目（带 fund_account_id）；盘盈 借账户挂接科目 / 贷 1901；
/// 差异 0 免凭证仅留盘点记录。凭证日期 = 盘点日期、归属月 = 盘点月（须未月结），
/// 凭证与状态更新同事务落库。
pub fn confirm_count_sheet(
    conn: &mut Connection,
    id: i64,
    operator: &str,
) -> AppResult<CashCountSheet> {
    resolve_operator(conn, operator)?;
    let tx = conn.unchecked_transaction()?;
    let sheet = get_sheet(&tx, id)?;
    if sheet.status != CASH_COUNT_STATUS_DRAFT {
        return Err(AppError::General(format!(
            "盘点单当前状态「{}」，不允许确认（仅草稿可确认）",
            status_label(&sheet.status)
        )));
    }
    ensure_month_open(&tx, &sheet.belong_month)?;
    // 确认前复核：面额明细（若有）合计必须等于实存金额
    let denominations = load_denominations(&tx, id)?;
    let denom_total: f64 = denominations.iter().map(|d| d.subtotal).sum();
    if !denominations.is_empty() && (denom_total - sheet.counted_amount).abs() > AMOUNT_TOLERANCE {
        return Err(AppError::InvalidParam(format!(
            "面额明细合计 {denom_total:.2} 与实存金额 {:.2} 不一致，不能确认",
            sheet.counted_amount
        )));
    }

    // 确认时点账面余额快照 + 差异重算（草稿保存后账面可能已变动）
    let book_balance = fund_account_balance(&tx, sheet.fund_account_id)?;
    let difference = sheet.counted_amount - book_balance;
    require_reason_if_difference(difference, sheet.difference_reason.as_deref(), "确认盘点单")?;

    let account_name = require_cash_account(&tx, sheet.fund_account_id)?;
    let gl_cash = fund_account_gl_code(&tx, sheet.fund_account_id)?;
    let voucher_id = if difference.abs() > AMOUNT_TOLERANCE {
        let amount = difference.abs();
        let summary = format!("现金盘点盈亏（{account_name}）");
        let lines = if difference < 0.0 {
            // 盘亏：借 1901 待处理财产损溢 / 贷现金账户挂接科目
            vec![
                gl_line(GL_PENDING_ASSETS.into(), amount, 0.0, None, &summary),
                gl_line(
                    gl_cash.clone(),
                    0.0,
                    amount,
                    Some(sheet.fund_account_id),
                    &summary,
                ),
            ]
        } else {
            // 盘盈：借现金账户挂接科目 / 贷 1901 待处理财产损溢
            vec![
                gl_line(
                    gl_cash.clone(),
                    amount,
                    0.0,
                    Some(sheet.fund_account_id),
                    &summary,
                ),
                gl_line(GL_PENDING_ASSETS.into(), 0.0, amount, None, &summary),
            ]
        };
        ensure_fund_voucher_lines(&lines)?;
        let voucher = accounting::insert_voucher(
            &tx,
            &VoucherDraft {
                belong_month: sheet.belong_month.clone(),
                voucher_date: sheet.count_date.clone(),
                source_type: VOUCHER_SOURCE_CASH_COUNT.into(),
                source_id: sheet.id,
                remark: Some(format!(
                    "{summary}：账面 {:.2} / 实存 {:.2} / 差异 {difference:.2}",
                    book_balance, sheet.counted_amount
                )),
                lines,
            },
        )?;
        Some(voucher.id)
    } else {
        None
    };

    let now = Utc::now().to_rfc3339();
    tx.execute(
        "UPDATE cash_count_sheets SET book_balance = ?2, difference = ?3, status = ?4,
            voucher_id = ?5, updated_at = ?6
         WHERE id = ?1",
        params![
            id,
            book_balance,
            difference,
            CASH_COUNT_STATUS_CONFIRMED,
            voucher_id,
            now,
        ],
    )?;
    tx.commit()?;
    get_sheet(conn, id)
}

// ==================== 作废（draft 直接作废 / confirmed 走红字冲正） ====================

/// 作废盘点单：
/// - draft：直接作废（无凭证无资金影响）；
/// - confirmed 且差异凭证存在：红字冲正——原差异凭证保留 active，生成借贷互换的反向凭证
///   （source_id = −盘点单 id），组合净影响 0；冲正原因必填；
/// - confirmed 且差异 0（无凭证）：直接作废。
/// 作废月份（盘点月）须未月结。
pub fn void_count_sheet(
    conn: &mut Connection,
    id: i64,
    reason: Option<&str>,
    operator: &str,
) -> AppResult<CashCountSheet> {
    resolve_operator(conn, operator)?;
    let tx = conn.unchecked_transaction()?;
    let sheet = get_sheet(&tx, id)?;
    if sheet.status == CASH_COUNT_STATUS_VOID {
        return Err(AppError::General("盘点单已作废，不能重复作废".into()));
    }
    ensure_month_open(&tx, &sheet.belong_month)?;
    let now = Utc::now().to_rfc3339();

    if sheet.status == CASH_COUNT_STATUS_CONFIRMED {
        // 有差异凭证的确认单：红字冲正（原因必填），原凭证保留 active
        if let Some(voucher_id) = sheet.voucher_id {
            let trimmed = reason.map(str::trim).unwrap_or("");
            if trimmed.is_empty() {
                return Err(AppError::InvalidParam(
                    "已确认盘点单存在差异凭证，作废走红字冲正必须填写原因".into(),
                ));
            }
            let source_voucher = accounting::get_active_voucher_for_source(
                &tx,
                VOUCHER_SOURCE_CASH_COUNT,
                sheet.id,
            )?
            .ok_or_else(|| {
                AppError::General(format!(
                    "盘点单 #{} 的差异凭证不存在或已失效，无法冲正",
                    sheet.id
                ))
            })?;
            if source_voucher.id != voucher_id {
                return Err(AppError::General(format!(
                    "盘点单 #{} 的差异凭证与登记不符（{} vs {voucher_id}），无法冲正",
                    sheet.id, source_voucher.id
                )));
            }
            // 红字：复制原凭证分录并交换借贷方向（资金行 fund_account_id 随科目保留）
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
                &tx,
                &VoucherDraft {
                    belong_month: sheet.belong_month.clone(),
                    voucher_date: sheet.count_date.clone(),
                    // 冲正凭证专用负 id 锚点：−盘点单 id（见模块头注释）
                    source_id: -sheet.id,
                    source_type: VOUCHER_SOURCE_CASH_COUNT.into(),
                    remark: Some(format!("冲正现金盘点单 #{}：{}", sheet.id, trimmed)),
                    lines,
                },
            )?;
        }
    }

    tx.execute(
        "UPDATE cash_count_sheets SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, CASH_COUNT_STATUS_VOID, now],
    )?;
    tx.commit()?;
    get_sheet(conn, id)
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::setup_financial_db;

    const OPERATOR: &str = "张出纳";

    /// 盘点测试环境：财务库 + 操作人 + 现金账户(1001) + 银行账户(1002)
    struct CountEnv {
        conn: Connection,
        cash_account_id: i64,
        bank_account_id: i64,
    }

    fn count_env() -> CountEnv {
        let conn = setup_financial_db();
        let now = "2026-09-01T00:00:00+00:00";
        conn.execute(
            "INSERT INTO operator_profiles (name, role, is_active, created_at, updated_at)
             VALUES (?1, 'cashier', 1, ?2, ?2)",
            params![OPERATOR, now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO fund_accounts (account_code, name, account_type, gl_account_code)
             VALUES ('CASH-001', '现金库', 'cash', '1001')",
            [],
        )
        .unwrap();
        let cash_account_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO fund_accounts (account_code, name, account_type, gl_account_code)
             VALUES ('BANK-001', '基本户', 'bank', '1002')",
            [],
        )
        .unwrap();
        let bank_account_id = conn.last_insert_rowid();
        CountEnv {
            conn,
            cash_account_id,
            bank_account_id,
        }
    }

    /// 为现金账户注入账面余额：手工凭证 借 1001（带 fund_account_id）/ 贷 6602 管理费用。
    /// source_id 取存量凭证数 +1，保证多次预置不撞 (bank_manual, source_id) active 唯一索引。
    fn seed_cash_balance(conn: &Connection, account_id: i64, amount: f64) {
        let seq: i64 = conn
            .query_row("SELECT COUNT(*) + 1 FROM vouchers", [], |r| r.get(0))
            .unwrap();
        accounting::insert_voucher(
            conn,
            &VoucherDraft {
                belong_month: "2026-08".into(),
                voucher_date: "2026-08-31".into(),
                source_type: "bank_manual".into(),
                source_id: seq,
                remark: Some("测试预置余额".into()),
                lines: vec![
                    gl_line("1001".into(), amount, 0.0, Some(account_id), "预置余额"),
                    gl_line("6602".into(), 0.0, amount, None, "预置余额"),
                ],
            },
        )
        .unwrap();
    }

    /// 将某月置为已正式月结
    fn close_month(conn: &Connection, month: &str) {
        conn.execute(
            "INSERT INTO month_closes (month, status, created_at, updated_at)
             VALUES (?1, 'closed', '2026-09-30', '2026-09-30')",
            params![month],
        )
        .unwrap();
    }

    fn create_input(account_id: i64, counted: f64) -> CashCountCreateInput {
        CashCountCreateInput {
            count_date: "2026-09-15".into(),
            fund_account_id: account_id,
            counted_amount: counted,
            difference_reason: None,
            remark: None,
            denominations: None,
        }
    }

    fn denom(denomination: f64, quantity: i64) -> CashCountDenominationInput {
        CashCountDenominationInput {
            denomination,
            quantity,
        }
    }

    /// 读某凭证全部分录：(科目, 借, 贷, fund_account_id)，按 line_order
    fn voucher_lines(conn: &Connection, voucher_id: i64) -> Vec<(String, f64, f64, Option<i64>)> {
        let mut stmt = conn
            .prepare(
                "SELECT account_code, debit_amount, credit_amount, fund_account_id
                 FROM voucher_lines WHERE voucher_id = ?1 ORDER BY line_order",
            )
            .unwrap();
        stmt.query_map(params![voucher_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    }

    /// 断言凭证前两行分录与预期完全一致：(科目, 借, 贷, fund_account_id)
    fn assert_two_lines(
        conn: &Connection,
        voucher_id: i64,
        expect: [(&str, f64, f64, Option<i64>); 2],
    ) {
        let lines = voucher_lines(conn, voucher_id);
        assert_eq!(
            lines.len(),
            2,
            "凭证 {voucher_id} 应恰有两行分录：{lines:?}"
        );
        for (actual, expected) in lines.iter().zip(expect.iter()) {
            assert_eq!(actual.0, expected.0, "凭证 {voucher_id} 科目不符");
            assert!(
                (actual.1 - expected.1).abs() < AMOUNT_TOLERANCE,
                "凭证 {voucher_id} 借方不符：{actual:?} vs {expected:?}"
            );
            assert!(
                (actual.2 - expected.2).abs() < AMOUNT_TOLERANCE,
                "凭证 {voucher_id} 贷方不符：{actual:?} vs {expected:?}"
            );
            assert_eq!(
                actual.3, expected.3,
                "凭证 {voucher_id} fund_account_id 不符"
            );
        }
    }

    /// 断言一组凭证净影响为 0：各科目借贷轧差为 0 且总额平衡
    fn assert_net_zero(conn: &Connection, voucher_ids: &[i64]) {
        let mut nets: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
        let mut total = 0.0;
        for id in voucher_ids {
            for (code, debit, credit, _) in voucher_lines(conn, *id) {
                *nets.entry(code).or_default() += debit - credit;
                total += debit - credit;
            }
        }
        for (code, net) in &nets {
            assert!(
                net.abs() < AMOUNT_TOLERANCE,
                "科目 {code} 净影响 {net} 不为 0"
            );
        }
        assert!(
            total.abs() < AMOUNT_TOLERANCE,
            "凭证组合总净影响 {total} 不为 0"
        );
    }

    /// 盘点单关联的全部 active 凭证 id（差异凭证正 id + 冲正凭证负 id）
    fn sheet_active_vouchers(conn: &Connection, sheet_id: i64) -> Vec<i64> {
        let mut stmt = conn
            .prepare(
                "SELECT id FROM vouchers WHERE status = 'active' AND source_type = 'cash_count'
                 AND source_id IN (?1, ?2)",
            )
            .unwrap();
        stmt.query_map(params![sheet_id, -sheet_id], |r| r.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    // ---------- 新建：快照 / 差异 / 门禁 ----------

    #[test]
    fn test_create_snapshot_and_difference_reason_required() {
        let mut env = count_env();
        seed_cash_balance(&env.conn, env.cash_account_id, 500.0);
        // 差异≠0 原因必填
        let err = create_count_sheet(
            &mut env.conn,
            &create_input(env.cash_account_id, 450.0),
            OPERATOR,
        )
        .unwrap_err();
        assert!(err.to_string().contains("差异原因"), "{err}");
        // 带原因创建成功：账面快照 500、差异 -50、draft、操作人署名
        let mut input = create_input(env.cash_account_id, 450.0);
        input.difference_reason = Some("找零支出未记账".into());
        let sheet = create_count_sheet(&mut env.conn, &input, OPERATOR).unwrap();
        assert_eq!(sheet.status, "draft");
        assert!((sheet.book_balance - 500.0).abs() < AMOUNT_TOLERANCE);
        assert!((sheet.difference + 50.0).abs() < AMOUNT_TOLERANCE);
        assert_eq!(sheet.created_by.as_deref(), Some(OPERATOR));
        assert_eq!(sheet.voucher_id, None, "草稿不生成凭证");
        // 实存金额不能为负
        let err = create_count_sheet(
            &mut env.conn,
            &create_input(env.cash_account_id, -1.0),
            OPERATOR,
        )
        .unwrap_err();
        assert!(err.to_string().contains("不能为负"), "{err}");
        // 未知操作人
        let err = create_count_sheet(
            &mut env.conn,
            &create_input(env.cash_account_id, 500.0),
            "路人甲",
        )
        .unwrap_err();
        assert!(err.to_string().contains("操作人"), "{err}");
    }

    #[test]
    fn test_denomination_total_validation_and_persistence() {
        let mut env = count_env();
        // 合计 188 ≠ 实存 200 → 拒绝
        let mut input = create_input(env.cash_account_id, 200.0);
        input.denominations = Some(vec![
            denom(100.0, 1),
            denom(50.0, 1),
            denom(20.0, 1),
            denom(10.0, 1),
            denom(5.0, 1),
            denom(1.0, 3),
        ]);
        let err = create_count_sheet(&mut env.conn, &input, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("不一致"), "{err}");
        // 合计 200 = 实存 200 → 成功（账面 0，差异 +200，须带原因）
        input.difference_reason = Some("账实差 200 待查".into());
        input.denominations = Some(vec![
            denom(100.0, 1),
            denom(50.0, 1),
            denom(20.0, 1),
            denom(10.0, 2),
            denom(5.0, 1),
            denom(1.0, 3),
            denom(0.5, 2),
            denom(0.1, 10),
        ]);
        let sheet = create_count_sheet(&mut env.conn, &input, OPERATOR).unwrap();
        let detail = get_count_sheet_detail(&env.conn, sheet.id).unwrap();
        let total: f64 = detail.denominations.iter().map(|d| d.subtotal).sum();
        assert!(
            (total - 200.0).abs() < AMOUNT_TOLERANCE,
            "面额合计应 200：{total}"
        );
        assert_eq!(detail.denominations.len(), 8, "八档面额全部入库");
        assert!(
            (detail.sheet.difference - 200.0).abs() < AMOUNT_TOLERANCE,
            "账面 0 实存 200，差异自动算为 200：{}",
            detail.sheet.difference
        );
        // 同面额重复 → 拒绝
        let mut dup = create_input(env.cash_account_id, 150.0);
        dup.difference_reason = Some("差异".into());
        dup.denominations = Some(vec![denom(100.0, 1), denom(100.0, 0)]);
        let err = create_count_sheet(&mut env.conn, &dup, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("重复"), "{err}");
        // 面额 <= 0 → 拒绝
        let mut bad = create_input(env.cash_account_id, 0.0);
        bad.denominations = Some(vec![denom(0.0, 1)]);
        let err = create_count_sheet(&mut env.conn, &bad, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("大于 0"), "{err}");
    }

    #[test]
    fn test_non_cash_account_rejected() {
        let mut env = count_env();
        let err = create_count_sheet(
            &mut env.conn,
            &create_input(env.bank_account_id, 100.0),
            OPERATOR,
        )
        .unwrap_err();
        assert!(err.to_string().contains("现金类账户"), "{err}");
    }

    // ---------- 修改（draft） ----------

    #[test]
    fn test_update_draft_replaces_denominations_and_recomputes() {
        let mut env = count_env();
        let mut input = create_input(env.cash_account_id, 100.0);
        input.difference_reason = Some("初次试录".into());
        let sheet = create_count_sheet(&mut env.conn, &input, OPERATOR).unwrap();
        // 改实存 + 换面额明细
        let update = CashCountUpdateInput {
            count_date: "2026-09-16".into(),
            fund_account_id: env.cash_account_id,
            counted_amount: 188.0,
            difference_reason: Some("修正实存".into()),
            remark: Some("改过".into()),
            denominations: Some(vec![
                denom(100.0, 1),
                denom(50.0, 1),
                denom(20.0, 1),
                denom(10.0, 1),
                denom(5.0, 1),
                denom(1.0, 3),
            ]),
        };
        let updated = update_count_sheet(&mut env.conn, sheet.id, &update, OPERATOR).unwrap();
        assert_eq!(updated.count_date, "2026-09-16");
        assert!((updated.counted_amount - 188.0).abs() < AMOUNT_TOLERANCE);
        assert!(
            (updated.difference - 188.0).abs() < AMOUNT_TOLERANCE,
            "账面 0 差异重算"
        );
        let detail = get_count_sheet_detail(&env.conn, sheet.id).unwrap();
        assert_eq!(detail.denominations.len(), 6, "面额明细整表替换");
        // 面额合计不一致 → 拒绝
        let bad = CashCountUpdateInput {
            counted_amount: 200.0,
            denominations: Some(vec![denom(100.0, 1)]),
            ..update.clone()
        };
        let err = update_count_sheet(&mut env.conn, sheet.id, &bad, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("不一致"), "{err}");
        // 差异≠0 原因必填
        let no_reason = CashCountUpdateInput {
            counted_amount: 188.0,
            denominations: None,
            difference_reason: None,
            ..update.clone()
        };
        let err = update_count_sheet(&mut env.conn, sheet.id, &no_reason, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("差异原因"), "{err}");
    }

    // ---------- 确认：差异双向凭证 / 差异 0 免凭证 ----------

    #[test]
    fn test_confirm_shortage_generates_pending_assets_debit_voucher() {
        let mut env = count_env();
        seed_cash_balance(&env.conn, env.cash_account_id, 500.0);
        let mut input = create_input(env.cash_account_id, 450.0);
        input.difference_reason = Some("盘亏 50，待查明".into());
        let sheet = create_count_sheet(&mut env.conn, &input, OPERATOR).unwrap();
        let confirmed = confirm_count_sheet(&mut env.conn, sheet.id, OPERATOR).unwrap();
        assert_eq!(confirmed.status, "confirmed");
        let voucher_id = confirmed.voucher_id.expect("盘亏必须生成凭证");
        // 盘亏：借 1901 50 / 贷 1001 50（带 fund_account_id）
        assert_two_lines(
            &env.conn,
            voucher_id,
            [
                ("1901", 50.0, 0.0, None),
                ("1001", 0.0, 50.0, Some(env.cash_account_id)),
            ],
        );
        let voucher: (String, String, i64, String) = env
            .conn
            .query_row(
                "SELECT voucher_date, belong_month, source_id, source_type FROM vouchers WHERE id = ?1",
                params![voucher_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(voucher.0, "2026-09-15", "凭证日期=盘点日期");
        assert_eq!(voucher.1, "2026-09", "归属月=盘点月");
        assert_eq!(voucher.2, sheet.id);
        assert_eq!(voucher.3, "cash_count");
    }

    #[test]
    fn test_confirm_surplus_generates_pending_assets_credit_voucher() {
        let mut env = count_env();
        seed_cash_balance(&env.conn, env.cash_account_id, 300.0);
        let mut input = create_input(env.cash_account_id, 380.0);
        input.difference_reason = Some("盘盈 80，待查明".into());
        let sheet = create_count_sheet(&mut env.conn, &input, OPERATOR).unwrap();
        let confirmed = confirm_count_sheet(&mut env.conn, sheet.id, OPERATOR).unwrap();
        let voucher_id = confirmed.voucher_id.expect("盘盈必须生成凭证");
        // 盘盈：借 1001 80（带 fund_account_id）/ 贷 1901 80
        assert_two_lines(
            &env.conn,
            voucher_id,
            [
                ("1001", 80.0, 0.0, Some(env.cash_account_id)),
                ("1901", 0.0, 80.0, None),
            ],
        );
    }

    #[test]
    fn test_confirm_resnapshots_book_balance_at_confirm_time() {
        let mut env = count_env();
        seed_cash_balance(&env.conn, env.cash_account_id, 500.0);
        let mut input = create_input(env.cash_account_id, 450.0);
        input.difference_reason = Some("盘亏待查".into());
        let sheet = create_count_sheet(&mut env.conn, &input, OPERATOR).unwrap();
        assert!((sheet.book_balance - 500.0).abs() < AMOUNT_TOLERANCE);
        // 草稿保存后账面又增加 100：确认时点重快照 → 差异按确认时点算为 -150
        seed_cash_balance(&env.conn, env.cash_account_id, 100.0);
        let confirmed = confirm_count_sheet(&mut env.conn, sheet.id, OPERATOR).unwrap();
        assert_eq!(confirmed.status, "confirmed");
        assert!(
            (confirmed.book_balance - 600.0).abs() < AMOUNT_TOLERANCE,
            "账面余额取确认时点快照：{}",
            confirmed.book_balance
        );
        assert!((confirmed.difference + 150.0).abs() < AMOUNT_TOLERANCE);
        let voucher_id = confirmed.voucher_id.expect("差异≠0 必须生成凭证");
        assert_two_lines(
            &env.conn,
            voucher_id,
            [
                ("1901", 150.0, 0.0, None),
                ("1001", 0.0, 150.0, Some(env.cash_account_id)),
            ],
        );
    }

    #[test]
    fn test_confirm_zero_difference_skips_voucher() {
        let mut env = count_env();
        seed_cash_balance(&env.conn, env.cash_account_id, 400.0);
        let sheet = create_count_sheet(
            &mut env.conn,
            &create_input(env.cash_account_id, 400.0),
            OPERATOR,
        )
        .unwrap();
        let confirmed = confirm_count_sheet(&mut env.conn, sheet.id, OPERATOR).unwrap();
        assert_eq!(confirmed.status, "confirmed");
        assert_eq!(confirmed.voucher_id, None, "差异 0 免凭证");
        assert!(sheet_active_vouchers(&env.conn, sheet.id).is_empty());
        // 面额模式差异 0 同样免凭证
        let mut input = create_input(env.cash_account_id, 400.0);
        input.denominations = Some(vec![denom(100.0, 4)]);
        let sheet2 = create_count_sheet(&mut env.conn, &input, OPERATOR).unwrap();
        let confirmed2 = confirm_count_sheet(&mut env.conn, sheet2.id, OPERATOR).unwrap();
        assert_eq!(confirmed2.voucher_id, None);
        assert!(sheet_active_vouchers(&env.conn, sheet2.id).is_empty());
    }

    #[test]
    fn test_confirm_requires_reason_when_difference() {
        let mut env = count_env();
        seed_cash_balance(&env.conn, env.cash_account_id, 500.0);
        // 账面在草稿保存后变动，使确认时点差异≠0 且单上无原因 → 确认被拒
        let sheet = create_count_sheet(
            &mut env.conn,
            &create_input(env.cash_account_id, 500.0),
            OPERATOR,
        )
        .unwrap();
        seed_cash_balance(&env.conn, env.cash_account_id, 60.0);
        let err = confirm_count_sheet(&mut env.conn, sheet.id, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("差异原因"), "{err}");
        assert_eq!(
            get_sheet(&env.conn, sheet.id).unwrap().status,
            "draft",
            "确认失败不改状态"
        );
    }

    // ---------- 确认后修改拒绝 / 重复确认拒绝 ----------

    #[test]
    fn test_confirmed_sheet_cannot_update_or_reconfirm() {
        let mut env = count_env();
        let mut input = create_input(env.cash_account_id, 100.0);
        input.difference_reason = Some("盘盈 100".into());
        let sheet = create_count_sheet(&mut env.conn, &input, OPERATOR).unwrap();
        confirm_count_sheet(&mut env.conn, sheet.id, OPERATOR).unwrap();
        let update = CashCountUpdateInput {
            count_date: "2026-09-16".into(),
            fund_account_id: env.cash_account_id,
            counted_amount: 200.0,
            difference_reason: None,
            remark: None,
            denominations: None,
        };
        let err = update_count_sheet(&mut env.conn, sheet.id, &update, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("不允许修改"), "{err}");
        let err = confirm_count_sheet(&mut env.conn, sheet.id, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("不允许确认"), "{err}");
    }

    // ---------- 作废：draft 直接作废 / confirmed 红字冲正 ----------

    #[test]
    fn test_void_draft_directly() {
        let mut env = count_env();
        let mut input = create_input(env.cash_account_id, 100.0);
        input.difference_reason = Some("盘盈 100".into());
        let sheet = create_count_sheet(&mut env.conn, &input, OPERATOR).unwrap();
        let voided = void_count_sheet(&mut env.conn, sheet.id, Some("录错重盘"), OPERATOR).unwrap();
        assert_eq!(voided.status, "void");
        assert!(
            sheet_active_vouchers(&env.conn, sheet.id).is_empty(),
            "草稿作废无凭证"
        );
        // 重复作废拒绝
        let err = void_count_sheet(&mut env.conn, sheet.id, None, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("不能重复作废"), "{err}");
    }

    #[test]
    fn test_void_confirmed_red_letter_reversal_net_zero() {
        let mut env = count_env();
        seed_cash_balance(&env.conn, env.cash_account_id, 500.0);
        let mut input = create_input(env.cash_account_id, 450.0);
        input.difference_reason = Some("盘亏 50".into());
        let sheet = create_count_sheet(&mut env.conn, &input, OPERATOR).unwrap();
        let confirmed = confirm_count_sheet(&mut env.conn, sheet.id, OPERATOR).unwrap();
        let diff_voucher = confirmed.voucher_id.unwrap();
        // 冲正原因必填
        let err = void_count_sheet(&mut env.conn, sheet.id, Some("  "), OPERATOR).unwrap_err();
        assert!(err.to_string().contains("必须填写原因"), "{err}");
        // 带原因作废：原凭证保留 active + 反向凭证并存，净影响 0
        let voided = void_count_sheet(
            &mut env.conn,
            sheet.id,
            Some("盘点单录错，红字冲正"),
            OPERATOR,
        )
        .unwrap();
        assert_eq!(voided.status, "void");
        let status: String = env
            .conn
            .query_row(
                "SELECT status FROM vouchers WHERE id = ?1",
                params![diff_voucher],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "active", "原差异凭证保留 active（红字口径）");
        let all = sheet_active_vouchers(&env.conn, sheet.id);
        assert_eq!(all.len(), 2, "差异凭证 + 冲正凭证两张 active 凭证");
        assert_net_zero(&env.conn, &all);
        // 冲正凭证借贷互换（行序随原凭证保留）：贷 1901 / 借 1001
        let reversal_id = all.iter().copied().find(|id| *id != diff_voucher).unwrap();
        assert_two_lines(
            &env.conn,
            reversal_id,
            [
                ("1901", 0.0, 50.0, None),
                ("1001", 50.0, 0.0, Some(env.cash_account_id)),
            ],
        );
        // 差异 0 的确认单（无凭证）作废：直接作废、不产生凭证。
        // 此时差异凭证（贷 50）与冲正凭证（借 50）互抵，现金账面仍为 500
        let sheet2 = create_count_sheet(
            &mut env.conn,
            &create_input(env.cash_account_id, 500.0),
            OPERATOR,
        )
        .unwrap();
        let confirmed2 = confirm_count_sheet(&mut env.conn, sheet2.id, OPERATOR).unwrap();
        assert_eq!(confirmed2.voucher_id, None);
        let voided2 = void_count_sheet(&mut env.conn, sheet2.id, None, OPERATOR).unwrap();
        assert_eq!(voided2.status, "void");
        assert!(sheet_active_vouchers(&env.conn, sheet2.id).is_empty());
    }

    // ---------- 月结保护 ----------

    #[test]
    fn test_month_close_protection() {
        let mut env = count_env();
        seed_cash_balance(&env.conn, env.cash_account_id, 500.0);
        // 新建：盘点月已月结 → 拒绝
        close_month(&env.conn, "2026-09");
        let err = create_count_sheet(
            &mut env.conn,
            &create_input(env.cash_account_id, 500.0),
            OPERATOR,
        )
        .unwrap_err();
        assert!(err.to_string().contains("已正式月结"), "{err}");
        close_month_reopen(&env.conn, "2026-09");
        // 确认：盘点月已月结 → 拒绝
        let sheet = create_count_sheet(
            &mut env.conn,
            &create_input(env.cash_account_id, 500.0),
            OPERATOR,
        )
        .unwrap();
        close_month(&env.conn, "2026-09");
        let err = confirm_count_sheet(&mut env.conn, sheet.id, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("已正式月结"), "{err}");
        // 作废：盘点月已月结 → 拒绝
        let err = void_count_sheet(&mut env.conn, sheet.id, None, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("已正式月结"), "{err}");
        // 修改：原所属月已月结 → 拒绝（改期逃月禁令）
        let update = CashCountUpdateInput {
            count_date: "2026-10-10".into(),
            fund_account_id: env.cash_account_id,
            counted_amount: 500.0,
            difference_reason: None,
            remark: None,
            denominations: None,
        };
        let err = update_count_sheet(&mut env.conn, sheet.id, &update, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("已正式月结"), "{err}");
        close_month_reopen(&env.conn, "2026-09");
        // 跨月改期允许（原月 + 新月双开放）
        let moved = update_count_sheet(&mut env.conn, sheet.id, &update, OPERATOR).unwrap();
        assert_eq!(moved.belong_month, "2026-10");
    }

    /// 反月结（测试辅助）
    fn close_month_reopen(conn: &Connection, month: &str) {
        conn.execute("DELETE FROM month_closes WHERE month = ?1", params![month])
            .unwrap();
    }

    // ---------- 查询 ----------

    #[test]
    fn test_get_count_sheets_filters_and_detail() {
        let mut env = count_env();
        seed_cash_balance(&env.conn, env.cash_account_id, 100.0);
        let first = create_count_sheet(
            &mut env.conn,
            &create_input(env.cash_account_id, 100.0),
            OPERATOR,
        )
        .unwrap();
        let second = create_count_sheet(
            &mut env.conn,
            &create_input(env.cash_account_id, 100.0),
            OPERATOR,
        )
        .unwrap();
        void_count_sheet(&mut env.conn, second.id, Some("不要了"), OPERATOR).unwrap();

        // 全量 2 张，id 降序
        let all = get_count_sheets(&env.conn, &CashCountQuery::default()).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all[0].id > all[1].id, "列表应按 id 降序");
        // 状态过滤
        let drafts = get_count_sheets(
            &env.conn,
            &CashCountQuery {
                status: Some("draft".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].id, first.id);
        // 账户过滤
        let by_account = get_count_sheets(
            &env.conn,
            &CashCountQuery {
                fund_account_id: Some(env.cash_account_id),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_account.len(), 2);
        let by_other = get_count_sheets(
            &env.conn,
            &CashCountQuery {
                fund_account_id: Some(env.bank_account_id),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(by_other.is_empty());
        // 月份过滤
        let hits = get_count_sheets(
            &env.conn,
            &CashCountQuery {
                belong_month: Some("2026-09".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(hits.len(), 2);
        let miss = get_count_sheets(
            &env.conn,
            &CashCountQuery {
                belong_month: Some("2026-01".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(miss.is_empty());
        // 详情含面额明细与状态中文标签
        assert_eq!(status_label("draft"), "草稿");
        let err = get_count_sheet_detail(&env.conn, 9999).unwrap_err();
        assert!(err.to_string().contains("盘点单不存在"), "{err}");
    }
}
