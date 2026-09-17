// ==================== 第八阶段：票据状态机与凭证联动（Task 2，spec 4） ====================
// 模式照搬第七阶段 cashier.rs 资金单状态机：状态流转 + 凭证生成 + 审批事件同事务提交，
// 红字冲正保留原凭证 active（反向凭证并存、净影响 0），月结保护按登记月 + 操作月双查。
// 凭证归属：登记凭证 source_type='instrument'（source_id=票据 id，每票一张）；
// 流转/冲正凭证 source_type='instrument_flow'（source_id=审批事件 id，每次流转唯一，
// 天然满足 vouchers (source_type, source_id) active 部分唯一索引，冲正后同型流转可重做）。
//
// 本模块 pub API 由 Task 3 命令层（commands.rs）消费，接入前整段暂不可达，
// 以模块级 allow 放行 dead_code（同时作为 models.rs 已摘 allow 的票据类型的活性根）；
// Task 3 接线合入时必须移除本 allow。
#![allow(dead_code)]

use crate::accounting;
use crate::cashier::{ensure_fund_voucher_lines, fund_account_gl_code};
use crate::db::ensure_month_open;
use crate::errors::{AppError, AppResult};
use crate::models::{
    InstrumentEndorsement, InstrumentRegisterInput, NegotiableInstrument, VoucherDraft,
    VoucherLineDraft, INSTRUMENT_DIRECTIONS, INSTRUMENT_DIRECTION_ISSUED,
    INSTRUMENT_DIRECTION_RECEIVED, INSTRUMENT_STATUS_COLLECTED, INSTRUMENT_STATUS_COLLECTING,
    INSTRUMENT_STATUS_DISCOUNTED, INSTRUMENT_STATUS_ENDORSED_OUT, INSTRUMENT_STATUS_HOLDING,
    INSTRUMENT_STATUS_ISSUED_OUTSTANDING, INSTRUMENT_STATUS_PAID, INSTRUMENT_STATUS_VOID,
    INSTRUMENT_TYPES, INSTRUMENT_TYPE_CHECK,
};
use chrono::{NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};

/// 金额比较容差（与 cashier::AMOUNT_TOLERANCE 同值，彼处模块私有）
const AMOUNT_TOLERANCE: f64 = 0.005;

/// 票据凭证 source_type：登记凭证（source_id = 票据 id）
const VOUCHER_SOURCE_INSTRUMENT: &str = "instrument";
/// 票据流转/冲正凭证 source_type（source_id = 审批事件 id）
const VOUCHER_SOURCE_INSTRUMENT_FLOW: &str = "instrument_flow";

/// 审批事件实体类型（approval_events.entity_type 第八阶段扩展值）
const ENTITY_TYPE_INSTRUMENT: &str = "negotiable_instrument";

/// 科目 1121 应收票据（收到承兑借方；背书/贴现/到账确认贷方）
const GL_NOTE_RECEIVABLE: &str = "1121";
/// 科目 1122 应收账款（收到类登记缺省对方科目）
const GL_RECEIVABLE_DEFAULT: &str = "1122";
/// 科目 2201 应付票据（开出承兑登记贷方；开出兑付借方销账，spec 4.1 勘误后编码）
const GL_NOTE_PAYABLE: &str = "2201";
/// 科目 2202 应付账款（开出承兑登记/开出支票/背书的缺省对方科目，spec 4.1 勘误后编码）
const GL_PAYABLE_DEFAULT: &str = "2202";
/// 科目 6603 财务费用（贴现息，票面 − 实收）
const GL_FINANCE_EXPENSE: &str = "6603";

// ==================== 入参 / 出参模型 ====================

/// 背书转出入参（endorse_instrument）：本期约定全额背书，金额校验等于票面
#[derive(Debug, Clone, serde::Deserialize)]
pub struct InstrumentEndorseInput {
    pub instrument_id: i64,
    /// 被背书人
    pub endorsee: String,
    /// 背书日期（操作月 = 该日期所在月，凭证日期）
    pub endorse_date: String,
    /// 背书金额（必须等于票面，容差 0.005）
    pub amount: f64,
    /// 事由（付货款/转让等）
    pub purpose: Option<String>,
    /// 对方科目编码（缺省 2202 应付账款，可覆盖）
    pub counter_account_code: Option<String>,
}

/// 贴现入参（discount_instrument）：财务费用 = 票面 − 实收（差额 0 免财务费用腿）
#[derive(Debug, Clone, serde::Deserialize)]
pub struct InstrumentDiscountInput {
    pub instrument_id: i64,
    /// 贴现日期（操作月 = 该日期所在月，凭证日期）
    pub discount_date: String,
    /// 实收金额（不得大于票面）
    pub proceeds: f64,
    /// 入账资金账户（缺省取票据登记时的挂接账户）
    pub fund_account_id: Option<i64>,
}

/// 托收到账确认入参（confirm_collection）
#[derive(Debug, Clone, serde::Deserialize)]
pub struct InstrumentCollectConfirmInput {
    pub instrument_id: i64,
    /// 到账日期（操作月 = 该日期所在月，凭证日期）
    pub received_date: String,
    /// 入账资金账户（缺省取票据登记时的挂接账户）
    pub fund_account_id: Option<i64>,
}

/// 开出承兑兑付入参（settle_issued_instrument）
#[derive(Debug, Clone, serde::Deserialize)]
pub struct InstrumentSettleInput {
    pub instrument_id: i64,
    /// 兑付日期（操作月 = 该日期所在月，凭证日期）
    pub settle_date: String,
    /// 出账资金账户（缺省取票据登记时的挂接账户）
    pub fund_account_id: Option<i64>,
}

/// 红字冲正入参（reverse_instrument_flow）：已流转票据纠错，原因必填
#[derive(Debug, Clone, serde::Deserialize)]
pub struct InstrumentReverseInput {
    pub instrument_id: i64,
    /// 冲正日期（冲正凭证日期与归属月；已月结月份拒绝）
    pub reverse_date: String,
    /// 冲正原因（必填，approval_events 留痕）
    pub reason: String,
}

/// 背书结果：票据（已置 endorsed_out）+ 背书链记录（含背书凭证 id）
#[derive(Debug, Clone, serde::Serialize)]
pub struct InstrumentEndorseResult {
    pub instrument: NegotiableInstrument,
    pub endorsement: InstrumentEndorsement,
}

// ==================== 通用 helper ====================

/// 票据状态中文标签（报错展示用）
fn status_label(status: &str) -> &str {
    match status {
        INSTRUMENT_STATUS_HOLDING => "持有",
        INSTRUMENT_STATUS_ENDORSED_OUT => "已背书转出",
        INSTRUMENT_STATUS_DISCOUNTED => "已贴现",
        INSTRUMENT_STATUS_COLLECTING => "托收中",
        INSTRUMENT_STATUS_COLLECTED => "已到账",
        INSTRUMENT_STATUS_ISSUED_OUTSTANDING => "已开出未兑付",
        INSTRUMENT_STATUS_PAID => "已兑付",
        INSTRUMENT_STATUS_VOID => "已作废",
        other => other,
    }
}

/// 校验并解析操作人：`operator` 为操作人姓名（命令层经 require_current_operator 解析后传入），
/// 须为在册且启用的操作人，返回 (操作人 id, 姓名)。id 供 approval_events 署名（FK），
/// 姓名供 negotiable_instruments / instrument_endorsements.created_by 署名。
fn resolve_operator(conn: &Connection, operator: &str) -> AppResult<(i64, String)> {
    let name = operator.trim();
    if name.is_empty() {
        return Err(AppError::InvalidParam(
            "操作人必填：请先选择当前操作人".into(),
        ));
    }
    let id = conn
        .query_row(
            "SELECT id FROM operator_profiles WHERE name = ?1 AND is_active = 1",
            params![name],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::General(format!(
                "操作人「{name}」不存在或已停用，请先选择有效操作人"
            ))
        })?;
    Ok((id, name.to_string()))
}

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

/// 票据登记月（issue_date 所在月）：月结双查的"登记月"口径
fn register_month(inst: &NegotiableInstrument) -> AppResult<String> {
    let issue = validate_date(&inst.issue_date, "出票日")?;
    Ok(month_of(issue))
}

/// 缺省对方科目解析：显式编码去空格后非空则覆盖缺省
fn effective_counter(code: &Option<String>, default: &str) -> String {
    code.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
}

/// 必填资金账户解析（票据行挂接账户作为回退）
fn require_fund_account(
    explicit: Option<i64>,
    on_instrument: Option<i64>,
    message: &str,
) -> AppResult<i64> {
    explicit
        .or(on_instrument)
        .ok_or_else(|| AppError::General(message.to_string()))
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

/// 读取票据（19 列 → NegotiableInstrument），不存在报 NotFound
fn get_instrument(conn: &Connection, id: i64) -> AppResult<NegotiableInstrument> {
    conn.query_row(
        "SELECT id, instrument_type, direction, instrument_no, face_amount, issue_date, due_date,
                drawer, acceptor, payee, partner_id, fund_account_id, counter_account_code,
                status, voucher_id, remark, created_by, created_at, updated_at
         FROM negotiable_instruments WHERE id = ?1",
        params![id],
        |r| {
            Ok(NegotiableInstrument {
                id: r.get(0)?,
                instrument_type: r.get(1)?,
                direction: r.get(2)?,
                instrument_no: r.get(3)?,
                face_amount: r.get(4)?,
                issue_date: r.get(5)?,
                due_date: r.get(6)?,
                drawer: r.get(7)?,
                acceptor: r.get(8)?,
                payee: r.get(9)?,
                partner_id: r.get(10)?,
                fund_account_id: r.get(11)?,
                counter_account_code: r.get(12)?,
                status: r.get(13)?,
                voucher_id: r.get(14)?,
                remark: r.get(15)?,
                created_by: r.get(16)?,
                created_at: r.get(17)?,
                updated_at: r.get(18)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("票据不存在：id={id}")))
}

/// 读取背书记录
fn get_endorsement(conn: &Connection, id: i64) -> AppResult<InstrumentEndorsement> {
    conn.query_row(
        "SELECT id, instrument_id, endorse_order, endorsee, endorse_date, purpose, amount,
                voucher_id, created_by, created_at
         FROM instrument_endorsements WHERE id = ?1",
        params![id],
        |r| {
            Ok(InstrumentEndorsement {
                id: r.get(0)?,
                instrument_id: r.get(1)?,
                endorse_order: r.get(2)?,
                endorsee: r.get(3)?,
                endorse_date: r.get(4)?,
                purpose: r.get(5)?,
                amount: r.get(6)?,
                voucher_id: r.get(7)?,
                created_by: r.get(8)?,
                created_at: r.get(9)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("背书记录不存在：id={id}")))
}

/// 追加一条票据审批事件并返回事件 id（仅插入，无 UPDATE/DELETE 路径，spec 4.5 同口径）。
/// 事件 id 同时作为流转凭证的 source_id（vouchers 唯一性锚点）。必须与状态更新同事务调用。
#[allow(clippy::too_many_arguments)]
fn insert_instrument_event(
    conn: &Connection,
    entity_id: i64,
    action: &str,
    from_status: Option<&str>,
    to_status: Option<&str>,
    operator_id: i64,
    comment: Option<&str>,
) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO approval_events
            (entity_type, entity_id, action, from_status, to_status, operator_id, comment, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            ENTITY_TYPE_INSTRUMENT,
            entity_id,
            action,
            from_status,
            to_status,
            operator_id,
            comment,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

// ==================== 状态机事务骨架 ====================

/// 操作人上下文（事件署名 id + 业务行署名姓名）
struct OperatorContext {
    id: i64,
    name: String,
}

/// 票据流转命令公共骨架（照 cashier::transition_fund_document 模式）：
/// 操作人解析 → 事务内取票据 → 校验来源状态 → 月结双查（登记月 + 操作月）→
/// 追加审批事件（事件 id 即流转凭证 source_id）→ extra（凭证生成 / 从表写入，同事务）→
/// 状态更新 → 提交。任一步失败整体回滚，不留"已流转但无凭证"的半成品。
/// `extra` 返回值透传给调用方（背书命令返回背书链记录，其余为 ()）。
#[allow(clippy::too_many_arguments)]
fn transition_instrument<T, F>(
    conn: &mut Connection,
    instrument_id: i64,
    action: &str,
    action_label: &str,
    from_statuses: &[&str],
    to_status: &str,
    operate_date: &str,
    operator: &str,
    comment: Option<&str>,
    require_comment: bool,
    extra: F,
) -> AppResult<(NegotiableInstrument, T)>
where
    F: FnOnce(
        &Connection,
        &NegotiableInstrument,
        &OperatorContext,
        i64,
        &str,
        &str,
    ) -> AppResult<T>,
{
    let (operator_id, operator_name) = resolve_operator(conn, operator)?;
    let trimmed = comment.map(str::trim).unwrap_or("");
    if require_comment && trimmed.is_empty() {
        return Err(AppError::InvalidParam(format!(
            "{action_label}必须填写原因"
        )));
    }
    let op_date = validate_date(operate_date, &format!("{action_label}日期"))?;
    let op_month = month_of(op_date);

    let tx = conn.unchecked_transaction()?;
    let inst = get_instrument(&tx, instrument_id)?;
    if !from_statuses.contains(&inst.status.as_str()) {
        return Err(AppError::General(format!(
            "票据 {} 当前状态「{}」，不允许{action_label}（仅允许来源状态：{}）",
            inst.instrument_no,
            status_label(&inst.status),
            from_statuses
                .iter()
                .map(|s| status_label(s))
                .collect::<Vec<_>>()
                .join("、")
        )));
    }
    // 月结双查（spec 4 约束，与第七阶段冲正双月口径一致）：
    // 登记月（出票日所在月）与操作月（操作日期所在月）均须未月结，跨月流转允许
    ensure_month_open(&tx, &register_month(&inst)?)?;
    ensure_month_open(&tx, &op_month)?;

    let ctx = OperatorContext {
        id: operator_id,
        name: operator_name,
    };
    let now = Utc::now().to_rfc3339();
    let event_id = insert_instrument_event(
        &tx,
        instrument_id,
        action,
        Some(&inst.status),
        Some(to_status),
        operator_id,
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        },
    )?;
    let payload = extra(&tx, &inst, &ctx, event_id, operate_date.trim(), &now)?;
    tx.execute(
        "UPDATE negotiable_instruments SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![instrument_id, to_status, now],
    )?;
    tx.commit()?;
    let instrument = get_instrument(conn, instrument_id)?;
    Ok((instrument, payload))
}

// ==================== 登记 ====================

/// 登记票据（spec 4.1 四组合，登记即生成凭证，支票登记即终态）：
/// - 收到承兑 → holding：借 1121 应收票据 / 贷 counter（缺省 1122 应收账款）
/// - 开出承兑 → issued_outstanding：借 counter（缺省 2202 应付账款）/ 贷 2201 应付票据
/// - 收到支票 → collected：借入账账户挂接科目（带 fund_account_id）/ 贷 counter（缺省 1122）
/// - 开出支票 → paid：借 counter（缺省 2202）/ 贷出账账户挂接科目（带 fund_account_id）
/// 凭证日期 = 出票日、归属月 = 登记月（须未月结）；凭证与票据行同事务落库。
pub fn register_instrument(
    conn: &mut Connection,
    input: &InstrumentRegisterInput,
    operator: &str,
) -> AppResult<NegotiableInstrument> {
    let (_operator_id, operator_name) = resolve_operator(conn, operator)?;
    let no = input.instrument_no.trim();
    if no.is_empty() {
        return Err(AppError::InvalidParam("票据号码必填".into()));
    }
    if !INSTRUMENT_TYPES.contains(&input.instrument_type.as_str()) {
        return Err(AppError::InvalidParam(format!(
            "票据类型无效：{}（允许：{}）",
            input.instrument_type,
            INSTRUMENT_TYPES.join(" / ")
        )));
    }
    if !INSTRUMENT_DIRECTIONS.contains(&input.direction.as_str()) {
        return Err(AppError::InvalidParam(format!(
            "票据方向无效：{}（允许：{}）",
            input.direction,
            INSTRUMENT_DIRECTIONS.join(" / ")
        )));
    }
    let issue = validate_date(&input.issue_date, "出票日")?;
    let due = validate_date(&input.due_date, "到期日")?;
    if due < issue {
        return Err(AppError::InvalidParam("到期日不能早于出票日".into()));
    }
    if input.face_amount <= AMOUNT_TOLERANCE {
        return Err(AppError::InvalidParam("票面金额必须大于 0".into()));
    }
    let register_month = month_of(issue);

    let tx = conn.unchecked_transaction()?;
    // 登记月 = 操作月（登记即凭证发生月），月结后禁止补录
    ensure_month_open(&tx, &register_month)?;
    if let Some(partner_id) = input.partner_id {
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM business_partners WHERE id = ?1",
            params![partner_id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(AppError::InvalidParam(format!(
                "关联的往来单位不存在：id={partner_id}"
            )));
        }
    }
    // 同类型同号唯一（非 void）；中文预检避免裸 UNIQUE 报错
    let dup: i64 = tx.query_row(
        "SELECT COUNT(*) FROM negotiable_instruments
         WHERE instrument_type = ?1 AND instrument_no = ?2 AND status != 'void'",
        params![input.instrument_type, no],
        |r| r.get(0),
    )?;
    if dup > 0 {
        return Err(AppError::General(format!(
            "票据号 {no} 已存在同类型有效票据（作废后同号可重新登记）"
        )));
    }

    let summary = format!("票据登记 {no}");
    let is_check = input.instrument_type == INSTRUMENT_TYPE_CHECK;
    let (status, lines): (&'static str, Vec<VoucherLineDraft>) =
        match (input.direction.as_str(), is_check) {
            // 收到承兑：借 1121 / 贷对方（缺省 1122）
            (INSTRUMENT_DIRECTION_RECEIVED, false) => (
                INSTRUMENT_STATUS_HOLDING,
                vec![
                    gl_line(
                        GL_NOTE_RECEIVABLE.into(),
                        input.face_amount,
                        0.0,
                        None,
                        &summary,
                    ),
                    gl_line(
                        effective_counter(&input.counter_account_code, GL_RECEIVABLE_DEFAULT),
                        0.0,
                        input.face_amount,
                        None,
                        &summary,
                    ),
                ],
            ),
            // 开出承兑：借对方（缺省 2202 应付账款）/ 贷 2201 应付票据
            (INSTRUMENT_DIRECTION_ISSUED, false) => (
                INSTRUMENT_STATUS_ISSUED_OUTSTANDING,
                vec![
                    gl_line(
                        effective_counter(&input.counter_account_code, GL_PAYABLE_DEFAULT),
                        input.face_amount,
                        0.0,
                        None,
                        &summary,
                    ),
                    gl_line(
                        GL_NOTE_PAYABLE.into(),
                        0.0,
                        input.face_amount,
                        None,
                        &summary,
                    ),
                ],
            ),
            // 收到支票：登记即到账，借入账账户（资金行带 fund_account_id）/ 贷对方（缺省 1122）
            (INSTRUMENT_DIRECTION_RECEIVED, true) => {
                let account = input.fund_account_id.ok_or_else(|| {
                    AppError::General("收到支票登记即到账，必须选择入账资金账户".into())
                })?;
                let gl = fund_account_gl_code(&tx, account)?;
                (
                    INSTRUMENT_STATUS_COLLECTED,
                    vec![
                        gl_line(gl, input.face_amount, 0.0, Some(account), &summary),
                        gl_line(
                            effective_counter(&input.counter_account_code, GL_RECEIVABLE_DEFAULT),
                            0.0,
                            input.face_amount,
                            None,
                            &summary,
                        ),
                    ],
                )
            }
            // 开出支票：登记即付款，借对方（缺省 2202 应付账款）/ 贷出账账户（资金行带 fund_account_id）
            (INSTRUMENT_DIRECTION_ISSUED, true) => {
                let account = input.fund_account_id.ok_or_else(|| {
                    AppError::General("开出支票登记即付款，必须选择出账资金账户".into())
                })?;
                let gl = fund_account_gl_code(&tx, account)?;
                (
                    INSTRUMENT_STATUS_PAID,
                    vec![
                        gl_line(
                            effective_counter(&input.counter_account_code, GL_PAYABLE_DEFAULT),
                            input.face_amount,
                            0.0,
                            None,
                            &summary,
                        ),
                        gl_line(gl, 0.0, input.face_amount, Some(account), &summary),
                    ],
                )
            }
            _ => return Err(AppError::InvalidParam("票据类型与方向组合无效".into())),
        };
    ensure_fund_voucher_lines(&lines)?;

    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO negotiable_instruments
            (instrument_type, direction, instrument_no, face_amount, issue_date, due_date,
             drawer, acceptor, payee, partner_id, fund_account_id, counter_account_code,
             status, remark, created_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
        params![
            input.instrument_type,
            input.direction,
            no,
            input.face_amount,
            input.issue_date.trim(),
            input.due_date.trim(),
            input.drawer,
            input.acceptor,
            input.payee,
            input.partner_id,
            input.fund_account_id,
            input.counter_account_code,
            status,
            input.remark,
            operator_name,
            now,
        ],
    )?;
    let id = tx.last_insert_rowid();
    let voucher = accounting::insert_voucher(
        &tx,
        &VoucherDraft {
            belong_month: register_month,
            voucher_date: input.issue_date.trim().to_string(),
            source_type: VOUCHER_SOURCE_INSTRUMENT.into(),
            source_id: id,
            remark: Some(summary),
            lines,
        },
    )?;
    tx.execute(
        "UPDATE negotiable_instruments SET voucher_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, voucher.id, now],
    )?;
    tx.commit()?;
    get_instrument(conn, id)
}

// ==================== 收到承兑流转：背书 / 贴现 / 托收 / 到账 ====================

/// 背书转出（holding → endorsed_out）：借 counter（缺省 2202 应付账款）/ 贷 1121 票面。
/// 本期约定全额背书：背书金额校验等于票面（容差 0.005）；背书序号同一票据内递增。
pub fn endorse_instrument(
    conn: &mut Connection,
    input: &InstrumentEndorseInput,
    operator: &str,
) -> AppResult<InstrumentEndorseResult> {
    let (instrument, endorsement) = transition_instrument(
        conn,
        input.instrument_id,
        "endorse",
        "背书",
        &[INSTRUMENT_STATUS_HOLDING],
        INSTRUMENT_STATUS_ENDORSED_OUT,
        &input.endorse_date,
        operator,
        None,
        false,
        |tx, inst, ctx, event_id, op_date, now| {
            let endorsee = input.endorsee.trim();
            if endorsee.is_empty() {
                return Err(AppError::InvalidParam("被背书人必填".into()));
            }
            if (input.amount - inst.face_amount).abs() > AMOUNT_TOLERANCE {
                return Err(AppError::InvalidParam(format!(
                    "背书金额 {:.2} 必须等于票面 {:.2}（本期仅支持全额背书）",
                    input.amount, inst.face_amount
                )));
            }
            let summary = format!("票据背书 {}", inst.instrument_no);
            let lines = vec![
                gl_line(
                    effective_counter(&input.counter_account_code, GL_PAYABLE_DEFAULT),
                    inst.face_amount,
                    0.0,
                    None,
                    &summary,
                ),
                gl_line(
                    GL_NOTE_RECEIVABLE.into(),
                    0.0,
                    inst.face_amount,
                    None,
                    &summary,
                ),
            ];
            ensure_fund_voucher_lines(&lines)?;
            let voucher = accounting::insert_voucher(
                tx,
                &VoucherDraft {
                    belong_month: month_of(validate_date(op_date, "背书日期")?),
                    voucher_date: op_date.to_string(),
                    source_type: VOUCHER_SOURCE_INSTRUMENT_FLOW.into(),
                    source_id: event_id,
                    remark: Some(summary),
                    lines,
                },
            )?;
            let purpose = input
                .purpose
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let order: i64 = tx.query_row(
                "SELECT COALESCE(MAX(endorse_order), 0) + 1 FROM instrument_endorsements
                 WHERE instrument_id = ?1",
                params![inst.id],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT INTO instrument_endorsements
                    (instrument_id, endorse_order, endorsee, endorse_date, purpose, amount,
                     voucher_id, created_by, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    inst.id,
                    order,
                    endorsee,
                    input.endorse_date.trim(),
                    purpose,
                    input.amount,
                    voucher.id,
                    ctx.name,
                    now
                ],
            )?;
            let endorsement = get_endorsement(tx, tx.last_insert_rowid())?;
            Ok(endorsement)
        },
    )?;
    Ok(InstrumentEndorseResult {
        instrument,
        endorsement,
    })
}

/// 贴现（holding → discounted）：借资金账户实收（带 fund_account_id）
/// + 借 6603 财务费用（票面 − 实收，差额 0 免腿）/ 贷 1121 票面；
/// 实收不得大于票面（严格大于即拒，不容差——防负差额免 6603 腿入库借贷不平凭证）。
pub fn discount_instrument(
    conn: &mut Connection,
    input: &InstrumentDiscountInput,
    operator: &str,
) -> AppResult<NegotiableInstrument> {
    Ok(transition_instrument(
        conn,
        input.instrument_id,
        "discount",
        "贴现",
        &[INSTRUMENT_STATUS_HOLDING],
        INSTRUMENT_STATUS_DISCOUNTED,
        &input.discount_date,
        operator,
        None,
        false,
        |tx, inst, _ctx, event_id, op_date, now| {
            if input.proceeds <= AMOUNT_TOLERANCE {
                return Err(AppError::InvalidParam("贴现实收金额必须大于 0".into()));
            }
            // 实收 > 票面即拒（严格大于，不带容差）：容差放行会因负差额免 6603 腿
            // 而入库借贷不平（差额 ≤ 容差）的凭证
            if input.proceeds > inst.face_amount {
                return Err(AppError::InvalidParam(format!(
                    "贴现实收 {:.2} 不得大于票面 {:.2}",
                    input.proceeds, inst.face_amount
                )));
            }
            let account = require_fund_account(
                input.fund_account_id,
                inst.fund_account_id,
                "贴现必须选择贴现入账资金账户",
            )?;
            let gl = fund_account_gl_code(tx, account)?;
            let fee = inst.face_amount - input.proceeds;
            let summary = format!("票据贴现 {}", inst.instrument_no);
            let mut lines = vec![gl_line(gl, input.proceeds, 0.0, Some(account), &summary)];
            // 财务费用 = 票面 − 实收；差额为 0（容差内）时免财务费用腿
            if fee > AMOUNT_TOLERANCE {
                lines.push(gl_line(GL_FINANCE_EXPENSE.into(), fee, 0.0, None, &summary));
            }
            lines.push(gl_line(
                GL_NOTE_RECEIVABLE.into(),
                0.0,
                inst.face_amount,
                None,
                &summary,
            ));
            ensure_fund_voucher_lines(&lines)?;
            accounting::insert_voucher(
                tx,
                &VoucherDraft {
                    belong_month: month_of(validate_date(op_date, "贴现日期")?),
                    voucher_date: op_date.to_string(),
                    source_type: VOUCHER_SOURCE_INSTRUMENT_FLOW.into(),
                    source_id: event_id,
                    remark: Some(summary),
                    lines,
                },
            )?;
            // 回写实收账户，作为票据行的最新资金路径
            tx.execute(
                "UPDATE negotiable_instruments SET fund_account_id = ?2, updated_at = ?3
                 WHERE id = ?1",
                params![inst.id, account, now],
            )?;
            Ok(())
        },
    )?
    .0)
}

/// 托收发起（holding → collecting）：在途不记账，无凭证；操作以审批事件留痕。
pub fn start_collection(
    conn: &mut Connection,
    instrument_id: i64,
    operate_date: &str,
    operator: &str,
) -> AppResult<NegotiableInstrument> {
    Ok(transition_instrument(
        conn,
        instrument_id,
        "collect",
        "托收",
        &[INSTRUMENT_STATUS_HOLDING],
        INSTRUMENT_STATUS_COLLECTING,
        operate_date,
        operator,
        None,
        false,
        |_, _, _, _, _, _| Ok(()),
    )?
    .0)
}

/// 托收到账确认（collecting → collected）：借入账账户（带 fund_account_id）/ 贷 1121 票面。
pub fn confirm_collection(
    conn: &mut Connection,
    input: &InstrumentCollectConfirmInput,
    operator: &str,
) -> AppResult<NegotiableInstrument> {
    Ok(transition_instrument(
        conn,
        input.instrument_id,
        "confirm_collect",
        "到账确认",
        &[INSTRUMENT_STATUS_COLLECTING],
        INSTRUMENT_STATUS_COLLECTED,
        &input.received_date,
        operator,
        None,
        false,
        |tx, inst, _ctx, event_id, op_date, now| {
            let account = require_fund_account(
                input.fund_account_id,
                inst.fund_account_id,
                "到账确认必须选择入账资金账户",
            )?;
            let gl = fund_account_gl_code(tx, account)?;
            let summary = format!("托收到账 {}", inst.instrument_no);
            let lines = vec![
                gl_line(gl, inst.face_amount, 0.0, Some(account), &summary),
                gl_line(
                    GL_NOTE_RECEIVABLE.into(),
                    0.0,
                    inst.face_amount,
                    None,
                    &summary,
                ),
            ];
            ensure_fund_voucher_lines(&lines)?;
            accounting::insert_voucher(
                tx,
                &VoucherDraft {
                    belong_month: month_of(validate_date(op_date, "到账日期")?),
                    voucher_date: op_date.to_string(),
                    source_type: VOUCHER_SOURCE_INSTRUMENT_FLOW.into(),
                    source_id: event_id,
                    remark: Some(summary),
                    lines,
                },
            )?;
            tx.execute(
                "UPDATE negotiable_instruments SET fund_account_id = ?2, updated_at = ?3
                 WHERE id = ?1",
                params![inst.id, account, now],
            )?;
            Ok(())
        },
    )?
    .0)
}

// ==================== 开出承兑兑付 ====================

/// 开出承兑兑付（issued_outstanding → paid）：借 2201 应付票据（销账）/ 贷出账账户（带 fund_account_id）。
pub fn settle_issued_instrument(
    conn: &mut Connection,
    input: &InstrumentSettleInput,
    operator: &str,
) -> AppResult<NegotiableInstrument> {
    Ok(transition_instrument(
        conn,
        input.instrument_id,
        "settle",
        "兑付",
        &[INSTRUMENT_STATUS_ISSUED_OUTSTANDING],
        INSTRUMENT_STATUS_PAID,
        &input.settle_date,
        operator,
        None,
        false,
        |tx, inst, _ctx, event_id, op_date, now| {
            let account = require_fund_account(
                input.fund_account_id,
                inst.fund_account_id,
                "兑付必须选择出账资金账户",
            )?;
            let gl = fund_account_gl_code(tx, account)?;
            let summary = format!("票据兑付 {}", inst.instrument_no);
            let lines = vec![
                gl_line(
                    GL_NOTE_PAYABLE.into(),
                    inst.face_amount,
                    0.0,
                    None,
                    &summary,
                ),
                gl_line(gl, 0.0, inst.face_amount, Some(account), &summary),
            ];
            ensure_fund_voucher_lines(&lines)?;
            accounting::insert_voucher(
                tx,
                &VoucherDraft {
                    belong_month: month_of(validate_date(op_date, "兑付日期")?),
                    voucher_date: op_date.to_string(),
                    source_type: VOUCHER_SOURCE_INSTRUMENT_FLOW.into(),
                    source_id: event_id,
                    remark: Some(summary),
                    lines,
                },
            )?;
            tx.execute(
                "UPDATE negotiable_instruments SET fund_account_id = ?2, updated_at = ?3
                 WHERE id = ?1",
                params![inst.id, account, now],
            )?;
            Ok(())
        },
    )?
    .0)
}

// ==================== 作废与红字冲正 ====================

/// 作废（holding / issued_outstanding → void，仅未流转票据）：登记凭证置 void
///（报表不再计入，等同净影响 0），票据同号可重新登记；操作以审批事件留痕。
/// 已流转票据纠错须走 `reverse_instrument_flow` 红字冲正。
pub fn void_instrument(
    conn: &mut Connection,
    instrument_id: i64,
    reason: &str,
    operator: &str,
) -> AppResult<NegotiableInstrument> {
    let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
    Ok(transition_instrument(
        conn,
        instrument_id,
        "void",
        "作废",
        &[
            INSTRUMENT_STATUS_HOLDING,
            INSTRUMENT_STATUS_ISSUED_OUTSTANDING,
        ],
        INSTRUMENT_STATUS_VOID,
        &today,
        operator,
        Some(reason),
        true,
        |tx, inst, _ctx, _event_id, _op_date, now| {
            let voucher_id = inst.voucher_id.ok_or_else(|| {
                AppError::General(format!(
                    "票据 {} 缺少登记凭证，无法作废",
                    inst.instrument_no
                ))
            })?;
            let updated = tx.execute(
                "UPDATE vouchers SET status = 'void', updated_at = ?2
                 WHERE id = ?1 AND status = 'active'",
                params![voucher_id, now],
            )?;
            if updated == 0 {
                return Err(AppError::General(
                    "登记凭证已非生效状态，无法作废票据".into(),
                ));
            }
            Ok(())
        },
    )?
    .0)
}

/// 已流转票据红字冲正（spec 4 约束，复用第七阶段口径）：原凭证保留 active +
/// 生成借贷互换的反向凭证，净影响 0；冲正原因必填，approval_events 留痕；
/// 登记月与冲正月均须未月结（已月结月份须先反月结）。
///
/// 恢复状态（撤销最后一次流转，各恢复态均有后续出口，不留死票）：
/// - endorsed_out / discounted → holding
/// - collected：承兑 → holding（整段托收 episode 撤销，可改道或作废重录）；支票 → void（登记即到账的纠错，同号可重录）
/// - paid：承兑 → issued_outstanding（可重新兑付或作废）；支票 → void
///
/// 被冲正凭证定位：承兑按动作取最近一次流转事件的 active 凭证；
/// 支票（登记即终态）取登记凭证。
pub fn reverse_instrument_flow(
    conn: &mut Connection,
    input: &InstrumentReverseInput,
    operator: &str,
) -> AppResult<NegotiableInstrument> {
    if input.reason.trim().is_empty() {
        return Err(AppError::InvalidParam("冲正必须填写原因".into()));
    }
    // 预读票据决定恢复状态与被冲正动作（权威校验在骨架事务内重做）
    let inst = get_instrument(conn, input.instrument_id)?;
    let is_check = inst.instrument_type == INSTRUMENT_TYPE_CHECK;
    let (to_status, source_action): (&'static str, &str) = match inst.status.as_str() {
        INSTRUMENT_STATUS_ENDORSED_OUT => (INSTRUMENT_STATUS_HOLDING, "endorse"),
        INSTRUMENT_STATUS_DISCOUNTED => (INSTRUMENT_STATUS_HOLDING, "discount"),
        INSTRUMENT_STATUS_COLLECTED => {
            if is_check {
                (INSTRUMENT_STATUS_VOID, "")
            } else {
                (INSTRUMENT_STATUS_HOLDING, "confirm_collect")
            }
        }
        INSTRUMENT_STATUS_PAID => {
            if is_check {
                (INSTRUMENT_STATUS_VOID, "")
            } else {
                (INSTRUMENT_STATUS_ISSUED_OUTSTANDING, "settle")
            }
        }
        other => {
            return Err(AppError::General(format!(
                "票据 {} 当前状态「{}」，无可冲正的流转（仅已背书/已贴现/已到账/已兑付可冲正；未流转票据请使用作废）",
                inst.instrument_no,
                status_label(other)
            )))
        }
    };
    Ok(transition_instrument(
        conn,
        input.instrument_id,
        "reverse",
        "冲正",
        &[inst.status.as_str()],
        to_status,
        &input.reverse_date,
        operator,
        Some(&input.reason),
        true,
        |tx, inst, _ctx, event_id, op_date, _now| {
            // 定位被冲正凭证：支票取登记凭证，承兑按动作取最近流转事件的凭证
            let source_voucher = if is_check {
                accounting::get_active_voucher_for_source(tx, VOUCHER_SOURCE_INSTRUMENT, inst.id)?
                    .ok_or_else(|| {
                    AppError::General(format!(
                        "票据 {} 缺少生效的登记凭证，无法冲正",
                        inst.instrument_no
                    ))
                })?
            } else {
                let source_event_id: i64 = tx
                    .query_row(
                        "SELECT id FROM approval_events
                         WHERE entity_type = ?1 AND entity_id = ?2 AND action = ?3
                         ORDER BY id DESC LIMIT 1",
                        params![ENTITY_TYPE_INSTRUMENT, inst.id, source_action],
                        |r| r.get(0),
                    )
                    .optional()?
                    .ok_or_else(|| {
                        AppError::General(format!(
                            "票据 {} 未找到原流转记录，无法冲正",
                            inst.instrument_no
                        ))
                    })?;
                accounting::get_active_voucher_for_source(
                    tx,
                    VOUCHER_SOURCE_INSTRUMENT_FLOW,
                    source_event_id,
                )?
                .ok_or_else(|| {
                    AppError::General(format!(
                        "票据 {} 的原流转凭证不存在或已失效，无法冲正",
                        inst.instrument_no
                    ))
                })?
            };
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
                tx,
                &VoucherDraft {
                    belong_month: month_of(validate_date(op_date, "冲正日期")?),
                    voucher_date: op_date.to_string(),
                    source_type: VOUCHER_SOURCE_INSTRUMENT_FLOW.into(),
                    source_id: event_id,
                    remark: Some(format!(
                        "冲正票据 {}：{}",
                        inst.instrument_no,
                        input.reason.trim()
                    )),
                    lines,
                },
            )?;
            Ok(())
        },
    )?
    .0)
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::setup_financial_db;

    const OPERATOR: &str = "张会计";

    /// 票据测试环境：财务库 + 操作人张会计 + 银行账户(1002) + 现金账户(1001) + 供应商
    struct NotesEnv {
        conn: Connection,
        bank_account_id: i64,
        cash_account_id: i64,
    }

    fn notes_env() -> NotesEnv {
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
             VALUES ('BANK-001', '基本户', 'bank', '1002')",
            [],
        )
        .unwrap();
        let bank_account_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO fund_accounts (account_code, name, account_type, gl_account_code)
             VALUES ('CASH-001', '现金库', 'cash', '1001')",
            [],
        )
        .unwrap();
        let cash_account_id = conn.last_insert_rowid();
        NotesEnv {
            conn,
            bank_account_id,
            cash_account_id,
        }
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

    fn register_input(
        instrument_type: &str,
        direction: &str,
        instrument_no: &str,
    ) -> InstrumentRegisterInput {
        InstrumentRegisterInput {
            instrument_type: instrument_type.into(),
            direction: direction.into(),
            instrument_no: instrument_no.into(),
            face_amount: 100_000.0,
            issue_date: "2026-09-01".into(),
            due_date: "2026-12-01".into(),
            drawer: Some("出票人甲".into()),
            acceptor: None,
            payee: Some("本公司".into()),
            partner_id: None,
            fund_account_id: None,
            counter_account_code: None,
            remark: None,
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
        assert!(lines.len() >= 2, "凭证 {voucher_id} 应至少有两行分录");
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

    /// 票据登记凭证 id
    fn register_voucher_id(conn: &Connection, instrument_id: i64) -> i64 {
        conn.query_row(
            "SELECT voucher_id FROM negotiable_instruments WHERE id = ?1",
            params![instrument_id],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// 票据全部 active 凭证 id（登记 + 流转 + 冲正）
    fn instrument_active_vouchers(conn: &Connection, instrument_id: i64) -> Vec<i64> {
        let mut stmt = conn
            .prepare(
                "SELECT v.id FROM vouchers v
                 WHERE v.status = 'active' AND (
                     (v.source_type = 'instrument' AND v.source_id = ?1)
                  OR (v.source_type = 'instrument_flow' AND v.source_id IN
                      (SELECT id FROM approval_events WHERE entity_type = 'negotiable_instrument'
                       AND entity_id = ?1)))",
            )
            .unwrap();
        stmt.query_map(params![instrument_id], |r| r.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
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

    /// 票据审批事件 action 集合（按 id 升序）
    fn instrument_event_actions(conn: &Connection, instrument_id: i64) -> Vec<String> {
        let mut stmt = conn
            .prepare(
                "SELECT action FROM approval_events
                 WHERE entity_type = 'negotiable_instrument' AND entity_id = ?1 ORDER BY id",
            )
            .unwrap();
        stmt.query_map(params![instrument_id], |r| r.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    /// 最近一条指定动作流转事件对应的 active 凭证 id
    fn flow_voucher_id(conn: &Connection, instrument_id: i64, action: &str) -> i64 {
        conn.query_row(
            "SELECT v.id FROM vouchers v
             WHERE v.source_type = 'instrument_flow' AND v.source_id IN
                 (SELECT id FROM approval_events WHERE entity_type = 'negotiable_instrument'
                  AND entity_id = ?1 AND action = ?2)",
            params![instrument_id, action],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn endorse_input(instrument_id: i64) -> InstrumentEndorseInput {
        InstrumentEndorseInput {
            instrument_id,
            endorsee: "被背书人乙".into(),
            endorse_date: "2026-09-10".into(),
            amount: 100_000.0,
            purpose: Some("付货款".into()),
            counter_account_code: None,
        }
    }

    fn discount_input(instrument_id: i64, proceeds: f64) -> InstrumentDiscountInput {
        InstrumentDiscountInput {
            instrument_id,
            discount_date: "2026-09-15".into(),
            proceeds,
            fund_account_id: None,
        }
    }

    fn confirm_input(instrument_id: i64) -> InstrumentCollectConfirmInput {
        InstrumentCollectConfirmInput {
            instrument_id,
            received_date: "2026-09-20".into(),
            fund_account_id: None,
        }
    }

    fn settle_input(instrument_id: i64) -> InstrumentSettleInput {
        InstrumentSettleInput {
            instrument_id,
            settle_date: "2026-12-01".into(),
            fund_account_id: None,
        }
    }

    fn reverse_input(instrument_id: i64) -> InstrumentReverseInput {
        InstrumentReverseInput {
            instrument_id,
            reverse_date: "2026-09-25".into(),
            reason: "流转录错，红字冲正".into(),
        }
    }

    // ---------- 登记四组合 ----------

    #[test]
    fn test_register_received_acceptance_holding_entries() {
        let mut env = notes_env();
        let inst = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026001"),
            OPERATOR,
        )
        .unwrap();
        assert_eq!(inst.status, "holding");
        assert_eq!(
            inst.created_by.as_deref(),
            Some(OPERATOR),
            "登记须操作人署名"
        );
        let voucher_id = register_voucher_id(&env.conn, inst.id);
        // 借 1121 票面 / 贷 1122 票面；无资金行
        assert_two_lines(
            &env.conn,
            voucher_id,
            [
                ("1121", 100_000.0, 0.0, None),
                ("1122", 0.0, 100_000.0, None),
            ],
        );
    }

    #[test]
    fn test_register_issued_acceptance_entries_and_counter_override() {
        let mut env = notes_env();
        let mut input = register_input("commercial_acceptance", "issued", "YZ2026002");
        input.counter_account_code = Some(" 1221 ".into());
        let inst = register_instrument(&mut env.conn, &input, OPERATOR).unwrap();
        assert_eq!(inst.status, "issued_outstanding");
        let voucher_id = register_voucher_id(&env.conn, inst.id);
        // counter_account_code 覆盖缺省：借 1221 / 贷 2201 应付票据
        assert_two_lines(
            &env.conn,
            voucher_id,
            [
                ("1221", 100_000.0, 0.0, None),
                ("2201", 0.0, 100_000.0, None),
            ],
        );
    }

    #[test]
    fn test_register_check_received_collected_with_fund_line() {
        let mut env = notes_env();
        let mut input = register_input("check", "received", "ZP2026001");
        input.fund_account_id = Some(env.bank_account_id);
        input.due_date = "2026-09-01".into();
        let inst = register_instrument(&mut env.conn, &input, OPERATOR).unwrap();
        assert_eq!(inst.status, "collected", "收到支票登记即到账终态");
        let voucher_id = register_voucher_id(&env.conn, inst.id);
        // 借 1002（带 fund_account_id）/ 贷 1122
        assert_two_lines(
            &env.conn,
            voucher_id,
            [
                ("1002", 100_000.0, 0.0, Some(env.bank_account_id)),
                ("1122", 0.0, 100_000.0, None),
            ],
        );
    }

    #[test]
    fn test_register_check_issued_paid_with_fund_line() {
        let mut env = notes_env();
        let mut input = register_input("check", "issued", "ZP2026002");
        input.fund_account_id = Some(env.bank_account_id);
        input.due_date = "2026-09-01".into();
        let inst = register_instrument(&mut env.conn, &input, OPERATOR).unwrap();
        assert_eq!(inst.status, "paid", "开出支票登记即付款终态");
        let voucher_id = register_voucher_id(&env.conn, inst.id);
        // 借 2202 应付账款（缺省对方）/ 贷 1002（带 fund_account_id）
        assert_two_lines(
            &env.conn,
            voucher_id,
            [
                ("2202", 100_000.0, 0.0, None),
                ("1002", 0.0, 100_000.0, Some(env.bank_account_id)),
            ],
        );
    }

    #[test]
    fn test_register_check_requires_fund_account() {
        let mut env = notes_env();
        let err = register_instrument(
            &mut env.conn,
            &register_input("check", "received", "ZP2026003"),
            OPERATOR,
        )
        .unwrap_err();
        assert!(err.to_string().contains("入账资金账户"), "{err}");
    }

    #[test]
    fn test_register_duplicate_no_rejected_then_void_allows_reentry() {
        let mut env = notes_env();
        let first = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026003"),
            OPERATOR,
        )
        .unwrap();
        let err = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026003"),
            OPERATOR,
        )
        .unwrap_err();
        assert!(err.to_string().contains("已存在同类型有效票据"), "{err}");
        // 作废后同号可再录
        void_instrument(&mut env.conn, first.id, "录错重开", OPERATOR).unwrap();
        let again = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026003"),
            OPERATOR,
        )
        .unwrap();
        assert_eq!(again.status, "holding");
    }

    #[test]
    fn test_register_validations() {
        let mut env = notes_env();
        // 到期日早于出票日
        let mut input = register_input("bank_acceptance", "received", "YZ2026010");
        input.due_date = "2026-08-31".into();
        let err = register_instrument(&mut env.conn, &input, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("到期日"), "{err}");
        // 票面 <= 0
        let mut input = register_input("bank_acceptance", "received", "YZ2026010");
        input.face_amount = 0.0;
        let err = register_instrument(&mut env.conn, &input, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("票面金额"), "{err}");
        // 往来单位不存在
        let mut input = register_input("bank_acceptance", "received", "YZ2026010");
        input.partner_id = Some(9999);
        let err = register_instrument(&mut env.conn, &input, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("往来单位不存在"), "{err}");
        // 未知操作人 / 空操作人
        let err = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026010"),
            "路人甲",
        )
        .unwrap_err();
        assert!(err.to_string().contains("操作人"), "{err}");
        let err = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026010"),
            "  ",
        )
        .unwrap_err();
        assert!(err.to_string().contains("操作人"), "{err}");
        // 登记月已月结
        close_month(&env.conn, "2026-08");
        let mut input = register_input("bank_acceptance", "received", "YZ2026010");
        input.issue_date = "2026-08-20".into();
        input.due_date = "2026-11-20".into();
        let err = register_instrument(&mut env.conn, &input, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("已正式月结"), "{err}");
    }

    // ---------- 背书 ----------

    #[test]
    fn test_endorse_entries_and_chain() {
        let mut env = notes_env();
        let inst = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026004"),
            OPERATOR,
        )
        .unwrap();
        let result = endorse_instrument(&mut env.conn, &endorse_input(inst.id), OPERATOR).unwrap();
        assert_eq!(result.instrument.status, "endorsed_out");
        assert_eq!(result.endorsement.endorse_order, 1);
        assert!((result.endorsement.amount - 100_000.0).abs() < AMOUNT_TOLERANCE);
        assert_eq!(result.endorsement.created_by.as_deref(), Some(OPERATOR));
        // 借 2202 应付账款（缺省对方）/ 贷 1121，凭证与背书链关联
        assert_two_lines(
            &env.conn,
            result.endorsement.voucher_id,
            [
                ("2202", 100_000.0, 0.0, None),
                ("1121", 0.0, 100_000.0, None),
            ],
        );
        let chain_voucher: i64 = env
            .conn
            .query_row(
                "SELECT voucher_id FROM instrument_endorsements WHERE id = ?1",
                params![result.endorsement.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(chain_voucher, result.endorsement.voucher_id);
        // 冲正后再背书：序号递增为 2
        reverse_instrument_flow(&mut env.conn, &reverse_input(inst.id), OPERATOR).unwrap();
        let again = endorse_instrument(&mut env.conn, &endorse_input(inst.id), OPERATOR).unwrap();
        assert_eq!(again.endorsement.endorse_order, 2, "背书序号同一票据内递增");
    }

    #[test]
    fn test_endorse_guards() {
        let mut env = notes_env();
        let inst = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026005"),
            OPERATOR,
        )
        .unwrap();
        // 背书金额 != 票面
        let mut input = endorse_input(inst.id);
        input.amount = 99_000.0;
        let err = endorse_instrument(&mut env.conn, &input, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("全额背书"), "{err}");
        // 托收中禁背书
        start_collection(&mut env.conn, inst.id, "2026-09-05", OPERATOR).unwrap();
        let err = endorse_instrument(&mut env.conn, &endorse_input(inst.id), OPERATOR).unwrap_err();
        assert!(err.to_string().contains("托收中"), "{err}");
        // 托收中同样禁贴现
        let err = discount_instrument(&mut env.conn, &discount_input(inst.id, 99_000.0), OPERATOR)
            .unwrap_err();
        assert!(err.to_string().contains("托收中"), "{err}");
    }

    // ---------- 贴现 ----------

    #[test]
    fn test_discount_with_expense_leg() {
        let mut env = notes_env();
        let mut input = register_input("bank_acceptance", "received", "YZ2026006");
        input.fund_account_id = Some(env.bank_account_id);
        let inst = register_instrument(&mut env.conn, &input, OPERATOR).unwrap();
        let discounted =
            discount_instrument(&mut env.conn, &discount_input(inst.id, 99_000.0), OPERATOR)
                .unwrap();
        assert_eq!(discounted.status, "discounted");
        assert_eq!(
            discounted.fund_account_id,
            Some(env.bank_account_id),
            "实收账户回写"
        );
        let voucher_id = flow_voucher_id(&env.conn, inst.id, "discount");
        let lines = voucher_lines(&env.conn, voucher_id);
        // 借 1002 实收 99000（带账户）+ 借 6603 贴现息 1000 / 贷 1121 票面 100000
        let expect = [
            ("1002", 99_000.0, 0.0, Some(env.bank_account_id)),
            ("6603", 1_000.0, 0.0, None),
            ("1121", 0.0, 100_000.0, None),
        ];
        assert_eq!(lines.len(), expect.len(), "贴现息 1000 应生成三条分录");
        for (actual, expected) in lines.iter().zip(expect.iter()) {
            assert_eq!(actual.0, expected.0, "贴现凭证科目不符");
            assert!(
                (actual.1 - expected.1).abs() < AMOUNT_TOLERANCE
                    && (actual.2 - expected.2).abs() < AMOUNT_TOLERANCE,
                "贴现凭证金额不符：{actual:?} vs {expected:?}"
            );
            assert_eq!(actual.3, expected.3, "贴现凭证 fund_account_id 不符");
        }
    }

    #[test]
    fn test_discount_zero_fee_skips_expense_leg() {
        let mut env = notes_env();
        let inst = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026007"),
            OPERATOR,
        )
        .unwrap();
        let mut input = discount_input(inst.id, 100_000.0);
        input.fund_account_id = Some(env.cash_account_id);
        let discounted = discount_instrument(&mut env.conn, &input, OPERATOR).unwrap();
        assert_eq!(discounted.fund_account_id, Some(env.cash_account_id));
        let voucher_id = flow_voucher_id(&env.conn, inst.id, "discount");
        // 实收 = 票面：仅两腿，无 6603；资金行挂现金账户（科目 1001）
        assert_two_lines(
            &env.conn,
            voucher_id,
            [
                ("1001", 100_000.0, 0.0, Some(env.cash_account_id)),
                ("1121", 0.0, 100_000.0, None),
            ],
        );
        let lines = voucher_lines(&env.conn, voucher_id);
        assert_eq!(lines.len(), 2, "差额 0 免财务费用腿");
    }

    #[test]
    fn test_discount_proceeds_exceed_face_rejected() {
        let mut env = notes_env();
        let inst = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026008"),
            OPERATOR,
        )
        .unwrap();
        let err = discount_instrument(&mut env.conn, &discount_input(inst.id, 100_001.0), OPERATOR)
            .unwrap_err();
        assert!(err.to_string().contains("不得大于票面"), "{err}");
        // 容差内超收同样拒绝：实收 > 票面即拒（严格大于，不带容差），
        // 否则负差额免 6603 腿会入库借贷不平（差额 ≤ 0.005）的凭证
        let err = discount_instrument(
            &mut env.conn,
            &discount_input(inst.id, 100_000.004),
            OPERATOR,
        )
        .unwrap_err();
        assert!(err.to_string().contains("不得大于票面"), "{err}");
        // 实收 <= 0
        let err = discount_instrument(&mut env.conn, &discount_input(inst.id, 0.0), OPERATOR)
            .unwrap_err();
        assert!(err.to_string().contains("大于 0"), "{err}");
        // 无任何可用资金账户
        let err = discount_instrument(&mut env.conn, &discount_input(inst.id, 99_000.0), OPERATOR)
            .unwrap_err();
        assert!(err.to_string().contains("资金账户"), "{err}");
        assert_eq!(
            get_instrument(&env.conn, inst.id).unwrap().status,
            "holding",
            "失败贴现不得改变状态"
        );
    }

    // ---------- 托收与到账 ----------

    #[test]
    fn test_start_collection_then_confirm() {
        let mut env = notes_env();
        let mut input = register_input("bank_acceptance", "received", "YZ2026009");
        input.fund_account_id = Some(env.bank_account_id);
        let inst = register_instrument(&mut env.conn, &input, OPERATOR).unwrap();
        let vouchers_before = instrument_active_vouchers(&env.conn, inst.id).len();
        let collecting = start_collection(&mut env.conn, inst.id, "2026-09-05", OPERATOR).unwrap();
        assert_eq!(collecting.status, "collecting");
        assert_eq!(
            instrument_active_vouchers(&env.conn, inst.id).len(),
            vouchers_before,
            "托收在途不记账"
        );
        assert_eq!(
            instrument_event_actions(&env.conn, inst.id),
            vec!["collect".to_string()],
            "托收发起须事件留痕"
        );
        let collected =
            confirm_collection(&mut env.conn, &confirm_input(inst.id), OPERATOR).unwrap();
        assert_eq!(collected.status, "collected");
        let voucher_id = flow_voucher_id(&env.conn, inst.id, "confirm_collect");
        assert_two_lines(
            &env.conn,
            voucher_id,
            [
                ("1002", 100_000.0, 0.0, Some(env.bank_account_id)),
                ("1121", 0.0, 100_000.0, None),
            ],
        );
        // 终态不能再确认
        let err = confirm_collection(&mut env.conn, &confirm_input(inst.id), OPERATOR).unwrap_err();
        assert!(err.to_string().contains("已到账"), "{err}");
    }

    // ---------- 开出承兑兑付 ----------

    #[test]
    fn test_settle_issued_instrument() {
        let mut env = notes_env();
        let mut input = register_input("bank_acceptance", "issued", "YC2026001");
        input.fund_account_id = Some(env.bank_account_id);
        let inst = register_instrument(&mut env.conn, &input, OPERATOR).unwrap();
        let paid =
            settle_issued_instrument(&mut env.conn, &settle_input(inst.id), OPERATOR).unwrap();
        assert_eq!(paid.status, "paid");
        let voucher_id = flow_voucher_id(&env.conn, inst.id, "settle");
        // 借 2201 应付票据（销账）/ 贷 1002（带账户）
        assert_two_lines(
            &env.conn,
            voucher_id,
            [
                ("2201", 100_000.0, 0.0, None),
                ("1002", 0.0, 100_000.0, Some(env.bank_account_id)),
            ],
        );
        // holding 不能兑付（来源状态门禁）
        let other = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YC2026002"),
            OPERATOR,
        )
        .unwrap();
        let err =
            settle_issued_instrument(&mut env.conn, &settle_input(other.id), OPERATOR).unwrap_err();
        assert!(err.to_string().contains("持有"), "{err}");
    }

    // ---------- 作废 ----------

    #[test]
    fn test_void_instrument_voids_registration_voucher() {
        let mut env = notes_env();
        let holding = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026011"),
            OPERATOR,
        )
        .unwrap();
        let voided = void_instrument(&mut env.conn, holding.id, "票录错作废", OPERATOR).unwrap();
        assert_eq!(voided.status, "void");
        let voucher_status: String = env
            .conn
            .query_row(
                "SELECT status FROM vouchers WHERE id = ?1",
                params![register_voucher_id(&env.conn, holding.id)],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(voucher_status, "void", "作废须同步置登记凭证 void");
        assert_eq!(
            instrument_event_actions(&env.conn, holding.id),
            vec!["void".to_string()]
        );
        // 开出未兑付也可作废
        let issued = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "issued", "YC2026003"),
            OPERATOR,
        )
        .unwrap();
        void_instrument(&mut env.conn, issued.id, "承兑作废", OPERATOR).unwrap();
        // 已流转票据禁作废（走红字冲正）
        let flowed = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026012"),
            OPERATOR,
        )
        .unwrap();
        endorse_instrument(&mut env.conn, &endorse_input(flowed.id), OPERATOR).unwrap();
        let err = void_instrument(&mut env.conn, flowed.id, "已背书作废", OPERATOR).unwrap_err();
        assert!(err.to_string().contains("已背书转出"), "{err}");
        // 原因必填
        let err = void_instrument(&mut env.conn, issued.id, "  ", OPERATOR).unwrap_err();
        assert!(err.to_string().contains("原因"), "{err}");
    }

    // ---------- 红字冲正 ----------

    #[test]
    fn test_reverse_endorse_restores_holding_net_zero() {
        let mut env = notes_env();
        let inst = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026013"),
            OPERATOR,
        )
        .unwrap();
        let endorsed =
            endorse_instrument(&mut env.conn, &endorse_input(inst.id), OPERATOR).unwrap();
        let reversed =
            reverse_instrument_flow(&mut env.conn, &reverse_input(inst.id), OPERATOR).unwrap();
        assert_eq!(reversed.status, "holding", "冲正背书恢复持有");
        // 原登记/背书凭证保留 active，反向凭证并存，组合净影响 0
        for (label, vid) in [
            ("登记", register_voucher_id(&env.conn, inst.id)),
            ("背书", endorsed.endorsement.voucher_id),
        ] {
            let status: String = env
                .conn
                .query_row(
                    "SELECT status FROM vouchers WHERE id = ?1",
                    params![vid],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(status, "active", "{label}凭证保留 active（红字口径）");
        }
        let all = instrument_active_vouchers(&env.conn, inst.id);
        assert_eq!(all.len(), 3, "登记 + 背书 + 冲正三张 active 凭证");
        // 净影响 0 的口径是"被冲正凭证 + 冲正凭证"（登记凭证独立存在，不参与本次对冲）
        assert_net_zero(
            &env.conn,
            &[
                endorsed.endorsement.voucher_id,
                flow_voucher_id(&env.conn, inst.id, "reverse"),
            ],
        );
        // 审批留痕：endorse + reverse
        assert_eq!(
            instrument_event_actions(&env.conn, inst.id),
            vec!["endorse".to_string(), "reverse".to_string()]
        );
    }

    #[test]
    fn test_reverse_discount_and_confirm_and_settle() {
        let mut env = notes_env();
        // 贴现冲正 → holding
        let mut input = register_input("bank_acceptance", "received", "YZ2026014");
        input.fund_account_id = Some(env.bank_account_id);
        let discounted = register_instrument(&mut env.conn, &input, OPERATOR).unwrap();
        discount_instrument(
            &mut env.conn,
            &discount_input(discounted.id, 98_000.0),
            OPERATOR,
        )
        .unwrap();
        let reversed =
            reverse_instrument_flow(&mut env.conn, &reverse_input(discounted.id), OPERATOR)
                .unwrap();
        assert_eq!(reversed.status, "holding");
        assert_net_zero(
            &env.conn,
            &[
                flow_voucher_id(&env.conn, discounted.id, "discount"),
                flow_voucher_id(&env.conn, discounted.id, "reverse"),
            ],
        );
        // 托收到账冲正 → holding
        let mut collected_input = register_input("bank_acceptance", "received", "YZ2026015");
        collected_input.fund_account_id = Some(env.bank_account_id);
        let collected = register_instrument(&mut env.conn, &collected_input, OPERATOR).unwrap();
        start_collection(&mut env.conn, collected.id, "2026-09-05", OPERATOR).unwrap();
        confirm_collection(&mut env.conn, &confirm_input(collected.id), OPERATOR).unwrap();
        let reversed =
            reverse_instrument_flow(&mut env.conn, &reverse_input(collected.id), OPERATOR).unwrap();
        assert_eq!(
            reversed.status, "holding",
            "冲正到账恢复持有（整段托收 episode 撤销）"
        );
        assert_net_zero(
            &env.conn,
            &[
                flow_voucher_id(&env.conn, collected.id, "confirm_collect"),
                flow_voucher_id(&env.conn, collected.id, "reverse"),
            ],
        );
        // 兑付冲正 → issued_outstanding
        let mut issued_input = register_input("bank_acceptance", "issued", "YC2026004");
        issued_input.fund_account_id = Some(env.bank_account_id);
        let paid = register_instrument(&mut env.conn, &issued_input, OPERATOR).unwrap();
        settle_issued_instrument(&mut env.conn, &settle_input(paid.id), OPERATOR).unwrap();
        let reversed =
            reverse_instrument_flow(&mut env.conn, &reverse_input(paid.id), OPERATOR).unwrap();
        assert_eq!(reversed.status, "issued_outstanding");
        assert_net_zero(
            &env.conn,
            &[
                flow_voucher_id(&env.conn, paid.id, "settle"),
                flow_voucher_id(&env.conn, paid.id, "reverse"),
            ],
        );
    }

    #[test]
    fn test_reverse_check_registration_goes_void() {
        let mut env = notes_env();
        let mut input = register_input("check", "received", "ZP2026004");
        input.fund_account_id = Some(env.bank_account_id);
        input.due_date = "2026-09-01".into();
        let check = register_instrument(&mut env.conn, &input, OPERATOR).unwrap();
        assert_eq!(check.status, "collected");
        let reversed =
            reverse_instrument_flow(&mut env.conn, &reverse_input(check.id), OPERATOR).unwrap();
        assert_eq!(reversed.status, "void", "支票退票冲正后作废，同号可重录");
        assert_net_zero(
            &env.conn,
            &[
                register_voucher_id(&env.conn, check.id),
                flow_voucher_id(&env.conn, check.id, "reverse"),
            ],
        );
        // 同号可再录（void 不占唯一索引）
        let mut again = register_input("check", "received", "ZP2026004");
        again.fund_account_id = Some(env.bank_account_id);
        again.due_date = "2026-09-01".into();
        let re_registered = register_instrument(&mut env.conn, &again, OPERATOR).unwrap();
        assert_eq!(re_registered.status, "collected");
    }

    #[test]
    fn test_reverse_guards() {
        let mut env = notes_env();
        let holding = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026016"),
            OPERATOR,
        )
        .unwrap();
        // 未流转票据无可冲正（应用作废）
        let err = reverse_instrument_flow(&mut env.conn, &reverse_input(holding.id), OPERATOR)
            .unwrap_err();
        assert!(err.to_string().contains("无可冲正的流转"), "{err}");
        // 原因必填
        let mut no_reason = reverse_input(holding.id);
        no_reason.reason = " ".into();
        let err = reverse_instrument_flow(&mut env.conn, &no_reason, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("原因"), "{err}");
        // 不存在票据
        let mut missing = reverse_input(holding.id);
        missing.instrument_id = 9999;
        let err = reverse_instrument_flow(&mut env.conn, &missing, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("票据不存在"), "{err}");
    }

    // ---------- 月结双查 ----------

    #[test]
    fn test_month_close_double_check() {
        let mut env = notes_env();
        // 票据登记在 2026-09（开放），操作日期落在已月结的 2026-08 → 操作月拒绝
        let inst = register_instrument(
            &mut env.conn,
            &register_input("bank_acceptance", "received", "YZ2026017"),
            OPERATOR,
        )
        .unwrap();
        close_month(&env.conn, "2026-08");
        let mut endorse = endorse_input(inst.id);
        endorse.endorse_date = "2026-08-20".into();
        let err = endorse_instrument(&mut env.conn, &endorse, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("已正式月结"), "{err}");
        // 登记月已月结 → 登记拒绝
        let mut backdated = register_input("bank_acceptance", "received", "YZ2026018");
        backdated.issue_date = "2026-08-10".into();
        backdated.due_date = "2026-11-10".into();
        let err = register_instrument(&mut env.conn, &backdated, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("已正式月结"), "{err}");
        // 跨月流转允许：登记 2026-09、操作 2026-10（双 open）
        let mut endorse = endorse_input(inst.id);
        endorse.endorse_date = "2026-10-08".into();
        let result = endorse_instrument(&mut env.conn, &endorse, OPERATOR).unwrap();
        assert_eq!(result.instrument.status, "endorsed_out");
        // 冲正月已月结 → 拒绝
        close_month(&env.conn, "2026-10");
        let mut reverse = reverse_input(inst.id);
        reverse.reverse_date = "2026-10-20".into();
        let err = reverse_instrument_flow(&mut env.conn, &reverse, OPERATOR).unwrap_err();
        assert!(err.to_string().contains("已正式月结"), "{err}");
        // 冲正月开放即可冲正（登记月 2026-09 未月结，不受 2026-08/10 影响）
        let mut reverse = reverse_input(inst.id);
        reverse.reverse_date = "2026-09-30".into();
        let reversed = reverse_instrument_flow(&mut env.conn, &reverse, OPERATOR).unwrap();
        assert_eq!(reversed.status, "holding");
    }
}
