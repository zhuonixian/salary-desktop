use crate::db;
use crate::errors::{AppError, AppResult};
use crate::models::*;
use chrono::Utc;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

/// 查询全部会计科目（按编码排序）。
pub fn get_accounts(conn: &Connection) -> AppResult<Vec<GlAccount>> {
    let mut stmt = conn.prepare(
        "SELECT code, name, category, direction, cash_flow_category, is_system, is_active, remark
         FROM gl_accounts ORDER BY code",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(GlAccount {
                code: r.get(0)?,
                name: r.get(1)?,
                category: r.get(2)?,
                direction: r.get(3)?,
                cash_flow_category: r.get(4)?,
                is_system: r.get(5)?,
                is_active: r.get(6)?,
                remark: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 新增自定义科目（is_system=0，默认启用）。编码重复时报错。
pub fn create_account(conn: &Connection, input: &GlAccountInput) -> AppResult<GlAccount> {
    let cfc = input
        .cash_flow_category
        .clone()
        .unwrap_or_else(|| "none".into());
    if conn
        .query_row(
            "SELECT 1 FROM gl_accounts WHERE code = ?1",
            params![input.code],
            |_| Ok(()),
        )
        .is_ok()
    {
        return Err(AppError::General(format!("科目编码 {} 已存在", input.code)));
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO gl_accounts (code, name, category, direction, cash_flow_category, is_system, is_active, remark, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 1, ?6, ?7, ?7)",
        params![
            input.code,
            input.name,
            input.category,
            input.direction,
            cfc,
            input.remark,
            now
        ],
    )?;
    Ok(GlAccount {
        code: input.code.clone(),
        name: input.name.clone(),
        category: input.category.clone(),
        direction: input.direction.clone(),
        cash_flow_category: cfc,
        is_system: 0,
        is_active: 1,
        remark: input.remark.clone(),
    })
}

/// 启用/停用科目。已有有效（active）凭证分录的科目不允许停用；作废凭证不阻塞。
pub fn set_account_active(conn: &Connection, code: &str, active: bool) -> AppResult<bool> {
    let used: i64 = conn.query_row(
        "SELECT COUNT(*) FROM voucher_lines vl JOIN vouchers v ON v.id = vl.voucher_id
         WHERE vl.account_code = ?1 AND v.status = 'active'",
        params![code],
        |r| r.get(0),
    )?;
    if !active && used > 0 {
        return Err(AppError::General(format!(
            "科目 {code} 已有 {used} 条凭证分录，不能停用"
        )));
    }
    conn.execute(
        "UPDATE gl_accounts SET is_active = ?2, updated_at = ?3 WHERE code = ?1",
        params![code, active as i64, Utc::now().to_rfc3339()],
    )?;
    Ok(true)
}

/// 读取期初余额：返回 (所属月份, 余额行列表)。无数据时月份为 None。
pub fn get_opening_balances(
    conn: &Connection,
) -> AppResult<(Option<String>, Vec<OpeningBalanceRow>)> {
    let month: Option<String> = conn
        .query_row("SELECT MIN(month) FROM opening_balances", [], |r| r.get(0))
        .unwrap_or(None);
    let mut rows = Vec::new();
    if let Some(m) = &month {
        let mut stmt = conn.prepare(
            "SELECT account_code, debit_amount, credit_amount FROM opening_balances WHERE month = ?1 ORDER BY account_code",
        )?;
        rows = stmt
            .query_map(params![m], |r| {
                Ok(OpeningBalanceRow {
                    account_code: r.get(0)?,
                    debit_amount: r.get(1)?,
                    credit_amount: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
    }
    Ok((month, rows))
}

/// 保存期初余额（整体覆盖：先校验方向与平衡，再清空旧数据重写）。
pub fn save_opening_balances(
    conn: &Connection,
    month: &str,
    rows: &[OpeningBalanceRow],
) -> AppResult<()> {
    // 先校验科目方向：借方科目金额应填借方，贷方科目金额应填贷方
    for row in rows {
        let direction: String = conn.query_row(
            "SELECT direction FROM gl_accounts WHERE code = ?1",
            params![row.account_code],
            |r| r.get(0),
        )?;
        let wrong = (direction == "debit" && row.credit_amount != 0.0)
            || (direction == "credit" && row.debit_amount != 0.0);
        if wrong {
            return Err(AppError::General(format!(
                "科目 {} 是{}方向科目，金额应填在{}侧",
                row.account_code,
                if direction == "debit" { "借" } else { "贷" },
                if direction == "debit" { "借" } else { "贷" }
            )));
        }
    }
    // 再校验借贷平衡
    let debit: f64 = rows.iter().map(|r| r.debit_amount).sum();
    let credit: f64 = rows.iter().map(|r| r.credit_amount).sum();
    if (debit - credit).abs() > 0.005 {
        return Err(AppError::General(format!(
            "期初余额不平衡：借方合计 {debit:.2}，贷方合计 {credit:.2}，差额 {:.2}",
            debit - credit
        )));
    }
    // 保存前查重：同一批 rows 中重复科目直接报错，避免部分写入后覆盖语义混乱
    let mut seen = HashSet::new();
    for row in rows {
        if !seen.insert(&row.account_code) {
            return Err(AppError::General(format!(
                "期初余额存在重复科目 {}",
                row.account_code
            )));
        }
    }
    // 启用月变更守卫：已有 active 记账凭证时不允许变更启用月，否则报表滚入窗口
    // （"启用月起的累计发生额"）会整体漂移，与已入账凭证口径脱钩；同月重录不受影响
    let (old_month, _) = get_opening_balances(conn)?;
    if let Some(old) = old_month {
        if old != month {
            let active_vouchers: i64 = conn.query_row(
                "SELECT COUNT(*) FROM vouchers WHERE status = 'active'",
                [],
                |r| r.get(0),
            )?;
            if active_vouchers > 0 {
                return Err(AppError::General(
                    "已存在记账凭证，不能变更启用月；如需调整请联系管理员清理凭证后重录".into(),
                ));
            }
        }
    }
    let now = Utc::now().to_rfc3339();
    // DELETE + INSERT 事务包裹，保证整体覆盖的原子性（&Connection 下用 unchecked_transaction）
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM opening_balances", [])?;
    for row in rows {
        tx.execute(
            "INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![month, row.account_code, row.debit_amount, row.credit_amount, now],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// 查询全部科目映射。
pub fn get_account_mappings(conn: &Connection) -> AppResult<Vec<AccountMapping>> {
    let mut stmt = conn.prepare(
        "SELECT id, scope, key, account_code, remark FROM account_mappings ORDER BY scope, key",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(AccountMapping {
                id: r.get(0)?,
                scope: r.get(1)?,
                key: r.get(2)?,
                account_code: r.get(3)?,
                remark: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 保存科目映射（同 scope+key 幂等覆盖）。
pub fn save_account_mapping(
    conn: &Connection,
    input: &AccountMappingInput,
) -> AppResult<AccountMapping> {
    if !["expense_type", "department"].contains(&input.scope.as_str()) {
        return Err(AppError::General(
            "映射 scope 只支持 expense_type / department".into(),
        ));
    }
    let category: String = conn.query_row(
        "SELECT category FROM gl_accounts WHERE code = ?1",
        params![input.account_code],
        |r| r.get(0),
    )?;
    if input.scope == "expense_type" && !["profit_loss", "cost"].contains(&category.as_str()) {
        return Err(AppError::General(format!(
            "科目 {} 不是费用类科目",
            input.account_code
        )));
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO account_mappings (scope, key, account_code, remark, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(scope, key) DO UPDATE SET account_code = ?3, remark = ?4, updated_at = ?5",
        params![
            input.scope,
            input.key,
            input.account_code,
            input.remark,
            now
        ],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM account_mappings WHERE scope = ?1 AND key = ?2",
        params![input.scope, input.key],
        |r| r.get(0),
    )?;
    Ok(AccountMapping {
        id,
        scope: input.scope.clone(),
        key: input.key.clone(),
        account_code: input.account_code.clone(),
        remark: input.remark.clone(),
    })
}

/// 删除科目映射，返回是否实际删除了行。
pub fn delete_account_mapping(conn: &Connection, id: i64) -> AppResult<bool> {
    Ok(conn.execute("DELETE FROM account_mappings WHERE id = ?1", params![id])? > 0)
}

/// 生成凭证：校验借贷平衡与科目存在后落库，编号按月递增（记-YYYYMM-NNN）。
/// 同源（source_type+source_id）已有 active 凭证时由部分唯一索引拒绝（错误包装为 AppError 返回，不 panic）。
pub fn insert_voucher(conn: &Connection, draft: &VoucherDraft) -> AppResult<Voucher> {
    let debit: f64 = draft.lines.iter().map(|l| l.debit_amount).sum();
    let credit: f64 = draft.lines.iter().map(|l| l.credit_amount).sum();
    if (debit - credit).abs() > 0.005 || debit <= 0.0 {
        return Err(AppError::General(format!(
            "凭证借贷不平衡（借 {debit:.2} / 贷 {credit:.2}），拒绝生成"
        )));
    }
    for line in &draft.lines {
        if line.debit_amount < 0.0 || line.credit_amount < 0.0 {
            return Err(AppError::General("凭证分录金额不能为负".into()));
        }
    }
    for line in &draft.lines {
        if conn
            .query_row(
                "SELECT 1 FROM gl_accounts WHERE code = ?1",
                params![line.account_code],
                |_| Ok(()),
            )
            .is_err()
        {
            return Err(AppError::General(format!(
                "科目 {} 不存在",
                line.account_code
            )));
        }
    }
    let voucher_no = next_voucher_no(conn, &draft.belong_month)?;
    let now = Utc::now().to_rfc3339();
    // vouchers 主表 + voucher_lines 分录写入需原子。独立调用（autocommit）时用 unchecked_transaction 包裹；
    // 若调用方已开启事务（如 lock_salary_results），BEGIN 无法嵌套，直接写入、由外层事务保证原子性。
    let id = if conn.is_autocommit() {
        let tx = conn.unchecked_transaction()?;
        let id = insert_voucher_rows(&tx, draft, &voucher_no, &now)?;
        tx.commit()?;
        id
    } else {
        insert_voucher_rows(conn, draft, &voucher_no, &now)?
    };
    get_voucher(conn, id)
}

fn insert_voucher_rows(
    conn: &Connection,
    draft: &VoucherDraft,
    voucher_no: &str,
    now: &str,
) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status, remark, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8, ?8)",
        params![voucher_no, draft.voucher_date, draft.belong_month, draft.source_type, draft.source_id, draft.lines.iter().map(|l| l.debit_amount).sum::<f64>(), draft.remark, now],
    )?;
    let id = conn.last_insert_rowid();
    for (i, line) in draft.lines.iter().enumerate() {
        conn.execute(
            "INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount, summary, line_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, line.account_code, line.debit_amount, line.credit_amount, line.summary, i as i64],
        )?;
    }
    Ok(id)
}

/// 按 id 读取凭证（含按 line_order 排序的分录列表）。
pub fn get_voucher(conn: &Connection, id: i64) -> AppResult<Voucher> {
    let mut voucher = conn.query_row(
        "SELECT id, voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status, remark
         FROM vouchers WHERE id = ?1",
        params![id],
        |r| {
            Ok(Voucher {
                id: r.get(0)?, voucher_no: r.get(1)?, voucher_date: r.get(2)?,
                belong_month: r.get(3)?, source_type: r.get(4)?, source_id: r.get(5)?,
                total_amount: r.get(6)?, status: r.get(7)?, remark: r.get(8)?,
                lines: Vec::new(),
            })
        },
    )?;
    let mut stmt = conn.prepare(
        "SELECT id, account_code, debit_amount, credit_amount, summary, line_order
         FROM voucher_lines WHERE voucher_id = ?1 ORDER BY line_order",
    )?;
    voucher.lines = stmt
        .query_map(params![id], |r| {
            Ok(VoucherLine {
                id: r.get(0)?,
                account_code: r.get(1)?,
                debit_amount: r.get(2)?,
                credit_amount: r.get(3)?,
                summary: r.get(4)?,
                line_order: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(voucher)
}

/// 生成下一凭证号：记-YYYYMM-NNN（NNN 为该月已有凭证数 + 1）。
/// 并发安全由上层 Mutex<Connection> 串行化保证。
pub fn next_voucher_no(conn: &Connection, month: &str) -> AppResult<String> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers WHERE belong_month = ?1",
        params![month],
        |r| r.get(0),
    )?;
    Ok(format!("记-{}-{:03}", month.replace('-', ""), n + 1))
}

/// 作废某业务源下全部 active 凭证，返回作废条数。作废后同源可重新生成。
/// 测试环境（invoice.rs/db.rs 的最小 schema）可能没有 vouchers 表，此时视为无凭证可作废。
pub fn void_vouchers_for_source(
    conn: &Connection,
    source_type: &str,
    source_id: i64,
) -> AppResult<usize> {
    if !table_exists(conn, "vouchers") {
        return Ok(0);
    }
    Ok(conn.execute(
        "UPDATE vouchers SET status = 'void', updated_at = ?3 WHERE source_type = ?1 AND source_id = ?2 AND status = 'active'",
        params![source_type, source_id, Utc::now().to_rfc3339()],
    )?)
}

/// 年末结转：12 月月结时调用。凭证① 各损益科目余额 → 3103；凭证② 3103 余额 → 3104。
/// source_type='period_close'，凭证① source_id=YYYYMM*10+1、凭证② YYYYMM*10+2（避开部分唯一索引）。
/// 非损益凭证、全年损益净额为零或非 12 月返回 0；该月已有 active period_close 凭证时幂等返回 0。
/// 报表口径统一排除 period_close（见 compute_balances / profit_loss_amounts / build_cash_flow_statement）。
pub fn generate_period_close_vouchers(conn: &Connection, month: &str) -> AppResult<usize> {
    if !month.ends_with("-12") {
        return Ok(0);
    }
    // 幂等：该月已有 active period_close 凭证则跳过（避免撞部分唯一索引报错）
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers
         WHERE source_type='period_close' AND belong_month=?1 AND status='active'",
        params![month],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Ok(0);
    }
    // 全年（启用月~12 月）各损益科目净额（排除已有 period_close）
    let open_month = opening_month(conn).unwrap_or_else(|| format!("{}-01", &month[..4]));
    let mut stmt = conn.prepare(
        "SELECT vl.account_code, SUM(vl.debit_amount - vl.credit_amount)
         FROM voucher_lines vl JOIN vouchers v ON v.id = vl.voucher_id
         WHERE v.belong_month >= ?1 AND v.belong_month <= ?2 AND v.status = 'active'
           AND v.source_type != 'period_close'
           AND vl.account_code IN (SELECT code FROM gl_accounts WHERE category = 'profit_loss')
         GROUP BY vl.account_code",
    )?;
    let nets = stmt
        .query_map(params![open_month, month], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    let month_id: i64 = month.replace('-', "").parse().unwrap_or(0);
    let mut created = 0;
    let net_total: f64 = nets.iter().map(|(_, v)| v).sum();
    if !nets.is_empty() {
        // 凭证①：净额 = 借-贷。收入类为贷方余额（净额<0）→ 借记结平；
        // 费用类为借方余额（净额>0）→ 贷记结平；差额记 3103（贷为利润、借为亏损）
        let mut lines: Vec<VoucherLineDraft> = Vec::new();
        for (code, net) in &nets {
            if *net < 0.0 {
                lines.push(VoucherLineDraft {
                    account_code: code.clone(),
                    debit_amount: -*net,
                    credit_amount: 0.0,
                    summary: Some(format!("{month} 年末结转损益（{code}）")),
                });
            } else if *net > 0.0 {
                lines.push(VoucherLineDraft {
                    account_code: code.clone(),
                    debit_amount: 0.0,
                    credit_amount: *net,
                    summary: Some(format!("{month} 年末结转损益（{code}）")),
                });
            }
        }
        if net_total <= 0.0 {
            lines.push(VoucherLineDraft {
                account_code: "3103".into(),
                debit_amount: 0.0,
                credit_amount: -net_total,
                summary: Some(format!("{month} 结转本年利润")),
            });
        } else {
            lines.push(VoucherLineDraft {
                account_code: "3103".into(),
                debit_amount: net_total,
                credit_amount: 0.0,
                summary: Some(format!("{month} 结转本年亏损")),
            });
        }
        insert_voucher(
            conn,
            &VoucherDraft {
                belong_month: month.to_string(),
                voucher_date: format!("{month}-31"),
                source_type: "period_close".into(),
                source_id: month_id * 10 + 1,
                remark: Some(format!("{month} 年末损益结转")),
                lines,
            },
        )?;
        created += 1;
    }
    if net_total.abs() >= 0.005 {
        // 凭证②：3103 余额 → 3104（净利润时净额<0、3103 为贷方余额 → 借 3103 / 贷 3104；亏损反向）
        let (debit_code, credit_code) = if net_total <= 0.0 {
            ("3103", "3104")
        } else {
            ("3104", "3103")
        };
        let amount = net_total.abs();
        insert_voucher(
            conn,
            &VoucherDraft {
                belong_month: month.to_string(),
                voucher_date: format!("{month}-31"),
                source_type: "period_close".into(),
                source_id: month_id * 10 + 2,
                remark: Some(format!("{month} 本年利润结转未分配利润")),
                lines: vec![
                    VoucherLineDraft {
                        account_code: debit_code.into(),
                        debit_amount: amount,
                        credit_amount: 0.0,
                        summary: Some(format!("{month} 结转未分配利润")),
                    },
                    VoucherLineDraft {
                        account_code: credit_code.into(),
                        debit_amount: 0.0,
                        credit_amount: amount,
                        summary: Some(format!("{month} 结转未分配利润")),
                    },
                ],
            },
        )?;
        created += 1;
    }
    Ok(created)
}

/// 反月结联动：作废该月全部 period_close 凭证（按 belong_month + source_type 批量，覆盖两张）。
pub fn void_period_close_vouchers(conn: &Connection, month: &str) -> AppResult<usize> {
    let n = conn.execute(
        "UPDATE vouchers SET status='void', updated_at=?2
         WHERE source_type='period_close' AND belong_month=?1 AND status='active'",
        params![month, Utc::now().to_rfc3339()],
    )?;
    Ok(n)
}

pub fn generate_salary_accrual_vouchers(conn: &Connection, month: &str) -> AppResult<usize> {
    let mut stmt = conn.prepare(
        "SELECT id, name, department, gross_salary, attendance_deduction, other_deduction,
                social_security_personal, housing_fund_personal, tax_amount,
                social_security_employer, housing_fund_employer
         FROM salary_monthly_results
         WHERE salary_month = ?1 AND locked = 1 AND status != 'void'",
    )?;
    let rows = stmt
        .query_map(params![month], |r| {
            Ok((
                r.get::<_, i64>(0)?,            // id
                r.get::<_, Option<String>>(1)?, // name
                r.get::<_, Option<String>>(2)?, // department
                r.get::<_, f64>(3)?,            // gross
                r.get::<_, f64>(4)?,            // attendance
                r.get::<_, f64>(5)?,            // other
                r.get::<_, f64>(6)?,            // personal social security
                r.get::<_, f64>(7)?,            // personal housing fund
                r.get::<_, f64>(8)?,            // tax
                r.get::<_, f64>(9)?,            // employer social security
                r.get::<_, f64>(10)?,           // employer housing fund
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    let mut n = 0;
    for (
        id,
        name,
        department,
        gross,
        attendance,
        other,
        ss_personal,
        hf_personal,
        tax,
        ss_employer,
        hf_employer,
    ) in rows
    {
        let amount = (gross - attendance - other).max(0.0);
        let employer = ss_employer + hf_employer;
        if amount <= 0.0 && employer <= 0.0 {
            continue;
        }
        // 已有 active 凭证则跳过（幂等）
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM vouchers WHERE source_type='salary_accrual' AND source_id=?1 AND status='active'",
            params![id], |r| r.get(0))?;
        if exists > 0 {
            continue;
        }
        let dept_account =
            mapping_account(conn, "department", department.as_deref().unwrap_or(""))?;
        let emp = name.unwrap_or_else(|| "未知员工".into());
        let voucher_date = format!("{month}-28"); // 计提日固定 28 日，避开 31 天差异
                                                  // 全额成本口径：借 dept = 应发净额 + 单位社保公积金；贷 2211 同额
        let cost_amount = amount + employer;
        // 代扣腿：借 2211 = 个人社保公积金 + 个税；贷 2241 / 贷 2221
        let withholding_ss = ss_personal + hf_personal;
        let mut lines = vec![
            VoucherLineDraft {
                account_code: dept_account.clone(),
                debit_amount: cost_amount,
                credit_amount: 0.0,
                summary: Some(format!("{month} 工资费用（{emp}）")),
            },
            VoucherLineDraft {
                account_code: "2211".into(),
                debit_amount: 0.0,
                credit_amount: cost_amount,
                summary: Some(format!("{month} 应付职工薪酬（{emp}）")),
            },
        ];
        if withholding_ss + tax > 0.005 {
            lines.push(VoucherLineDraft {
                account_code: "2211".into(),
                debit_amount: withholding_ss + tax,
                credit_amount: 0.0,
                summary: Some(format!("{month} 代扣款项（{emp}）")),
            });
            if withholding_ss > 0.005 {
                lines.push(VoucherLineDraft {
                    account_code: "2241".into(),
                    debit_amount: 0.0,
                    credit_amount: withholding_ss,
                    summary: Some(format!("{month} 代扣社保公积金（{emp}）")),
                });
            }
            if tax > 0.005 {
                lines.push(VoucherLineDraft {
                    account_code: "2221".into(),
                    debit_amount: 0.0,
                    credit_amount: tax,
                    summary: Some(format!("{month} 代扣个税（{emp}）")),
                });
            }
        }
        insert_voucher(
            conn,
            &VoucherDraft {
                belong_month: month.to_string(),
                voucher_date,
                source_type: "salary_accrual".into(),
                source_id: id,
                remark: Some(format!("{month} 工资计提（{emp}）")),
                lines,
            },
        )?;
        n += 1;
    }
    Ok(n)
}

fn mapping_account(conn: &Connection, scope: &str, key: &str) -> AppResult<String> {
    let mapped: Option<String> = conn
        .query_row(
            "SELECT account_code FROM account_mappings WHERE scope = ?1 AND key = ?2",
            params![scope, key],
            |r| r.get(0),
        )
        .unwrap_or(None);
    Ok(mapped.unwrap_or_else(|| "6602".into()))
}

pub fn void_salary_accrual_vouchers(conn: &Connection, month: &str) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE vouchers SET status='void', updated_at=?2
         WHERE source_type='salary_accrual' AND status='active'
           AND source_id IN (SELECT id FROM salary_monthly_results WHERE salary_month = ?1)",
        params![month, Utc::now().to_rfc3339()],
    )?)
}

/// 生成付款凭证：批次已标记 paid 后调用。salary：借 2211/贷 1002；reimbursement：借 2241/贷 1002。
pub fn generate_payment_voucher(conn: &Connection, batch_id: i64) -> AppResult<Voucher> {
    let (batch_no, belong_month, batch_type, payment_date, total, status): (
        String,
        String,
        String,
        Option<String>,
        f64,
        String,
    ) = conn.query_row(
        "SELECT batch_no, belong_month, batch_type, payment_date, total_amount, status
         FROM payment_batches WHERE id = ?1",
        params![batch_id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        },
    )?;
    if status != "paid" {
        return Err(AppError::General(format!(
            "批次 {batch_no} 未标记已付款，不能生成付款凭证"
        )));
    }
    let (source_type, debit_account, remark) = match batch_type.as_str() {
        "salary" => ("salary_payment", "2211", "工资代发"),
        "reimbursement" => ("reimbursement_payment", "2241", "报销付款"),
        other => return Err(AppError::General(format!("未知批次类型 {other}"))),
    };
    let date = payment_date.unwrap_or_else(|| format!("{belong_month}-28"));
    insert_voucher(
        conn,
        &VoucherDraft {
            belong_month: belong_month.clone(),
            voucher_date: date,
            source_type: source_type.into(),
            source_id: batch_id,
            remark: Some(format!("{remark}（{batch_no}）")),
            lines: vec![
                VoucherLineDraft {
                    account_code: debit_account.into(),
                    debit_amount: total,
                    credit_amount: 0.0,
                    summary: Some(format!("{remark}（{batch_no}）")),
                },
                VoucherLineDraft {
                    account_code: "1002".into(),
                    debit_amount: 0.0,
                    credit_amount: total,
                    summary: Some(format!("{batch_no} 银行支出")),
                },
            ],
        },
    )
}

/// 作废某付款批次对应的全部付款凭证（salary_payment / reimbursement_payment），返回作废条数。
pub fn void_payment_voucher(conn: &Connection, batch_id: i64) -> AppResult<usize> {
    let n1 = void_vouchers_for_source(conn, "salary_payment", batch_id)?;
    let n2 = void_vouchers_for_source(conn, "reimbursement_payment", batch_id)?;
    Ok(n1 + n2)
}

/// 未匹配银行流水手工指定科目生成凭证（bank_manual）。
/// 流水必须 unmatched 且未忽略；支出流水：借所选科目/贷 1002；收入流水：借 1002/贷所选科目。
pub fn create_bank_manual_voucher(
    conn: &Connection,
    transaction_id: i64,
    account_code: &str,
    summary: Option<String>,
) -> AppResult<Voucher> {
    let (belong_month, transaction_date, income, expense, status, ignore_reason): (
        String,
        String,
        f64,
        f64,
        String,
        Option<String>,
    ) = conn.query_row(
        "SELECT belong_month, transaction_date, income_amount, expense_amount, status, ignore_reason FROM bank_transactions WHERE id = ?1",
        params![transaction_id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        },
    )?;
    if status != "unmatched" || ignore_reason.is_some() {
        return Err(AppError::General(
            "只有未匹配且未忽略的流水才能生成凭证".into(),
        ));
    }
    // 已有 active bank_manual 凭证时先拦截，避免触发部分唯一索引裸 UNIQUE 报错
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers WHERE source_type='bank_manual' AND source_id=?1 AND status='active'",
        params![transaction_id],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Err(AppError::General(
            "该流水已生成入账凭证，不能重复生成".into(),
        ));
    }
    db::ensure_month_open(conn, &belong_month)?;
    let (amount, lines) = if expense > 0.0 {
        (
            expense,
            vec![
                VoucherLineDraft {
                    account_code: account_code.into(),
                    debit_amount: expense,
                    credit_amount: 0.0,
                    summary: summary.clone(),
                },
                VoucherLineDraft {
                    account_code: "1002".into(),
                    debit_amount: 0.0,
                    credit_amount: expense,
                    summary,
                },
            ],
        )
    } else {
        (
            income,
            vec![
                VoucherLineDraft {
                    account_code: "1002".into(),
                    debit_amount: income,
                    credit_amount: 0.0,
                    summary: summary.clone(),
                },
                VoucherLineDraft {
                    account_code: account_code.into(),
                    debit_amount: 0.0,
                    credit_amount: income,
                    summary,
                },
            ],
        )
    };
    if amount <= 0.0 {
        return Err(AppError::General(
            "流水收入支出金额均为 0，不能生成凭证".into(),
        ));
    }
    insert_voucher(
        conn,
        &VoucherDraft {
            belong_month,
            voucher_date: transaction_date,
            source_type: "bank_manual".into(),
            source_id: transaction_id,
            remark: Some("银行流水入账".into()),
            lines,
        },
    )
}

/// 报销计提凭证借方行：按报销单关联发票的费用类型映射生成借方行，税额进 2221，贷方 2241 汇总。
fn invoice_expense_lines(
    conn: &Connection,
    claim_id: i64,
    month: &str,
) -> AppResult<Vec<VoucherLineDraft>> {
    let mut stmt = conn.prepare(
        "SELECT i.amount, i.tax_amount, i.expense_type_code
         FROM invoices i JOIN reimbursement_claim_invoices rc ON rc.invoice_id = i.id
         WHERE rc.claim_id = ?1 AND i.status = 'normal'",
    )?;
    let rows = stmt
        .query_map(params![claim_id], |r| {
            Ok((
                r.get::<_, f64>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut expense_map: std::collections::BTreeMap<String, f64> = Default::default();
    let mut tax_total = 0.0;
    for (amount, tax, type_code) in &rows {
        let account = match type_code {
            Some(code) if !code.is_empty() => mapping_account(conn, "expense_type", code)?,
            _ => "6602".into(),
        };
        *expense_map.entry(account).or_insert(0.0) += amount;
        tax_total += tax;
    }
    let total: f64 = rows.iter().map(|(a, t, _)| a + t).sum();
    let mut lines = Vec::new();
    for (account, amt) in expense_map {
        if amt > 0.0 {
            lines.push(VoucherLineDraft {
                account_code: account,
                debit_amount: amt,
                credit_amount: 0.0,
                summary: Some(format!("{month} 报销费用")),
            });
        }
    }
    if tax_total > 0.0 {
        lines.push(VoucherLineDraft {
            account_code: "2221".into(),
            debit_amount: tax_total,
            credit_amount: 0.0,
            summary: Some(format!("{month} 报销进项税额")),
        });
    }
    lines.push(VoucherLineDraft {
        account_code: "2241".into(),
        debit_amount: 0.0,
        credit_amount: total,
        summary: Some(format!("{month} 应付报销款")),
    });
    Ok(lines)
}

/// 报销审批计提凭证：claim 状态为 approved（及之后状态，即非 draft/submitted/void/rejected）时生成。
/// 已有 active 凭证时跳过（幂等）。
pub fn generate_reimbursement_accrual_voucher(
    conn: &Connection,
    claim_id: i64,
) -> AppResult<Option<Voucher>> {
    if !table_exists(conn, "vouchers") {
        return Ok(None);
    }
    let (claim_no, belong_month, total, status): (String, String, f64, String) = conn.query_row(
        "SELECT claim_no, belong_month, total_amount, status FROM reimbursement_claims WHERE id = ?1",
        params![claim_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    // 实际状态机为 draft/submitted/approved/rejected/void；仅 approved 生成计提
    // （brief 的"approved 及之后状态"在本状态机中即 approved 本身）
    if status != "approved" {
        return Ok(None);
    }
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers WHERE source_type='reimbursement_accrual' AND source_id=?1 AND status='active'",
        params![claim_id],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Ok(None);
    }
    let lines = invoice_expense_lines(conn, claim_id, &belong_month)?;
    let mut lines = lines;
    let has_invoices = lines.len() > 1; // invoice_expense_lines 有发票时至少含 2241 贷方 + 借方行
    if !has_invoices {
        // 无发票报销（如现金支出）：总额整体进 6602，贷 2241
        lines = vec![
            VoucherLineDraft {
                account_code: "6602".into(),
                debit_amount: total,
                credit_amount: 0.0,
                summary: Some(format!("{belong_month} 报销费用（无票部分）")),
            },
            VoucherLineDraft {
                account_code: "2241".into(),
                debit_amount: 0.0,
                credit_amount: total,
                summary: Some(format!("{belong_month} 应付报销款")),
            },
        ];
    } else {
        // 借贷差额兜底：发票合计与贷方不平时差额进 6602 配平
        let debit: f64 = lines.iter().map(|l| l.debit_amount).sum();
        let credit: f64 = lines.iter().map(|l| l.credit_amount).sum();
        if (credit - debit).abs() > 0.005 {
            lines.insert(
                0,
                VoucherLineDraft {
                    account_code: "6602".into(),
                    debit_amount: credit - debit,
                    credit_amount: 0.0,
                    summary: Some(format!("{belong_month} 报销费用（无票部分）")),
                },
            );
        }
    }
    let voucher = insert_voucher(
        conn,
        &VoucherDraft {
            belong_month: belong_month.clone(),
            voucher_date: format!("{belong_month}-28"),
            source_type: "reimbursement_accrual".into(),
            source_id: claim_id,
            remark: Some(format!("报销计提（{claim_no}）")),
            lines,
        },
    )?;
    Ok(Some(voucher))
}

/// 发票费用入账凭证：仅当发票 normal 且未挂任何报销单时生成（防止与报销计提重复入账）。
pub fn maybe_generate_invoice_expense_voucher(
    conn: &Connection,
    invoice_id: i64,
) -> AppResult<Option<Voucher>> {
    let (belong_month, amount, tax, total, type_code, status): (
        Option<String>,
        f64,
        f64,
        f64,
        Option<String>,
        String,
    ) = conn.query_row(
        "SELECT belong_month, amount, tax_amount, total_amount, expense_type_code, status FROM invoices WHERE id = ?1",
        params![invoice_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    )?;
    if status != "normal" {
        return Ok(None);
    }
    if !table_exists(conn, "vouchers") {
        return Ok(None);
    }
    // 借方（费用+税额）为 0 或非正时无法构成平衡凭证（如历史数据缺 amount/tax），跳过
    if amount + tax <= 0.0 {
        return Ok(None);
    }
    // amount+tax 与 total 不一致的历史/异常数据跳过入账：否则借(费用+税)≠贷(total)
    // 会导致 insert_voucher 借贷不平衡 Err，进而阻断 insert_invoice/update_invoice 保存
    if (amount + tax - total).abs() > 0.005 {
        return Ok(None);
    }
    let belong_month = match belong_month {
        Some(m) if !m.trim().is_empty() => m,
        _ => return Ok(None),
    };
    // 仅统计挂到"会生成计提凭证"的报销单（approved）上的关联：
    // 挂在 draft/submitted/rejected/void 报销单上的发票仍需单独入账，作废/反审批时由此得到补偿。
    let linked: i64 = conn.query_row(
        "SELECT COUNT(*) FROM reimbursement_claim_invoices rc
         JOIN reimbursement_claims c ON c.id = rc.claim_id
         WHERE rc.invoice_id = ?1 AND c.status = 'approved'",
        params![invoice_id],
        |r| r.get(0),
    )?;
    if linked > 0 {
        return Ok(None);
    }
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers WHERE source_type='invoice_expense' AND source_id=?1 AND status='active'",
        params![invoice_id],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Ok(None);
    }
    let account = match &type_code {
        Some(code) if !code.is_empty() => mapping_account(conn, "expense_type", code)?,
        _ => "6602".into(),
    };
    let mut lines = vec![VoucherLineDraft {
        account_code: account,
        debit_amount: amount,
        credit_amount: 0.0,
        summary: Some(format!("{belong_month} 费用（无报销关联发票）")),
    }];
    if tax > 0.0 {
        lines.push(VoucherLineDraft {
            account_code: "2221".into(),
            debit_amount: tax,
            credit_amount: 0.0,
            summary: Some("进项税额".into()),
        });
    }
    lines.push(VoucherLineDraft {
        account_code: "2241".into(),
        debit_amount: 0.0,
        credit_amount: total,
        summary: Some(format!("{belong_month} 应付费用")),
    });
    let voucher = insert_voucher(
        conn,
        &VoucherDraft {
            belong_month: belong_month.clone(),
            voucher_date: format!("{belong_month}-28"),
            source_type: "invoice_expense".into(),
            source_id: invoice_id,
            remark: Some("发票费用入账".into()),
            lines,
        },
    )?;
    Ok(Some(voucher))
}

/// 作废某报销单的全部计提凭证，返回作废条数。
pub fn void_reimbursement_accrual_voucher(conn: &Connection, claim_id: i64) -> AppResult<usize> {
    void_vouchers_for_source(conn, "reimbursement_accrual", claim_id)
}

/// 作废某发票的费用入账凭证，返回作废条数。
pub fn void_invoice_expense_voucher(conn: &Connection, invoice_id: i64) -> AppResult<usize> {
    void_vouchers_for_source(conn, "invoice_expense", invoice_id)
}

/// 测试环境（invoice.rs/db.rs 的最小 schema）可能没有 vouchers 表，此时跳过凭证生成。
pub(crate) fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name = ?1",
        params![table],
        |_| Ok(()),
    )
    .is_ok()
}

/// 按条件查询凭证列表（月份/来源类型/状态均可选），含分录，按凭证号排序。
pub fn get_vouchers(conn: &Connection, q: &VoucherQuery) -> AppResult<Vec<Voucher>> {
    let mut sql = String::from("SELECT id FROM vouchers WHERE 1=1");
    let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(m) = &q.month {
        sql.push_str(&format!(" AND belong_month = ?{}", args.len() + 1));
        args.push(Box::new(m.clone()));
    }
    if let Some(s) = &q.source_type {
        sql.push_str(&format!(" AND source_type = ?{}", args.len() + 1));
        args.push(Box::new(s.clone()));
    }
    if let Some(s) = &q.status {
        sql.push_str(&format!(" AND status = ?{}", args.len() + 1));
        args.push(Box::new(s.clone()));
    }
    sql.push_str(" ORDER BY voucher_no");
    let ids: Vec<i64> = {
        let mut stmt = conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            args.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    ids.iter().map(|id| get_voucher(conn, *id)).collect()
}

// ==================== 报表计算引擎（Task 10） ====================

/// 科目余额中间结构：opening/opening_raw 按科目方向取正负号（借方科目借正，贷方科目贷正）。
struct AccountBalance {
    code: String,
    name: String,
    category: String,
    direction: String,
    /// 月初余额 = 启用期初 + 启用月至本月前一月凭证净额（跨月查看报表时滚入）
    opening: f64,
    /// 年初（启用月期初）余额 = 仅 opening_balances，不滚入凭证净额
    opening_raw: f64,
    period_debit: f64,
    period_credit: f64,
}

impl AccountBalance {
    /// 期末余额（按科目方向为正）。ending = opening + (借-贷) × 方向系数。
    fn ending(&self) -> f64 {
        self.opening
            + (self.period_debit - self.period_credit)
                * if self.direction == "debit" { 1.0 } else { -1.0 }
    }
}

/// 期初启用月：opening_balances 的 MIN(month)。无期初数据时 None（所有报表 enabled=false）。
fn opening_month(conn: &Connection) -> Option<String> {
    conn.query_row("SELECT MIN(month) FROM opening_balances", [], |r| r.get(0))
        .unwrap_or(None)
}

/// month 是否不早于启用月。
fn month_enabled(conn: &Connection, month: &str) -> bool {
    match opening_month(conn) {
        Some(m) => month >= m.as_str(),
        None => false,
    }
}

/// 计算 month 各科目的 期初/本期借贷/期末。
/// - opening：启用月 <= month 时 = 启用月期初余额（按科目方向正负号）
///   + 启用月至 month 前一月的 active 凭证累计净发生额（借-贷 × 方向系数）；
///   跨月查看报表时此前月份的凭证必须滚入期初，否则资产负债表无法平衡
/// - period_debit/credit：当月 active 凭证分录按科目合计
fn compute_balances(conn: &Connection, month: &str) -> AppResult<Vec<AccountBalance>> {
    let mut stmt =
        conn.prepare("SELECT code, name, category, direction FROM gl_accounts ORDER BY code")?;
    let mut balances: Vec<AccountBalance> = stmt
        .query_map([], |r| {
            Ok(AccountBalance {
                code: r.get(0)?,
                name: r.get(1)?,
                category: r.get(2)?,
                direction: r.get(3)?,
                opening: 0.0,
                opening_raw: 0.0,
                period_debit: 0.0,
                period_credit: 0.0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    if let Some(open_month) = opening_month(conn) {
        if month >= open_month.as_str() {
            // 启用月期初（按方向正负号：借方科目取借方金额，贷方科目取贷方金额为正）
            let mut stmt = conn.prepare(
                "SELECT account_code, debit_amount, credit_amount FROM opening_balances",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            })?;
            for row in rows {
                let (code, debit, credit) = row?;
                if let Some(b) = balances.iter_mut().find(|b| b.code == code) {
                    let raw = if b.direction == "debit" {
                        debit
                    } else {
                        credit
                    };
                    b.opening = raw;
                    b.opening_raw = raw;
                }
            }
            drop(stmt);
            // 启用月至 month 前一月的累计净发生额滚入期初
            let mut stmt = conn.prepare(
                "SELECT vl.account_code, SUM(vl.debit_amount), SUM(vl.credit_amount)
                 FROM voucher_lines vl JOIN vouchers v ON v.id = vl.voucher_id
                 WHERE v.belong_month >= ?1 AND v.belong_month < ?2 AND v.status = 'active'
                   AND v.source_type != 'period_close'
                 GROUP BY vl.account_code",
            )?;
            let rows = stmt.query_map(params![open_month, month], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            })?;
            for row in rows {
                let (code, debit, credit) = row?;
                if let Some(b) = balances.iter_mut().find(|b| b.code == code) {
                    let net = (debit - credit) * if b.direction == "debit" { 1.0 } else { -1.0 };
                    b.opening += net;
                }
            }
        }
    }
    // 当月发生额
    let mut stmt = conn.prepare(
        "SELECT vl.account_code, SUM(vl.debit_amount), SUM(vl.credit_amount)
         FROM voucher_lines vl JOIN vouchers v ON v.id = vl.voucher_id
         WHERE v.belong_month = ?1 AND v.status = 'active'
           AND v.source_type != 'period_close'
         GROUP BY vl.account_code",
    )?;
    let rows = stmt.query_map(params![month], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, f64>(1)?,
            r.get::<_, f64>(2)?,
        ))
    })?;
    for row in rows {
        let (code, debit, credit) = row?;
        if let Some(b) = balances.iter_mut().find(|b| b.code == code) {
            b.period_debit = debit;
            b.period_credit = credit;
        }
    }
    Ok(balances)
}

/// 科目余额表（试算平衡）：区间 [from_month, to_month] 每科目期初/本期发生/期末（借贷双侧）。
/// 与三大报表不同：包含全部凭证（含 period_close），反映真实账面。
pub fn build_trial_balance(
    conn: &Connection,
    from_month: &str,
    to_month: &str,
) -> AppResult<TrialBalanceReport> {
    let mut report = TrialBalanceReport {
        from_month: from_month.to_string(),
        to_month: to_month.to_string(),
        enabled: false,
        rows: Vec::new(),
        balanced: false,
    };
    let Some(open_month) = opening_month(conn) else {
        return Ok(report);
    };
    if from_month < open_month.as_str() {
        return Ok(report);
    }
    report.enabled = true;

    let mut stmt =
        conn.prepare("SELECT code, name, category, direction FROM gl_accounts ORDER BY code")?;
    let mut rows: Vec<TrialBalanceRow> = stmt
        .query_map([], |r| {
            Ok(TrialBalanceRow {
                code: r.get(0)?,
                name: r.get(1)?,
                category: r.get(2)?,
                direction: r.get(3)?,
                opening_debit: 0.0,
                opening_credit: 0.0,
                period_debit: 0.0,
                period_credit: 0.0,
                ending_debit: 0.0,
                ending_credit: 0.0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    // 期初（带符号，借方为正）：启用月期初 + [启用月, from_month) 凭证净额（含 period_close）
    let mut opening_signed: HashMap<String, f64> = HashMap::new();
    let mut stmt =
        conn.prepare("SELECT account_code, debit_amount, credit_amount FROM opening_balances")?;
    let obs = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    for row in &rows {
        if let Some((_, debit, credit)) = obs.iter().find(|(c, _, _)| *c == row.code) {
            *opening_signed.entry(row.code.clone()).or_insert(0.0) += debit - credit;
        }
    }
    let mut stmt = conn.prepare(
        "SELECT vl.account_code, SUM(vl.debit_amount - vl.credit_amount)
         FROM voucher_lines vl JOIN vouchers v ON v.id = vl.voucher_id
         WHERE v.belong_month >= ?1 AND v.belong_month < ?2 AND v.status = 'active'
         GROUP BY vl.account_code",
    )?;
    let nets = stmt
        .query_map(params![open_month, from_month], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    for row in &rows {
        if let Some((_, net)) = nets.iter().find(|(c, _)| *c == row.code) {
            *opening_signed.entry(row.code.clone()).or_insert(0.0) += net;
        }
    }

    // 区间发生额（含 period_close）
    let mut stmt = conn.prepare(
        "SELECT vl.account_code, SUM(vl.debit_amount), SUM(vl.credit_amount)
         FROM voucher_lines vl JOIN vouchers v ON v.id = vl.voucher_id
         WHERE v.belong_month >= ?1 AND v.belong_month <= ?2 AND v.status = 'active'
         GROUP BY vl.account_code",
    )?;
    let periods = stmt
        .query_map(params![from_month, to_month], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut total_debit = 0.0;
    let mut total_credit = 0.0;
    for row in rows.iter_mut() {
        let opening = *opening_signed.get(&row.code).unwrap_or(&0.0);
        if let Some((_, debit, credit)) = periods.iter().find(|(c, _, _)| *c == row.code) {
            row.period_debit = *debit;
            row.period_credit = *credit;
        }
        // 期末（带符号）= 期初 + (借-贷)（借贷记账法下净额方向即科目余额方向，无需 direction 系数）
        let ending = opening + row.period_debit - row.period_credit;
        // 分侧：正数记借方、负数记贷方（绝对值）
        if opening >= 0.0 {
            row.opening_debit = opening;
        } else {
            row.opening_credit = -opening;
        }
        if ending >= 0.0 {
            row.ending_debit = ending;
        } else {
            row.ending_credit = -ending;
        }
        total_debit += row.ending_debit;
        total_credit += row.ending_credit;
    }
    // 仅保留有数据（期初或发生非零）的科目
    report.rows = rows
        .into_iter()
        .filter(|r| {
            r.opening_debit.abs() > 0.005
                || r.opening_credit.abs() > 0.005
                || r.period_debit.abs() > 0.005
                || r.period_credit.abs() > 0.005
        })
        .collect();
    report.balanced = (total_debit - total_credit).abs() < 0.005;
    Ok(report)
}

/// 利润表标准行顺序（code, 中文名, 科目方向）。金额按发生额展示：
/// 收入类（贷方向）取 贷-借，费用类（借方向）取 借-贷（正数展示）。
const INCOME_STATEMENT_ROWS: &[(&str, &str, &str)] = &[
    ("6001", "主营业务收入", "credit"),
    ("6051", "其他业务收入", "credit"),
    ("6111", "投资收益", "credit"),
    ("6401", "主营业务成本", "debit"),
    ("6402", "其他业务成本", "debit"),
    ("6403", "税金及附加", "debit"),
    ("6601", "销售费用", "debit"),
    ("6602", "管理费用", "debit"),
    ("6603", "财务费用", "debit"),
    ("6301", "营业外收入", "credit"),
    ("6711", "营业外支出", "debit"),
    ("6801", "所得税费用", "debit"),
];

/// 计算损益类科目的发生额：返回 (当月, 年初至当月累计)，金额为正数展示值
/// （收入类取 贷-借，费用类取 借-贷）。month < 启用月时返回 None；
/// 累计范围为 启用月..=month 的 active 凭证。
fn profit_loss_amounts(
    conn: &Connection,
    month: &str,
) -> AppResult<Option<HashMap<String, (f64, f64)>>> {
    let Some(open_month) = opening_month(conn) else {
        return Ok(None);
    };
    if month < open_month.as_str() {
        return Ok(None);
    }
    // 方向系数：贷方科目 贷-借（收入为正），借方科目 借-贷（费用为正）
    let sign = |code: &str| -> f64 {
        INCOME_STATEMENT_ROWS
            .iter()
            .find(|(c, _, _)| *c == code)
            .map(|(_, _, dir)| if *dir == "credit" { 1.0 } else { -1.0 })
            .unwrap_or(1.0)
    };
    // 当月发生额（贷-借）
    let mut month_map: HashMap<String, f64> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT vl.account_code, SUM(vl.credit_amount - vl.debit_amount)
         FROM voucher_lines vl JOIN vouchers v ON v.id = vl.voucher_id
         WHERE v.belong_month = ?1 AND v.status = 'active'
           AND v.source_type != 'period_close'
           AND vl.account_code IN (SELECT code FROM gl_accounts WHERE category = 'profit_loss')
         GROUP BY vl.account_code",
    )?;
    let rows = stmt.query_map(params![month], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
    })?;
    for row in rows {
        let (code, amt) = row?;
        month_map.insert(code, amt);
    }
    drop(stmt);
    // 年初（启用月）至当月累计（贷-借）
    let mut year_map: HashMap<String, f64> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT vl.account_code, SUM(vl.credit_amount - vl.debit_amount)
         FROM voucher_lines vl JOIN vouchers v ON v.id = vl.voucher_id
         WHERE v.belong_month >= ?1 AND v.belong_month <= ?2 AND v.status = 'active'
           AND v.source_type != 'period_close'
           AND vl.account_code IN (SELECT code FROM gl_accounts WHERE category = 'profit_loss')
         GROUP BY vl.account_code",
    )?;
    let rows = stmt.query_map(params![open_month, month], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
    })?;
    for row in rows {
        let (code, amt) = row?;
        year_map.insert(code, amt);
    }
    let mut result = HashMap::new();
    for (code, _, _) in INCOME_STATEMENT_ROWS {
        let s = sign(code);
        let m = *month_map.get(*code).unwrap_or(&0.0) * s;
        let y = *year_map.get(*code).unwrap_or(&0.0) * s;
        result.insert((*code).to_string(), (m, y));
    }
    // 兜底行：非 12 标准编码的 profit_loss 科目（如自定义 660299）求和（贷-借），
    // 避免自定义损益科目金额从报表消失导致资产负债表不平
    let is_standard = |code: &str| INCOME_STATEMENT_ROWS.iter().any(|(c, _, _)| *c == code);
    let mut other_m = 0.0;
    let mut other_y = 0.0;
    for (code, amt) in &month_map {
        if !is_standard(code) {
            other_m += amt;
        }
    }
    for (code, amt) in &year_map {
        if !is_standard(code) {
            other_y += amt;
        }
    }
    result.insert("other_pl".to_string(), (other_m, other_y));
    Ok(Some(result))
}

/// 净利润 = 收入类合计 − 费用类合计 + 其他未列示损益（贷方向科目为正、借方向科目为负）。
/// 返回 (当月, 年初至当月累计)；未启用返回 (0, 0)。
fn net_profit(conn: &Connection, month: &str) -> AppResult<(f64, f64)> {
    match profit_loss_amounts(conn, month)? {
        Some(map) => {
            let mut m = 0.0;
            let mut y = 0.0;
            for (code, _, dir) in INCOME_STATEMENT_ROWS {
                let (mv, yv) = map.get(*code).copied().unwrap_or((0.0, 0.0));
                let s = if *dir == "credit" { 1.0 } else { -1.0 };
                m += mv * s;
                y += yv * s;
            }
            // 其他未列示损益已是净额（贷-借），直接计入
            let (om, oy) = map.get("other_pl").copied().unwrap_or((0.0, 0.0));
            m += om;
            y += oy;
            Ok((m, y))
        }
        None => Ok((0.0, 0.0)),
    }
}

/// 现金等价物科目（货币资金合并行 / 编制现金流量表时视为"现金"）。
const CASH_ACCOUNTS: &[&str] = &["1001", "1002", "1012"];

/// 资产负债表：asset 科目期末 → 资产行（1001+1002+1012 合并"货币资金"，其余一科目一行）；
/// cost 类科目期末余额合计 → "成本类科目"行（借方，资产行末尾兜底，避免成本类科目金额消失致报表不平）；
/// 未分配利润 = 3104 期末 + 启用月至当月累计净利润；comparative = 年初（启用月期初口径，不滚入凭证）；
/// balanced 兜底校验 |资产合计 - 负债权益合计| < 0.005。
pub fn build_balance_sheet(conn: &Connection, month: &str) -> AppResult<BalanceSheet> {
    // 上年同期：上年 12 月（年末时点数）；上年早于启用月或年份解析失败时 prior 全 0
    let prior_dec = prior_year_month(month, "12").unwrap_or_default();
    let prior_enabled = !prior_dec.is_empty() && month_enabled(conn, &prior_dec);
    let prior_balances = if prior_enabled {
        compute_balances(conn, &prior_dec)?
    } else {
        Vec::new()
    };
    let prior_ending = |code: &str| -> f64 {
        prior_balances
            .iter()
            .find(|b| b.code == code)
            .map(|b| b.ending())
            .unwrap_or(0.0)
    };
    let prior_year_profit = if prior_enabled {
        net_profit(conn, &prior_dec)?.1
    } else {
        0.0
    };
    let mut sheet = BalanceSheet {
        month: month.to_string(),
        enabled: month_enabled(conn, month),
        asset_rows: Vec::new(),
        liability_equity_rows: Vec::new(),
        asset_total: 0.0,
        liability_equity_total: 0.0,
        balanced: false,
        has_prior_year: prior_enabled,
    };
    if !sheet.enabled {
        return Ok(sheet);
    }
    let balances = compute_balances(conn, month)?;
    let (_, year_profit) = net_profit(conn, month)?;
    // 资产端：货币资金合并行 + 其余一科目一行
    let mut monetary_row = ReportRow {
        key: "monetary".into(),
        label: "货币资金".into(),
        current: 0.0,
        comparative: 0.0,
        prior_year: 0.0,
    };
    for b in balances.iter().filter(|b| b.category == "asset") {
        let ending = b.ending();
        if CASH_ACCOUNTS.contains(&b.code.as_str()) {
            monetary_row.current += ending;
            monetary_row.comparative += b.opening_raw;
            monetary_row.prior_year += prior_ending(&b.code);
        } else {
            sheet.asset_rows.push(ReportRow {
                key: b.code.clone(),
                label: b.name.clone(),
                current: ending,
                comparative: b.opening_raw,
                prior_year: prior_ending(&b.code),
            });
        }
    }
    sheet.asset_rows.insert(0, monetary_row);
    // cost 类科目期末余额合计（借方）：资产行末尾兜底行，保证资产负债表平衡
    let cost_total: f64 = balances
        .iter()
        .filter(|b| b.category == "cost")
        .map(|b| b.ending())
        .sum();
    // 年初口径：cost 类科目 Σ opening_raw（启用月期初余额合计）
    let cost_opening_total: f64 = balances
        .iter()
        .filter(|b| b.category == "cost")
        .map(|b| b.opening_raw)
        .sum();
    let cost_prior_total: f64 = prior_balances
        .iter()
        .filter(|b| b.category == "cost")
        .map(|b| b.ending())
        .sum();
    sheet.asset_rows.push(ReportRow {
        key: "cost_accounts".into(),
        label: "成本类科目".into(),
        current: cost_total,
        comparative: cost_opening_total,
        prior_year: cost_prior_total,
    });
    sheet.asset_total = sheet.asset_rows.iter().map(|r| r.current).sum();
    // 负债与权益端：3104 替换为"未分配利润" = 3104 期末 + 启用月至当月累计净利润
    for b in balances
        .iter()
        .filter(|b| b.category == "liability" || b.category == "equity")
    {
        let (ending, comp, prior) = if b.code == "3104" {
            (
                b.ending() + year_profit,
                b.opening_raw,
                prior_ending("3104") + prior_year_profit,
            )
        } else {
            (b.ending(), b.opening_raw, prior_ending(&b.code))
        };
        sheet.liability_equity_rows.push(ReportRow {
            key: if b.code == "3104" {
                "undistributed".into()
            } else {
                b.code.clone()
            },
            label: if b.code == "3104" {
                "未分配利润".into()
            } else {
                b.name.clone()
            },
            current: ending,
            comparative: comp,
            prior_year: prior,
        });
    }
    sheet.liability_equity_total = sheet.liability_equity_rows.iter().map(|r| r.current).sum();
    sheet.balanced = (sheet.asset_total - sheet.liability_equity_total).abs() < 0.005;
    Ok(sheet)
}

/// 上年同月：month = "YYYY-MM" → "YYYY-1-MM"；月份格式非法返回 None。
fn prior_year_month(month: &str, mm: &str) -> Option<String> {
    let y: i64 = month.get(0..4)?.parse().ok()?;
    Some(format!("{}-{}", y - 1, mm))
}

/// 利润表：profit_loss 科目当月与年初至当月累计发生额（贷-借）映射到标准行；
/// 非 12 标准编码的损益科目（自定义科目）合并进"其他未列示损益"兜底行；
/// 营业利润 = 收入类 − 成本费用类（营业外收支与所得税之前，不含兜底行）；
/// 利润总额 = 营业利润 + 营业外收支净额；净利润 = 利润总额 + 其他未列示损益 − 所得税。
pub fn build_income_statement(conn: &Connection, month: &str) -> AppResult<IncomeStatement> {
    let amounts = profit_loss_amounts(conn, month)?;
    let enabled = amounts.is_some();
    // 上年同期（上年同月的年初至上月同月累计分量）
    let prior_amounts = match prior_year_month(month, month.get(5..).unwrap_or("01")) {
        Some(pm) => profit_loss_amounts(conn, &pm)?,
        None => None,
    };
    let has_prior_year = prior_amounts.is_some();
    let prior_of = |code: &str| -> f64 {
        prior_amounts
            .as_ref()
            .map(|m| m.get(code).map(|(_, y)| *y).unwrap_or(0.0))
            .unwrap_or(0.0)
    };
    let mut rows = Vec::new();
    for (code, label, _) in INCOME_STATEMENT_ROWS {
        let (m, y) = match &amounts {
            Some(map) => map.get(*code).copied().unwrap_or((0.0, 0.0)),
            None => (0.0, 0.0),
        };
        rows.push(ReportRow {
            key: (*code).to_string(),
            label: (*label).to_string(),
            current: m,
            comparative: y,
            prior_year: prior_of(code),
        });
    }
    let get = |code: &str| -> (f64, f64) {
        rows.iter()
            .find(|r| r.key == code)
            .map(|r| (r.current, r.comparative))
            .unwrap_or((0.0, 0.0))
    };
    // 营业利润 = 收入(6001/6051/6111) − 成本费用(6401/6402/6403/6601/6602/6603)
    let op_m = get("6001").0 + get("6051").0 + get("6111").0
        - get("6401").0
        - get("6402").0
        - get("6403").0
        - get("6601").0
        - get("6602").0
        - get("6603").0;
    let op_y = get("6001").1 + get("6051").1 + get("6111").1
        - get("6401").1
        - get("6402").1
        - get("6403").1
        - get("6601").1
        - get("6602").1
        - get("6603").1;
    let op_p = prior_of("6001") + prior_of("6051") + prior_of("6111")
        - prior_of("6401")
        - prior_of("6402")
        - prior_of("6403")
        - prior_of("6601")
        - prior_of("6602")
        - prior_of("6603");
    let (non_in_m, non_in_y) = get("6301");
    let (non_out_m, non_out_y) = get("6711");
    let (tax_m, tax_y) = get("6801");
    // other_pl 不在 INCOME_STATEMENT_ROWS，从 amounts 直接取
    let (other_m, other_y) = match &amounts {
        Some(map) => map.get("other_pl").copied().unwrap_or((0.0, 0.0)),
        None => (0.0, 0.0),
    };
    let other_p = prior_of("other_pl");
    let total_m = op_m + non_in_m - non_out_m;
    let total_y = op_y + non_in_y - non_out_y;
    let total_p = op_p + prior_of("6301") - prior_of("6711");
    // 净利润 = 利润总额 + 其他未列示损益 − 所得税费用（营业利润/利润总额不包含它）
    let net_m = total_m + other_m - tax_m;
    let net_y = total_y + other_y - tax_y;
    let net_p = total_p + other_p - prior_of("6801");
    rows.push(ReportRow {
        key: "other_pl".into(),
        label: "其他未列示损益".into(),
        current: other_m,
        comparative: other_y,
        prior_year: other_p,
    });
    rows.push(ReportRow {
        key: "operating_profit".into(),
        label: "营业利润".into(),
        current: op_m,
        comparative: op_y,
        prior_year: op_p,
    });
    rows.push(ReportRow {
        key: "total_profit".into(),
        label: "利润总额".into(),
        current: total_m,
        comparative: total_y,
        prior_year: total_p,
    });
    rows.push(ReportRow {
        key: "net_profit".into(),
        label: "净利润".into(),
        current: net_m,
        comparative: net_y,
        prior_year: net_p,
    });
    Ok(IncomeStatement {
        month: month.to_string(),
        year_cumulative: enabled,
        rows,
        net_profit_month: net_m,
        net_profit_year: net_y,
        has_prior_year,
    })
}

/// 现金流量表（直接法）：当月含现金科目行且含对方行的 active 凭证，
/// 现金净流入（借-贷）按对方行金额占对方行总额比例分摊到对方科目的 cash_flow_category；
/// 对方科目 category=none 的部分归"其他"行并记入 unclassified 明细。
pub fn build_cash_flow_statement(conn: &Connection, month: &str) -> AppResult<CashFlowStatement> {
    let vouchers = get_vouchers(
        conn,
        &VoucherQuery {
            month: Some(month.to_string()),
            source_type: None,
            status: Some("active".into()),
        },
    )?;
    let cfc_map = cash_flow_categories(conn)?;
    let (cash, unclassified) = sum_cash_flow(&vouchers, &cfc_map);
    // 上年同期：上年 1 月~上年 12 月区间凭证（排除 period_close）；同期不重复提示未分类明细
    let prior_month = prior_year_month(month, "01");
    let prior_sums = match &prior_month {
        Some(pm) if month_enabled(conn, pm) => {
            let year: i64 = month[..4].parse().unwrap();
            let to = format!("{}-12", year - 1);
            let prior_vouchers = get_vouchers_range(conn, pm, &to)?;
            sum_cash_flow(&prior_vouchers, &cfc_map).0
        }
        _ => CashFlowSums::default(),
    };
    let has_prior_year = prior_month
        .as_deref()
        .map(|pm| month_enabled(conn, pm))
        .unwrap_or(false);
    let rows = vec![
        ReportRow {
            key: "operating_inflow".into(),
            label: "经营活动现金流入".into(),
            current: cash.operating_in,
            comparative: 0.0,
            prior_year: prior_sums.operating_in,
        },
        ReportRow {
            key: "operating_outflow".into(),
            label: "经营活动现金流出".into(),
            current: cash.operating_out,
            comparative: 0.0,
            prior_year: prior_sums.operating_out,
        },
        ReportRow {
            key: "investing_inflow".into(),
            label: "投资活动现金流入".into(),
            current: cash.investing_in,
            comparative: 0.0,
            prior_year: prior_sums.investing_in,
        },
        ReportRow {
            key: "investing_outflow".into(),
            label: "投资活动现金流出".into(),
            current: cash.investing_out,
            comparative: 0.0,
            prior_year: prior_sums.investing_out,
        },
        ReportRow {
            key: "financing_inflow".into(),
            label: "筹资活动现金流入".into(),
            current: cash.financing_in,
            comparative: 0.0,
            prior_year: prior_sums.financing_in,
        },
        ReportRow {
            key: "financing_outflow".into(),
            label: "筹资活动现金流出".into(),
            current: cash.financing_out,
            comparative: 0.0,
            prior_year: prior_sums.financing_out,
        },
        ReportRow {
            key: "other".into(),
            label: "其他（未分类）".into(),
            current: cash.other,
            comparative: 0.0,
            prior_year: prior_sums.other,
        },
    ];
    let net_increase = cash.operating_in + cash.investing_in + cash.financing_in + cash.other
        - cash.operating_out
        - cash.investing_out
        - cash.financing_out;
    Ok(CashFlowStatement {
        month: month.to_string(),
        rows,
        net_increase,
        unclassified,
        has_prior_year,
    })
}

/// 现金流分摊汇总：凭证的现金净流入按对方行金额占比分摊到对方科目的现金流量分类。
/// 返回六行汇总与未分类明细（对方科目 category=none 的部分）。
fn sum_cash_flow(
    vouchers: &[Voucher],
    cfc_map: &HashMap<String, String>,
) -> (CashFlowSums, Vec<UnclassifiedCashItem>) {
    let mut cash = CashFlowSums::default();
    let mut unclassified: Vec<UnclassifiedCashItem> = Vec::new();
    for v in vouchers {
        // 年末结转凭证不参与现金流量表（口径统一排除 period_close）
        if v.source_type == "period_close" {
            continue;
        }
        // 现金行净流入（借方为正、贷方为负）；无现金行的凭证不参与
        let cash_net: f64 = v
            .lines
            .iter()
            .filter(|l| CASH_ACCOUNTS.contains(&l.account_code.as_str()))
            .map(|l| l.debit_amount - l.credit_amount)
            .sum();
        if cash_net.abs() < 0.005 {
            continue;
        }
        // 对方行（非现金科目行），金额取借+贷发生额绝对值合计作为分摊权重
        let counter_lines: Vec<&VoucherLine> = v
            .lines
            .iter()
            .filter(|l| !CASH_ACCOUNTS.contains(&l.account_code.as_str()))
            .collect();
        let counter_total: f64 = counter_lines
            .iter()
            .map(|l| l.debit_amount + l.credit_amount)
            .sum();
        if counter_lines.is_empty() || counter_total.abs() < 0.005 {
            // 凭证只有现金行（不应发生）：整笔归 unclassified 提示
            unclassified.push(UnclassifiedCashItem {
                voucher_no: v.voucher_no.clone(),
                summary: v.remark.clone(),
                amount: cash_net,
            });
            cash.other += cash_net;
            continue;
        }
        // 按对方行金额比例分摊现金净流入/流出
        for l in &counter_lines {
            let weight = (l.debit_amount + l.credit_amount) / counter_total;
            let amount = cash_net * weight;
            let cfc = cfc_map
                .get(l.account_code.as_str())
                .map(|s| s.as_str())
                .unwrap_or("none");
            match cfc {
                "operating" => {
                    if amount >= 0.0 {
                        cash.operating_in += amount
                    } else {
                        cash.operating_out += -amount
                    }
                }
                "investing" => {
                    if amount >= 0.0 {
                        cash.investing_in += amount
                    } else {
                        cash.investing_out += -amount
                    }
                }
                "financing" => {
                    if amount >= 0.0 {
                        cash.financing_in += amount
                    } else {
                        cash.financing_out += -amount
                    }
                }
                _ => {
                    cash.other += amount;
                    unclassified.push(UnclassifiedCashItem {
                        voucher_no: v.voucher_no.clone(),
                        summary: l.summary.clone(),
                        amount,
                    });
                }
            }
        }
    }
    (cash, unclassified)
}

/// 区间 [from_month, to_month] 的 active 凭证（排除 period_close），按凭证号排序，含分录。
fn get_vouchers_range(
    conn: &Connection,
    from_month: &str,
    to_month: &str,
) -> AppResult<Vec<Voucher>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM vouchers
         WHERE belong_month >= ?1 AND belong_month <= ?2 AND status = 'active'
           AND source_type != 'period_close'
         ORDER BY voucher_no",
    )?;
    let ids: Vec<i64> = stmt
        .query_map(params![from_month, to_month], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.iter().map(|id| get_voucher(conn, *id)).collect()
}

/// 现金流量表六行汇总中间结构。
#[derive(Default)]
struct CashFlowSums {
    operating_in: f64,
    operating_out: f64,
    investing_in: f64,
    investing_out: f64,
    financing_in: f64,
    financing_out: f64,
    other: f64,
}

/// 科目编码 → cash_flow_category 映射。
fn cash_flow_categories(conn: &Connection) -> AppResult<HashMap<String, String>> {
    let mut stmt = conn.prepare("SELECT code, cash_flow_category FROM gl_accounts")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut map = HashMap::new();
    for row in rows {
        let (code, cfc) = row?;
        map.insert(code, cfc);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::Connection;

    /// Task 12：三大报表上年同期对比列（资产负债表=上年年末时点；利润表=上年 1 月~上年同月累计）
    #[test]
    fn test_reports_prior_year_columns() {
        let conn = setup();
        // 启用月 2024-01（期初全 0，保证平衡校验通过）
        conn.execute(
            "INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount)
             VALUES ('2024-01', '1001', 0.0, 0.0)",
            [],
        )
        .unwrap();
        // 2024-12 凭证：收入 6001 贷 1200
        conn.execute(
            "INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status)
             VALUES ('记-202412-001', '2024-12-05', '2024-12', 'bank_manual', 1, 1200.0, 'active')",
            [],
        )
        .unwrap();
        let v24: i64 = conn
            .query_row(
                "SELECT id FROM vouchers WHERE voucher_no='记-202412-001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '6001', 0.0, 1200.0)",
            [v24],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '1001', 1200.0, 0.0)",
            [v24],
        )
        .unwrap();
        // 2025-12 凭证：收入 6001 贷 800
        conn.execute(
            "INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status)
             VALUES ('记-202512-001', '2025-12-05', '2025-12', 'bank_manual', 2, 800.0, 'active')",
            [],
        )
        .unwrap();
        let v25: i64 = conn
            .query_row(
                "SELECT id FROM vouchers WHERE voucher_no='记-202512-001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '6001', 0.0, 800.0)",
            [v25],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '1001', 800.0, 0.0)",
            [v25],
        )
        .unwrap();

        let income = build_income_statement(&conn, "2025-12").unwrap();
        assert!(income.has_prior_year);
        let rev = income.rows.iter().find(|r| r.key == "6001").unwrap();
        assert_eq!(rev.prior_year, 1200.0); // 上年同期累计
                                            // 净利润 prior 分量同步重算
        let net = income.rows.iter().find(|r| r.key == "net_profit").unwrap();
        assert_eq!(net.prior_year, 1200.0);

        let bs = build_balance_sheet(&conn, "2025-12").unwrap();
        assert!(bs.has_prior_year);
        let cash_row = bs.asset_rows.iter().find(|r| r.key == "monetary").unwrap();
        assert_eq!(cash_row.prior_year, 1200.0); // 上年年末时点

        // 2026-12：上年=2025，prior 累计复用 profit_loss_amounts（启用月 2024-01 起）= 1200+800
        let income26 = build_income_statement(&conn, "2026-12").unwrap();
        assert!(income26.has_prior_year);
        let rev26 = income26.rows.iter().find(|r| r.key == "6001").unwrap();
        assert_eq!(rev26.prior_year, 2000.0);

        // 早于启用月的年份：2024-12 的上年=2023 < 启用月 2024-01 → 无同期列
        let income24 = build_income_statement(&conn, "2024-12").unwrap();
        assert!(!income24.has_prior_year);
        let rev24 = income24.rows.iter().find(|r| r.key == "6001").unwrap();
        assert_eq!(rev24.prior_year, 0.0);

        // 现金流量表同期：2025-12 的上年区间 2024 全年，6001 分类 operating → 经营流入 1200
        let cf = build_cash_flow_statement(&conn, "2025-12").unwrap();
        assert!(cf.has_prior_year);
        let op_in = cf
            .rows
            .iter()
            .find(|r| r.key == "operating_inflow")
            .unwrap();
        assert_eq!(op_in.prior_year, 1200.0);
        // 当月口径不受影响
        assert_eq!(op_in.current, 800.0);
    }

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::create_tables(&conn).unwrap();
        db::seed_gl_accounts(&conn).unwrap();
        conn.execute_batch(
            "INSERT OR IGNORE INTO invoice_expense_types (code, name, sort_order) VALUES
                ('office', '办公费', 1), ('travel', '差旅费', 2), ('other', '其他', 99);
            INSERT OR IGNORE INTO employees (id, employee_no, name, department, status, base_salary, created_at, updated_at)
            VALUES (1, 'E001', '张三', '销售部', 'active', 10000, '2026-08-01', '2026-08-01');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_account_crud() {
        let conn = setup();
        let accounts = get_accounts(&conn).unwrap();
        // Task 1 预置《小企业会计准则》62 个一级科目（brief 写 70 与实际交付不符，按 62 断言）
        assert!(accounts.len() >= 62);
        // 新增自定义科目
        let created = create_account(
            &conn,
            &GlAccountInput {
                code: "660201".into(),
                name: "管理费用—办公费".into(),
                category: "profit_loss".into(),
                direction: "debit".into(),
                cash_flow_category: Some("operating".into()),
                remark: None,
            },
        )
        .unwrap();
        assert_eq!(created.is_system, 0);
        // 编码重复报错
        assert!(create_account(
            &conn,
            &GlAccountInput {
                code: "660201".into(),
                name: "重复".into(),
                category: "profit_loss".into(),
                direction: "debit".into(),
                cash_flow_category: None,
                remark: None,
            }
        )
        .is_err());
        // 停用/启用
        assert!(set_account_active(&conn, "660201", false).unwrap());
        assert!(set_account_active(&conn, "660201", true).unwrap());
    }

    #[test]
    fn test_opening_balance_validation() {
        let conn = setup();
        let mut rows = vec![
            OpeningBalanceRow {
                account_code: "1002".into(),
                debit_amount: 100000.0,
                credit_amount: 0.0,
            },
            OpeningBalanceRow {
                account_code: "2001".into(),
                debit_amount: 0.0,
                credit_amount: 40000.0,
            },
            // 少 60000，不平
        ];
        let err = save_opening_balances(&conn, "2026-01", &rows);
        assert!(err.is_err());
        rows.push(OpeningBalanceRow {
            account_code: "3001".into(),
            debit_amount: 0.0,
            credit_amount: 60000.0,
        });
        save_opening_balances(&conn, "2026-01", &rows).unwrap();
        // 换月重录：清空旧月重新保存（取原 rows 前 2 条科目，金额改为平衡）
        let (month, loaded) = get_opening_balances(&conn).unwrap();
        assert_eq!(month.as_deref(), Some("2026-01"));
        assert_eq!(loaded.len(), 3);
        let rows2 = vec![
            OpeningBalanceRow {
                account_code: "1002".into(),
                debit_amount: 40000.0,
                credit_amount: 0.0,
            },
            OpeningBalanceRow {
                account_code: "2001".into(),
                debit_amount: 0.0,
                credit_amount: 40000.0,
            },
        ];
        save_opening_balances(&conn, "2026-02", &rows2).unwrap();
        let (month2, loaded2) = get_opening_balances(&conn).unwrap();
        assert_eq!(month2.as_deref(), Some("2026-02"));
        assert_eq!(loaded2.len(), 2);
        // 重复科目报错
        let dup = vec![
            OpeningBalanceRow {
                account_code: "1002".into(),
                debit_amount: 100.0,
                credit_amount: 0.0,
            },
            OpeningBalanceRow {
                account_code: "1002".into(),
                debit_amount: 100.0,
                credit_amount: 0.0,
            },
            OpeningBalanceRow {
                account_code: "2001".into(),
                debit_amount: 0.0,
                credit_amount: 200.0,
            },
        ];
        assert!(save_opening_balances(&conn, "2026-03", &dup).is_err());
        // 查重报错不应破坏已保存数据
        let (month3, loaded3) = get_opening_balances(&conn).unwrap();
        assert_eq!(month3.as_deref(), Some("2026-02"));
        assert_eq!(loaded3.len(), 2);
    }

    /// 启用月变更守卫：已有 active 记账凭证后变更启用月应报错；同月重录仍成功。
    #[test]
    fn test_opening_balance_month_change_guard() {
        let conn = setup();
        let rows = vec![
            OpeningBalanceRow {
                account_code: "1002".into(),
                debit_amount: 100000.0,
                credit_amount: 0.0,
            },
            OpeningBalanceRow {
                account_code: "2001".into(),
                debit_amount: 0.0,
                credit_amount: 40000.0,
            },
            OpeningBalanceRow {
                account_code: "3001".into(),
                debit_amount: 0.0,
                credit_amount: 60000.0,
            },
        ];
        // 2026-01 期初
        save_opening_balances(&conn, "2026-01", &rows).unwrap();
        // 生成一张 active 凭证
        insert_voucher(
            &conn,
            &manual_draft(
                "2026-01",
                1,
                vec![vline("6602", 500.0, 0.0), vline("1002", 0.0, 500.0)],
            ),
        )
        .unwrap();
        // 改存 2026-02 期初：已有 active 凭证，应拒绝（报表滚入窗口不可漂移）
        let err = save_opening_balances(&conn, "2026-02", &rows).unwrap_err();
        assert!(
            matches!(err, AppError::General(ref msg) if msg.contains("不能变更启用月")),
            "expected month-change guard error, got {:?}",
            err
        );
        // 原数据未被破坏
        let (month, loaded) = get_opening_balances(&conn).unwrap();
        assert_eq!(month.as_deref(), Some("2026-01"));
        assert_eq!(loaded.len(), 3);
        // 同月重录（金额调整后平衡）不受影响
        let rows_same_month = vec![
            OpeningBalanceRow {
                account_code: "1002".into(),
                debit_amount: 90000.0,
                credit_amount: 0.0,
            },
            OpeningBalanceRow {
                account_code: "2001".into(),
                debit_amount: 0.0,
                credit_amount: 40000.0,
            },
            OpeningBalanceRow {
                account_code: "3001".into(),
                debit_amount: 0.0,
                credit_amount: 50000.0,
            },
        ];
        save_opening_balances(&conn, "2026-01", &rows_same_month).unwrap();
        let (month2, loaded2) = get_opening_balances(&conn).unwrap();
        assert_eq!(month2.as_deref(), Some("2026-01"));
        assert_eq!(loaded2.len(), 3);
    }

    #[test]
    fn test_deactivate_account_with_voucher_lines() {
        let conn = setup();
        // 手工插入一条银行流水凭证（source_type=bank_manual）及分录，引用 1002 科目
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status, created_at, updated_at)
             VALUES ('BK-2026-0001', '2026-01-10', '2026-01', 'bank_manual', 1, 500.0, 'active', ?1, ?1)",
            params![now],
        )
        .unwrap();
        let vid: i64 = conn
            .query_row(
                "SELECT id FROM vouchers WHERE voucher_no = 'BK-2026-0001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount, summary, line_order)
             VALUES (?1, '1002', 500.0, 0.0, '测试分录', 1)",
            params![vid],
        )
        .unwrap();
        // active 凭证引用时停用报错，启用不受影响
        assert!(set_account_active(&conn, "1002", false).is_err());
        assert!(set_account_active(&conn, "1002", true).unwrap());
        // 凭证作废后不再阻塞停用
        conn.execute(
            "UPDATE vouchers SET status = 'void' WHERE id = ?1",
            params![vid],
        )
        .unwrap();
        assert!(set_account_active(&conn, "1002", false).unwrap());
    }

    #[test]
    fn test_voucher_core() {
        let conn = setup();
        let draft = VoucherDraft {
            belong_month: "2026-08".into(),
            voucher_date: "2026-08-31".into(),
            source_type: "bank_manual".into(),
            source_id: 1,
            remark: Some("手续费".into()),
            lines: vec![
                VoucherLineDraft {
                    account_code: "6603".into(),
                    debit_amount: 30.0,
                    credit_amount: 0.0,
                    summary: Some("手续费".into()),
                },
                VoucherLineDraft {
                    account_code: "1002".into(),
                    debit_amount: 0.0,
                    credit_amount: 30.0,
                    summary: Some("手续费".into()),
                },
            ],
        };
        let v = insert_voucher(&conn, &draft).unwrap();
        assert!(v.voucher_no.starts_with("记-202608-"));
        assert_eq!(v.total_amount, 30.0);
        // 不平衡拒绝
        let bad = VoucherDraft {
            lines: vec![VoucherLineDraft {
                account_code: "6603".into(),
                debit_amount: 30.0,
                credit_amount: 0.0,
                summary: None,
            }],
            ..draft.clone()
        };
        assert!(insert_voucher(&conn, &bad).is_err());
        // 同源重复拒绝（部分唯一索引）
        assert!(insert_voucher(&conn, &draft).is_err());
        // 作废后可重新生成，编号递增
        assert_eq!(
            void_vouchers_for_source(&conn, "bank_manual", 1).unwrap(),
            1
        );
        let v2 = insert_voucher(&conn, &draft).unwrap();
        assert_ne!(v.id, v2.id);
        assert_ne!(v.voucher_no, v2.voucher_no);
        // 查询
        let list = get_vouchers(
            &conn,
            &VoucherQuery {
                month: Some("2026-08".into()),
                source_type: None,
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].lines.len(), 2);
    }

    #[test]
    fn test_salary_accrual_voucher() {
        let conn = setup();
        // 插入 1 条 2026-08 工资结果：应发 10000，缺勤 500，其他扣款 100，
        // 个人社保 1000，个人公积金 800，个税 200，单位社保 2500，单位公积金 1200
        // （插入语句参考 db.rs setup_financial_db 现有工资测试）
        conn.execute(
            "INSERT INTO salary_monthly_results
                (salary_month, employee_no, name, department, gross_salary, net_salary,
                 social_security_personal, housing_fund_personal, attendance_deduction,
                 tax_amount, other_deduction, status, locked, created_at, updated_at,
                 social_security_employer, housing_fund_employer)
             VALUES ('2026-08', 'E001', '张三', '销售部', 10000, 7400, 1000, 800, 500, 200, 100,
                     'reviewed', 0, '2026-08-31', '2026-08-31', 2500, 1200)",
            [],
        )
        .unwrap();
        db::lock_salary_results(&conn, "2026-08").unwrap();
        let vouchers = get_vouchers(
            &conn,
            &VoucherQuery {
                month: Some("2026-08".into()),
                source_type: Some("salary_accrual".into()),
                status: None,
            },
        )
        .unwrap();
        assert_eq!(vouchers.len(), 1);
        let v = &vouchers[0];
        // 凭证分录：
        // 借 6602 = gross - attendance - other + employer_ss + employer_hf = 9400 + 3700 = 13100
        // 贷 2211 同额 13100
        // 借 2211 = personal_ss + personal_hf + tax = 1000 + 800 + 200 = 2000
        // 贷 2241 = personal_ss + personal_hf = 1800
        // 贷 2221 = tax = 200
        assert_eq!(v.lines.len(), 5);
        assert_eq!(v.total_amount, 15100.0); // 借方合计 = 13100 + 2000
        let find = |code: &str, debit: bool| {
            v.lines
                .iter()
                .find(|l| {
                    l.account_code == code
                        && if debit {
                            l.debit_amount > 0.0
                        } else {
                            l.credit_amount > 0.0
                        }
                })
                .unwrap_or_else(|| panic!("missing {} line", code))
        };
        let dept = find("6602", true);
        assert_eq!(dept.debit_amount, 13100.0);
        let payable_credit = find("2211", false);
        assert_eq!(payable_credit.credit_amount, 13100.0);
        let payable_debit = find("2211", true);
        assert_eq!(payable_debit.debit_amount, 2000.0);
        let withheld = find("2241", false);
        assert_eq!(withheld.credit_amount, 1800.0);
        let tax = find("2221", false);
        assert_eq!(tax.credit_amount, 200.0);
        // 借贷合计平衡
        let debit_sum: f64 = v.lines.iter().map(|l| l.debit_amount).sum();
        let credit_sum: f64 = v.lines.iter().map(|l| l.credit_amount).sum();
        assert!((debit_sum - credit_sum).abs() < 0.005);
        // 解锁后凭证作废
        db::unlock_salary_results(&conn, "2026-08").unwrap();
        let active = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("salary_accrual".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active.len(), 0);
    }

    #[test]
    fn test_salary_accrual_zero_withholding() {
        let conn = setup();
        // 个人社保/公积金/个税全 0，单位部分也为 0：只有借 dept / 贷 2211 两行
        conn.execute(
            "INSERT INTO salary_monthly_results
                (salary_month, employee_no, name, department, gross_salary, net_salary,
                 social_security_personal, housing_fund_personal, attendance_deduction,
                 tax_amount, other_deduction, status, locked, created_at, updated_at,
                 social_security_employer, housing_fund_employer)
             VALUES ('2026-09', 'E002', '李四', '销售部', 8000, 7900, 0, 0, 100, 0, 0,
                     'reviewed', 0, '2026-09-30', '2026-09-30', 0, 0)",
            [],
        )
        .unwrap();
        db::lock_salary_results(&conn, "2026-09").unwrap();
        let vouchers = get_vouchers(
            &conn,
            &VoucherQuery {
                month: Some("2026-09".into()),
                source_type: Some("salary_accrual".into()),
                status: None,
            },
        )
        .unwrap();
        assert_eq!(vouchers.len(), 1);
        let v = &vouchers[0];
        assert_eq!(v.lines.len(), 2);
        // 计提金额 = 8000 - 100 = 7900
        assert_eq!(v.total_amount, 7900.0);
        assert_eq!(v.lines[0].account_code, "6602");
        assert_eq!(v.lines[0].debit_amount, 7900.0);
        assert_eq!(v.lines[1].account_code, "2211");
        assert_eq!(v.lines[1].credit_amount, 7900.0);
        // 无 2241/2221 代扣行
        assert!(v.lines.iter().all(|l| l.account_code != "2241"));
        assert!(v.lines.iter().all(|l| l.account_code != "2221"));
    }

    #[test]
    fn test_unlock_salary_results_no_locked() {
        let conn = setup(); // 既有 helper：create_tables + seed_gl_accounts
        let err = db::unlock_salary_results(&conn, "2026-08").unwrap_err();
        assert!(err.to_string().contains("没有已锁定"));
    }

    #[test]
    fn test_payment_voucher() {
        let mut conn = db::tests::setup_financial_db();
        db::tests::fill_employee_bank_info(&conn);
        db::lock_salary_results(&conn, "2026-08").unwrap();
        // 2026-08 两个员工 net 合计 = 7800 + 6600 = 14400，只用 E001 构造 total=7800
        let detail = db::create_payment_batch(
            &mut conn,
            &crate::models::PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "salary".into(),
                source_ids: None,
                remark: None,
            },
        )
        .unwrap();
        assert_eq!(detail.batch.total_amount, 14400.0);
        db::mark_payment_batch_exported(&conn, detail.batch.id).unwrap();
        // 标记已付款
        db::mark_payment_batch_paid(
            &mut conn,
            &crate::models::PaymentBatchPaidInput {
                id: detail.batch.id,
                payment_date: "2026-08-31".into(),
            },
        )
        .unwrap();
        let vouchers = get_vouchers(
            &conn,
            &VoucherQuery {
                month: Some("2026-08".into()),
                source_type: Some("salary_payment".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(vouchers.len(), 1);
        let v = &vouchers[0];
        assert_eq!(v.total_amount, 14400.0);
        assert_eq!(v.voucher_date, "2026-08-31");
        assert_eq!(v.source_id, detail.batch.id);
        // 借 2211 14400，贷 1002 14400
        let debit_line = v.lines.iter().find(|l| l.debit_amount > 0.0).unwrap();
        assert_eq!(debit_line.account_code, "2211");
        assert_eq!(debit_line.debit_amount, 14400.0);
        let credit_line = v.lines.iter().find(|l| l.credit_amount > 0.0).unwrap();
        assert_eq!(credit_line.account_code, "1002");
        assert_eq!(credit_line.credit_amount, 14400.0);
        // 作废批次后凭证 void：
        db::void_payment_batch(
            &mut conn,
            &crate::models::PaymentBatchVoidInput {
                id: detail.batch.id,
                reason: "测试作废".into(),
            },
        )
        .unwrap();
        let active = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("salary_payment".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active.len(), 0);
        let voided = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("salary_payment".into()),
                status: Some("void".into()),
            },
        )
        .unwrap();
        assert_eq!(voided.len(), 1);
        // 批次状态与工资明细付款状态同步
        assert_eq!(voided[0].status, "void");
        let paid_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month='2026-08' AND payment_status='paid'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(paid_count, 2);
    }

    #[test]
    fn test_reimbursement_payment_voucher_accounts() {
        let mut conn = db::tests::setup_financial_db();
        db::tests::fill_employee_bank_info(&conn);
        // 报销批次：借 2241，贷 1002（claim 2 未付，500 元）
        let detail = db::create_payment_batch(
            &mut conn,
            &crate::models::PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "reimbursement".into(),
                source_ids: None,
                remark: None,
            },
        )
        .unwrap();
        assert_eq!(detail.batch.total_amount, 500.0);
        db::mark_payment_batch_exported(&conn, detail.batch.id).unwrap();
        db::mark_payment_batch_paid(
            &mut conn,
            &crate::models::PaymentBatchPaidInput {
                id: detail.batch.id,
                payment_date: "2026-08-31".into(),
            },
        )
        .unwrap();
        let vouchers = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("reimbursement_payment".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(vouchers.len(), 1);
        let debit_line = vouchers[0]
            .lines
            .iter()
            .find(|l| l.debit_amount > 0.0)
            .unwrap();
        assert_eq!(debit_line.account_code, "2241");
        let credit_line = vouchers[0]
            .lines
            .iter()
            .find(|l| l.credit_amount > 0.0)
            .unwrap();
        assert_eq!(credit_line.account_code, "1002");
        // 作废已付批次，报销单付款状态重置后重建 draft 批次，draft 状态直接生成凭证应被拒绝
        db::void_payment_batch(
            &mut conn,
            &crate::models::PaymentBatchVoidInput {
                id: detail.batch.id,
                reason: "重建测试".into(),
            },
        )
        .unwrap();
        conn.execute(
            "UPDATE reimbursement_claims SET payment_status='unpaid', payment_batch_id=NULL WHERE id=2",
            [],
        )
        .unwrap();
        let detail2 = db::create_payment_batch(
            &mut conn,
            &crate::models::PaymentBatchInput {
                belong_month: "2026-08".into(),
                batch_type: "reimbursement".into(),
                source_ids: None,
                remark: None,
            },
        )
        .unwrap();
        let err = generate_payment_voucher(&conn, detail2.batch.id).unwrap_err();
        assert!(err.to_string().contains("未标记已付款"), "got: {err:?}");
    }

    #[test]
    fn test_salary_accrual_department_mapping() {
        let conn = setup();
        save_account_mapping(
            &conn,
            &AccountMappingInput {
                scope: "department".into(),
                key: "销售部".into(),
                account_code: "6601".into(),
                remark: None,
            },
        )
        .unwrap();
        conn.execute(
            "INSERT INTO salary_monthly_results
                (salary_month, employee_no, name, department, gross_salary, net_salary,
                 social_security_personal, housing_fund_personal, attendance_deduction,
                 tax_amount, other_deduction, status, locked, created_at, updated_at)
             VALUES ('2026-08', 'E001', '张三', '销售部', 10000, 7400, 1000, 800, 500, 200, 100,
                     'reviewed', 0, '2026-08-31', '2026-08-31')",
            [],
        )
        .unwrap();
        db::lock_salary_results(&conn, "2026-08").unwrap();
        let vouchers = get_vouchers(
            &conn,
            &VoucherQuery {
                month: Some("2026-08".into()),
                source_type: Some("salary_accrual".into()),
                status: None,
            },
        )
        .unwrap();
        assert_eq!(vouchers.len(), 1);
        // 销售部映射到 6601（销售费用），非默认 6602
        let debit_line = vouchers[0]
            .lines
            .iter()
            .find(|l| l.debit_amount > 0.0)
            .unwrap();
        assert_eq!(debit_line.account_code, "6601");
    }

    #[test]
    fn test_voucher_rejects_negative_amount() {
        let conn = setup();
        let draft = VoucherDraft {
            belong_month: "2026-08".into(),
            voucher_date: "2026-08-31".into(),
            source_type: "bank_manual".into(),
            source_id: 1,
            remark: None,
            lines: vec![
                VoucherLineDraft {
                    account_code: "6603".into(),
                    debit_amount: 50.0,
                    credit_amount: 0.0,
                    summary: None,
                },
                VoucherLineDraft {
                    account_code: "6603".into(),
                    debit_amount: -20.0,
                    credit_amount: 0.0,
                    summary: None,
                },
                VoucherLineDraft {
                    account_code: "1002".into(),
                    debit_amount: 0.0,
                    credit_amount: 30.0,
                    summary: None,
                },
            ],
        };
        // 借贷合计平衡（30/30）但存在负数分录，应被拒绝
        let err = insert_voucher(&conn, &draft).unwrap_err();
        assert!(err.to_string().contains("不能为负"), "got: {err:?}");
    }

    #[test]
    fn test_reimbursement_and_invoice_vouchers() {
        let conn = setup();
        // 场景 1：无报销关联的发票入账
        // amount=100 tax=13 total=113 belong_month=2026-08，office 映射到 6602
        save_account_mapping(
            &conn,
            &AccountMappingInput {
                scope: "expense_type".into(),
                key: "office".into(),
                account_code: "6602".into(),
                remark: None,
            },
        )
        .unwrap();
        conn.execute(
            "INSERT INTO invoices (id, invoice_code, invoice_number, amount, tax_amount, total_amount,
                 expense_type_code, employee_id, belong_month, status, created_at, updated_at)
             VALUES (1, 'A', '001', 100.0, 13.0, 113.0, 'office', 1, '2026-08', 'normal',
                     '2026-08-10', '2026-08-10')",
            [],
        )
        .unwrap();
        assert!(maybe_generate_invoice_expense_voucher(&conn, 1)
            .unwrap()
            .is_some());
        let vouchers = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("invoice_expense".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(vouchers.len(), 1);
        assert_eq!(vouchers[0].source_id, 1);
        assert_eq!(vouchers[0].total_amount, 113.0);
        assert_eq!(vouchers[0].voucher_date, "2026-08-28");
        // 借 6602=100、借 2221=13、贷 2241=113
        let debit6602 = vouchers[0]
            .lines
            .iter()
            .find(|l| l.account_code == "6602")
            .unwrap();
        assert_eq!(debit6602.debit_amount, 100.0);
        let debit2221 = vouchers[0]
            .lines
            .iter()
            .find(|l| l.account_code == "2221")
            .unwrap();
        assert_eq!(debit2221.debit_amount, 13.0);
        let credit2241 = vouchers[0]
            .lines
            .iter()
            .find(|l| l.account_code == "2241")
            .unwrap();
        assert_eq!(credit2241.credit_amount, 113.0);

        // 场景 2：发票挂到报销单并审批通过
        // 先作废 invoice_expense，再生成 reimbursement_accrual
        conn.execute(
            "INSERT INTO reimbursement_claims
                (id, claim_no, employee_id, belong_month, title, total_amount, invoice_count,
                 status, payment_status, created_at, updated_at)
             VALUES (10, 'BX202608010', 1, '2026-08', '办公报销', 113.0, 1,
                     'approved', 'unpaid', '2026-08-12', '2026-08-12')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reimbursement_claim_invoices (claim_id, invoice_id, created_at)
             VALUES (10, 1, '2026-08-12')",
            [],
        )
        .unwrap();
        // 审批通过后：void 发票单独凭证 + 生成报销计提
        assert_eq!(void_invoice_expense_voucher(&conn, 1).unwrap(), 1);
        assert!(generate_reimbursement_accrual_voucher(&conn, 10)
            .unwrap()
            .is_some());
        // 报销计提凭证：借 6602=100、借 2221=13、贷 2241=113
        let accruals = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("reimbursement_accrual".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(accruals.len(), 1);
        assert_eq!(accruals[0].source_id, 10);
        assert_eq!(accruals[0].total_amount, 113.0);
        let debit6602 = accruals[0]
            .lines
            .iter()
            .find(|l| l.account_code == "6602")
            .unwrap();
        assert_eq!(debit6602.debit_amount, 100.0);
        let credit2241 = accruals[0]
            .lines
            .iter()
            .find(|l| l.account_code == "2241")
            .unwrap();
        assert_eq!(credit2241.credit_amount, 113.0);
        // 发票单独凭证已 void，防止重复计费
        let active_invoice_expense = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("invoice_expense".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active_invoice_expense.len(), 0);
        // 重复审批（幂等）：不新增凭证
        assert!(generate_reimbursement_accrual_voucher(&conn, 10)
            .unwrap()
            .is_none());

        // 场景 3+4：报销反审批（status 回 submitted）后凭证 void，发票恢复单独入账
        // 反审批 = void 计提凭证 + 更新报销单状态 + maybe 补发票单独凭证
        assert_eq!(void_reimbursement_accrual_voucher(&conn, 10).unwrap(), 1);
        conn.execute(
            "UPDATE reimbursement_claims SET status='submitted' WHERE id=10",
            [],
        )
        .unwrap();
        assert!(maybe_generate_invoice_expense_voucher(&conn, 1)
            .unwrap()
            .is_some());
        let active_after = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("invoice_expense".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active_after.len(), 1);
        // 报销计提凭证 active 数为 0
        let active_accruals = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("reimbursement_accrual".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active_accruals.len(), 0);
    }

    #[test]
    fn test_invoice_linked_to_claim_no_expense_voucher() {
        let conn = setup();
        // 已挂报销单的发票不生成单独费用凭证
        conn.execute(
            "INSERT INTO invoices (id, invoice_code, invoice_number, amount, tax_amount, total_amount,
                 expense_type_code, employee_id, belong_month, status, created_at, updated_at)
             VALUES (1, 'A', '001', 100.0, 13.0, 113.0, 'office', 1, '2026-08', 'normal',
                     '2026-08-10', '2026-08-10')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reimbursement_claims
                (id, claim_no, employee_id, belong_month, title, total_amount, invoice_count,
                 status, payment_status, created_at, updated_at)
             VALUES (10, 'BX202608010', 1, '2026-08', '办公报销', 113.0, 1,
                     'approved', 'unpaid', '2026-08-12', '2026-08-12')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reimbursement_claim_invoices (claim_id, invoice_id, created_at)
             VALUES (10, 1, '2026-08-12')",
            [],
        )
        .unwrap();
        // 已挂 approved 报销单的发票不生成单独费用凭证
        assert!(maybe_generate_invoice_expense_voucher(&conn, 1)
            .unwrap()
            .is_none());
        // void 发票也不生成
        conn.execute("UPDATE invoices SET status='void' WHERE id=1", [])
            .unwrap();
        assert!(maybe_generate_invoice_expense_voucher(&conn, 1)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_draft_claim_does_not_gate_invoice_or_accrual() {
        let conn = setup();
        conn.execute(
            "INSERT INTO invoices (id, invoice_code, invoice_number, amount, tax_amount, total_amount,
                 expense_type_code, employee_id, belong_month, status, created_at, updated_at)
             VALUES (1, 'A', '001', 100.0, 13.0, 113.0, 'office', 1, '2026-08', 'normal',
                     '2026-08-10', '2026-08-10')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reimbursement_claims
                (id, claim_no, employee_id, belong_month, title, total_amount, invoice_count,
                 status, payment_status, created_at, updated_at)
             VALUES (10, 'BX202608010', 1, '2026-08', '办公报销', 113.0, 1,
                     'draft', 'unpaid', '2026-08-12', '2026-08-12')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reimbursement_claim_invoices (claim_id, invoice_id, created_at)
             VALUES (10, 1, '2026-08-12')",
            [],
        )
        .unwrap();
        // draft 报销单不生成计提凭证
        assert!(generate_reimbursement_accrual_voucher(&conn, 10)
            .unwrap()
            .is_none());
        // 挂在 draft 报销单上的发票仍单独入账（防漏记）
        assert!(maybe_generate_invoice_expense_voucher(&conn, 1)
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_accrual_no_invoice_part_falls_to_6602() {
        let conn = setup();
        // 报销单无关联发票（如现金支出）：总额 200 全部进 6602
        conn.execute(
            "INSERT INTO reimbursement_claims
                (id, claim_no, employee_id, belong_month, title, total_amount, invoice_count,
                 status, payment_status, created_at, updated_at)
             VALUES (10, 'BX202608010', 1, '2026-08', '无票报销', 200.0, 0,
                     'approved', 'unpaid', '2026-08-12', '2026-08-12')",
            [],
        )
        .unwrap();
        assert!(generate_reimbursement_accrual_voucher(&conn, 10)
            .unwrap()
            .is_some());
        let accruals = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("reimbursement_accrual".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(accruals.len(), 1);
        let debit = accruals[0]
            .lines
            .iter()
            .find(|l| l.debit_amount > 0.0)
            .unwrap();
        assert_eq!(debit.account_code, "6602");
        assert_eq!(debit.debit_amount, 200.0);
        let credit = accruals[0]
            .lines
            .iter()
            .find(|l| l.credit_amount > 0.0)
            .unwrap();
        assert_eq!(credit.account_code, "2241");
    }

    #[test]
    fn test_db_hooks_reimbursement_invoice_vouchers() {
        let conn = setup();
        // 通过 db.rs 公开入口验证挂接（setup 已预置员工 E001）
        let invoice = db::insert_invoice(
            &conn,
            &crate::models::InvoiceInput {
                invoice_code: Some("A".into()),
                invoice_number: Some("100".into()),
                invoice_type: Some("普通发票".into()),
                issue_date: Some("2026-08-01".into()),
                check_code: None,
                amount: Some(100.0),
                tax_amount: Some(13.0),
                total_amount: Some(113.0),
                seller_name: Some("销售方".into()),
                seller_tax_id: None,
                buyer_name: None,
                buyer_tax_id: None,
                expense_type_code: Some("office".into()),
                employee_id: Some(1),
                belong_month: Some("2026-08".into()),
                remark: None,
                image_path: None,
                raw_ocr_json: None,
            },
            "/tmp/a.pdf",
            0,
        )
        .unwrap();
        // insert_invoice 挂接：未挂报销的发票自动生成 invoice_expense 凭证
        let vouchers = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("invoice_expense".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(vouchers.len(), 1);
        assert_eq!(vouchers[0].source_id, invoice.id);

        // 挂到报销单并审批通过
        let claim = db::save_reimbursement_claim(
            &conn,
            &crate::models::ReimbursementClaimInput {
                id: None,
                employee_id: Some(1),
                belong_month: "2026-08".into(),
                title: "办公报销".into(),
                invoice_ids: vec![invoice.id],
                status: Some("submitted".into()),
                payment_status: None,
                payment_date: None,
                remark: None,
            },
        )
        .unwrap();
        // 保存后（submitted 未审批）不应有报销计提；发票仍单独入账（防漏记），
        // 其单独凭证被补偿性 void+重建（金额不变，凭证号前移）
        let accruals = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("reimbursement_accrual".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(accruals.len(), 0);
        let active_expense = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("invoice_expense".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active_expense.len(), 1);

        // 审批通过：生成报销计提
        db::update_reimbursement_claim_status(&conn, claim.id, Some("approved".into()), None, None)
            .unwrap();
        let accruals = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("reimbursement_accrual".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(accruals.len(), 1);
        assert_eq!(accruals[0].source_id, claim.id);
        assert_eq!(accruals[0].total_amount, 113.0);

        // 反审批：报销计提 void，发票恢复单独入账
        db::update_reimbursement_claim_status(
            &conn,
            claim.id,
            Some("submitted".into()),
            None,
            None,
        )
        .unwrap();
        let active_accruals = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("reimbursement_accrual".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active_accruals.len(), 0);
        let active_expense = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("invoice_expense".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active_expense.len(), 1);
        assert_eq!(active_expense[0].source_id, invoice.id);

        // soft_delete_invoice：发票费用凭证 void
        db::soft_delete_invoice(&conn, invoice.id).unwrap();
        let active_expense = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("invoice_expense".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active_expense.len(), 0);

        // soft_delete_reimbursement_claim：报销计提凭证 void
        db::update_reimbursement_claim_status(&conn, claim.id, Some("approved".into()), None, None)
            .unwrap();
        db::soft_delete_reimbursement_claim(&conn, claim.id).unwrap();
        let active_accruals = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("reimbursement_accrual".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active_accruals.len(), 0);
    }

    #[test]
    fn test_update_invoice_regenerates_expense_voucher() {
        let conn = setup();
        conn.execute(
            "INSERT INTO invoices (id, invoice_code, invoice_number, amount, tax_amount, total_amount,
                 expense_type_code, employee_id, belong_month, status, created_at, updated_at)
             VALUES (1, 'A', '001', 100.0, 13.0, 113.0, 'office', 1, '2026-08', 'normal',
                     '2026-08-10', '2026-08-10')",
            [],
        )
        .unwrap();
        assert!(maybe_generate_invoice_expense_voucher(&conn, 1)
            .unwrap()
            .is_some());
        // 修改金额：100 -> 200（tax 不变，total 213）
        let updated = db::update_invoice(
            &conn,
            1,
            &crate::models::InvoiceInput {
                invoice_code: None,
                invoice_number: None,
                invoice_type: None,
                issue_date: None,
                check_code: None,
                amount: Some(200.0),
                tax_amount: None,
                total_amount: Some(213.0),
                seller_name: None,
                seller_tax_id: None,
                buyer_name: None,
                buyer_tax_id: None,
                expense_type_code: None,
                employee_id: None,
                belong_month: None,
                remark: None,
                image_path: None,
                raw_ocr_json: None,
            },
            None,
        )
        .unwrap();
        assert!(updated);
        let vouchers = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("invoice_expense".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        // void 旧凭证后重建：active 凭证金额为新值
        assert_eq!(vouchers.len(), 1);
        assert_eq!(vouchers[0].total_amount, 213.0);
        let voided = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("invoice_expense".into()),
                status: Some("void".into()),
            },
        )
        .unwrap();
        assert_eq!(voided.len(), 1);
        // 幂等：再次 update（金额未变）不重复生成
        db::update_invoice(
            &conn,
            1,
            &crate::models::InvoiceInput {
                invoice_code: None,
                invoice_number: None,
                invoice_type: None,
                issue_date: None,
                check_code: None,
                amount: Some(200.0),
                tax_amount: None,
                total_amount: Some(213.0),
                seller_name: None,
                seller_tax_id: None,
                buyer_name: None,
                buyer_tax_id: None,
                expense_type_code: None,
                employee_id: None,
                belong_month: None,
                remark: None,
                image_path: None,
                raw_ocr_json: None,
            },
            None,
        )
        .unwrap();
        let active = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("invoice_expense".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        // 幂等：金额未变的 update 走 void+重建，active 仍只有 1 张且金额为新值
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].total_amount, 213.0);
    }

    #[test]
    fn test_soft_delete_invoice_rebuilds_approved_claim_accrual() {
        let conn = setup();
        // approved 报销单挂 2 张发票（100+13、200+26）→ 计提 total=339
        conn.execute(
            "INSERT INTO invoices (id, invoice_code, invoice_number, amount, tax_amount, total_amount,
                 expense_type_code, employee_id, belong_month, status, created_at, updated_at)
             VALUES (1, 'A', '001', 100.0, 13.0, 113.0, 'office', 1, '2026-08', 'normal',
                     '2026-08-10', '2026-08-10'),
                   (2, 'A', '002', 200.0, 26.0, 226.0, 'office', 1, '2026-08', 'normal',
                     '2026-08-10', '2026-08-10')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reimbursement_claims
                (id, claim_no, employee_id, belong_month, title, total_amount, invoice_count,
                 status, payment_status, created_at, updated_at)
             VALUES (10, 'BX202608010', 1, '2026-08', '办公报销', 339.0, 2,
                     'approved', 'unpaid', '2026-08-12', '2026-08-12')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reimbursement_claim_invoices (claim_id, invoice_id, created_at)
             VALUES (10, 1, '2026-08-12'), (10, 2, '2026-08-12')",
            [],
        )
        .unwrap();
        assert!(generate_reimbursement_accrual_voucher(&conn, 10)
            .unwrap()
            .is_some());
        let accruals = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("reimbursement_accrual".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(accruals.len(), 1);
        assert_eq!(accruals[0].total_amount, 339.0);

        // soft_delete 发票 2（200+26）：计提凭证应 void + 按剩余发票重建（total=113）
        assert!(db::soft_delete_invoice(&conn, 2).unwrap());
        let active_accruals = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("reimbursement_accrual".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active_accruals.len(), 1);
        // 新计提凭证只含剩余发票：借 6602=100、借 2221=13、贷 2241=113
        // （按发票 amount+tax 汇总，不含已作废发票）
        assert!((active_accruals[0].total_amount - 113.0).abs() < 0.005);
        let credit2241 = active_accruals[0]
            .lines
            .iter()
            .find(|l| l.account_code == "2241")
            .unwrap();
        assert!((credit2241.credit_amount - 113.0).abs() < 0.005);
        // 旧计提凭证（339）已 void
        let voided = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("reimbursement_accrual".into()),
                status: Some("void".into()),
            },
        )
        .unwrap();
        assert_eq!(voided.len(), 1);
        assert_eq!(voided[0].total_amount, 339.0);
    }

    #[test]
    fn test_invoice_amount_mismatch_skips_voucher() {
        let conn = setup();
        // amount+tax=113 但 total=120：历史/异常数据，insert_invoice 不应因凭证不平衡而失败
        let invoice = db::insert_invoice(
            &conn,
            &crate::models::InvoiceInput {
                invoice_code: Some("A".into()),
                invoice_number: Some("900".into()),
                invoice_type: Some("普通发票".into()),
                issue_date: Some("2026-08-01".into()),
                check_code: None,
                amount: Some(100.0),
                tax_amount: Some(13.0),
                total_amount: Some(120.0),
                seller_name: Some("销售方".into()),
                seller_tax_id: None,
                buyer_name: None,
                buyer_tax_id: None,
                expense_type_code: Some("office".into()),
                employee_id: Some(1),
                belong_month: Some("2026-08".into()),
                remark: None,
                image_path: None,
                raw_ocr_json: None,
            },
            "/tmp/mismatch.pdf",
            0,
        )
        .unwrap();
        assert_eq!(invoice.amount, 100.0);
        // 不生成 invoice_expense 凭证（跳过入账而非阻断保存）
        let vouchers = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("invoice_expense".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(vouchers.len(), 0);
        // 直接调用同样返回 None（幂等）
        assert!(maybe_generate_invoice_expense_voucher(&conn, invoice.id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_bank_manual_voucher() {
        let conn = setup();
        // 插入 1 条 unmatched 支出流水 expense=30 belong_month=2026-08（参考现有流水测试构造）
        conn.execute(
            "INSERT INTO bank_transactions (transaction_date, belong_month, summary, income_amount, expense_amount, balance, status, ignore_reason)
             VALUES ('2026-08-05', '2026-08', '手续费', 0.0, 30.0, 9970.0, 'unmatched', NULL)",
            [],
        )
        .unwrap();
        let tx_id: i64 = conn
            .query_row("SELECT MAX(id) FROM bank_transactions", [], |r| r.get(0))
            .unwrap();
        let v = create_bank_manual_voucher(&conn, tx_id, "6603", Some("手续费".into())).unwrap();
        assert_eq!(v.total_amount, 30.0);
        assert_eq!(v.source_type, "bank_manual");
        assert_eq!(v.source_id, tx_id);
        assert_eq!(v.belong_month, "2026-08");
        assert_eq!(v.voucher_date, "2026-08-05");
        // 借 6603 贷 1002
        let debit_line = v.lines.iter().find(|l| l.debit_amount > 0.0).unwrap();
        assert_eq!(debit_line.account_code, "6603");
        assert_eq!(debit_line.debit_amount, 30.0);
        let credit_line = v.lines.iter().find(|l| l.credit_amount > 0.0).unwrap();
        assert_eq!(credit_line.account_code, "1002");
        assert_eq!(credit_line.credit_amount, 30.0);
        // 重复生成报错：入口拦截返回友好中文提示，而非裸 UNIQUE 索引错误（F2）
        let err = create_bank_manual_voucher(&conn, tx_id, "6603", None).unwrap_err();
        assert!(err.to_string().contains("不能重复生成"), "got: {err:?}");
        // 忽略流水后凭证 void：
        // db::ignore_bank_transaction(...) 后 active bank_manual 数为 0
        db::ignore_bank_transaction(
            &conn,
            &crate::models::BankTransactionIgnoreInput {
                transaction_id: tx_id,
                reason: "非业务流水".into(),
            },
        )
        .unwrap();
        let active = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("bank_manual".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active.len(), 0);

        // 收入流水：借 1002 / 贷所选科目（如利息收入进 6603 之外的 6011 不在预置科目，改用 6603 验证方向即可）
        conn.execute(
            "INSERT INTO bank_transactions (transaction_date, belong_month, summary, income_amount, expense_amount, balance, status, ignore_reason)
             VALUES ('2026-08-06', '2026-08', '利息收入', 12.0, 0.0, 9982.0, 'unmatched', NULL)",
            [],
        )
        .unwrap();
        let tx_id2: i64 = conn
            .query_row("SELECT MAX(id) FROM bank_transactions", [], |r| r.get(0))
            .unwrap();
        let v2 = create_bank_manual_voucher(&conn, tx_id2, "6603", None).unwrap();
        assert_eq!(v2.total_amount, 12.0);
        let debit2 = v2.lines.iter().find(|l| l.debit_amount > 0.0).unwrap();
        assert_eq!(debit2.account_code, "1002");
        let credit2 = v2.lines.iter().find(|l| l.credit_amount > 0.0).unwrap();
        assert_eq!(credit2.account_code, "6603");
        // 取消匹配也 void bank_manual（spec 3.3：取消匹配 → void bank_manual）
        db::cancel_bank_transaction_match(&conn, tx_id2).unwrap();
        let active2 = get_vouchers(
            &conn,
            &VoucherQuery {
                month: None,
                source_type: Some("bank_manual".into()),
                status: Some("active".into()),
            },
        )
        .unwrap();
        assert_eq!(active2.len(), 0);
        // 已忽略流水不能再生成凭证
        assert!(create_bank_manual_voucher(&conn, tx_id, "6603", None).is_err());
    }

    #[test]
    fn test_month_close_voucher_balance_check() {
        let conn = setup();
        // 无凭证时检查项通过
        let wb = db::get_month_close_workbench(&conn, "2026-08").unwrap();
        let item = wb
            .checks
            .iter()
            .find(|c| c.title.contains("记账凭证平衡"))
            .unwrap();
        assert_eq!(item.status, "ok");
        // 平衡凭证不触发
        let draft = VoucherDraft {
            belong_month: "2026-08".into(),
            voucher_date: "2026-08-31".into(),
            source_type: "bank_manual".into(),
            source_id: 1,
            remark: None,
            lines: vec![
                VoucherLineDraft {
                    account_code: "6603".into(),
                    debit_amount: 30.0,
                    credit_amount: 0.0,
                    summary: None,
                },
                VoucherLineDraft {
                    account_code: "1002".into(),
                    debit_amount: 0.0,
                    credit_amount: 30.0,
                    summary: None,
                },
            ],
        };
        insert_voucher(&conn, &draft).unwrap();
        let wb = db::get_month_close_workbench(&conn, "2026-08").unwrap();
        let item = wb
            .checks
            .iter()
            .find(|c| c.title.contains("记账凭证平衡"))
            .unwrap();
        assert_eq!(item.status, "ok");
        // 手动 UPDATE vouchers SET total_amount=0 制造异常（模拟不平衡），断言月结检查返回阻塞项
        conn.execute("UPDATE vouchers SET total_amount = 0", [])
            .unwrap();
        let wb = db::get_month_close_workbench(&conn, "2026-08").unwrap();
        let item = wb
            .checks
            .iter()
            .find(|c| c.title.contains("记账凭证平衡"))
            .unwrap();
        assert_eq!(item.status, "blocking");
        assert_eq!(item.count, 1);
        assert!(
            item.description.contains("不平衡") && item.description.contains("1 张"),
            "got: {}",
            item.description
        );
        // 其他月份的不平衡凭证不影响本月检查
        conn.execute("UPDATE vouchers SET total_amount = 30.0", [])
            .unwrap();
        let wb = db::get_month_close_workbench(&conn, "2026-08").unwrap();
        let item = wb
            .checks
            .iter()
            .find(|c| c.title.contains("记账凭证平衡"))
            .unwrap();
        assert_eq!(item.status, "ok");
        // 月份隔离：2026-07 存在 total_amount=0 的不平衡凭证，2026-08 检查仍通过
        conn.execute(
            "INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status, created_at, updated_at)
             VALUES ('记-202607-001', '2026-07-31', '2026-07', 'bank_manual', 999, 0.0, 'active', '2026-07-31', '2026-07-31')",
            [],
        )
        .unwrap();
        let july_vid: i64 = conn
            .query_row(
                "SELECT id FROM vouchers WHERE voucher_no = '记-202607-001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount, summary, line_order)
             VALUES (?1, '6603', 10.0, 0.0, '他月不平衡凭证', 0)",
            params![july_vid],
        )
        .unwrap();
        let wb = db::get_month_close_workbench(&conn, "2026-08").unwrap();
        let item = wb
            .checks
            .iter()
            .find(|c| c.title.contains("记账凭证平衡"))
            .unwrap();
        assert_eq!(item.status, "ok");
        // 而 2026-07 检查应阻塞（确认该凭证确实被检出，只是不影响他月）
        let wb_july = db::get_month_close_workbench(&conn, "2026-07").unwrap();
        let item_july = wb_july
            .checks
            .iter()
            .find(|c| c.title.contains("记账凭证平衡"))
            .unwrap();
        assert_eq!(item_july.status, "blocking");
    }

    #[test]
    fn test_account_mapping() {
        let conn = setup();
        save_account_mapping(
            &conn,
            &AccountMappingInput {
                scope: "expense_type".into(),
                key: "OFFICE".into(),
                account_code: "6602".into(),
                remark: None,
            },
        )
        .unwrap();
        let maps = get_account_mappings(&conn).unwrap();
        assert_eq!(maps.len(), 1);
        assert!(delete_account_mapping(&conn, maps[0].id).unwrap());
    }

    // ==================== 报表计算引擎（Task 10） ====================

    fn approx(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 0.005,
            "期望 {expected}，实际 {actual}"
        );
    }

    fn row_value<'a>(rows: impl IntoIterator<Item = &'a ReportRow>, key: &str) -> &'a ReportRow {
        rows.into_iter()
            .find(|r| r.key == key)
            .unwrap_or_else(|| panic!("报表缺少行 {key}"))
    }

    fn vline(code: &str, debit: f64, credit: f64) -> VoucherLineDraft {
        VoucherLineDraft {
            account_code: code.into(),
            debit_amount: debit,
            credit_amount: credit,
            summary: Some(format!("{code} 摘要")),
        }
    }

    fn manual_draft(month: &str, source_id: i64, lines: Vec<VoucherLineDraft>) -> VoucherDraft {
        VoucherDraft {
            belong_month: month.into(),
            voucher_date: format!("{month}-28"),
            source_type: "bank_manual".into(),
            source_id,
            remark: None,
            lines,
        }
    }

    #[test]
    fn test_reports() {
        let conn = setup();
        // 期初：1002 借 100000，2001 贷 40000，3001 贷 60000，启用月 2026-01
        save_opening_balances(
            &conn,
            "2026-01",
            &[
                OpeningBalanceRow {
                    account_code: "1002".into(),
                    debit_amount: 100000.0,
                    credit_amount: 0.0,
                },
                OpeningBalanceRow {
                    account_code: "2001".into(),
                    debit_amount: 0.0,
                    credit_amount: 40000.0,
                },
                OpeningBalanceRow {
                    account_code: "3001".into(),
                    debit_amount: 0.0,
                    credit_amount: 60000.0,
                },
            ],
        )
        .unwrap();
        // 2026-02 三张手工凭证：
        //   借 6602 5000 / 贷 1002 5000（管理费用，经营流出）
        //   借 1002 2000 / 贷 6301 2000（营业外收入，经营流入）
        //   借 1601 30000 / 贷 1002 30000（购固定资产，投资流出）
        insert_voucher(
            &conn,
            &manual_draft(
                "2026-02",
                1,
                vec![vline("6602", 5000.0, 0.0), vline("1002", 0.0, 5000.0)],
            ),
        )
        .unwrap();
        insert_voucher(
            &conn,
            &manual_draft(
                "2026-02",
                2,
                vec![vline("1002", 2000.0, 0.0), vline("6301", 0.0, 2000.0)],
            ),
        )
        .unwrap();
        insert_voucher(
            &conn,
            &manual_draft(
                "2026-02",
                3,
                vec![vline("1601", 30000.0, 0.0), vline("1002", 0.0, 30000.0)],
            ),
        )
        .unwrap();

        // ---- 资产负债表 2026-02 ----
        let bs = build_balance_sheet(&conn, "2026-02").unwrap();
        assert!(bs.enabled);
        // 货币资金 = 100000 - 5000 + 2000 - 30000 = 67000；年初（期初口径）100000
        approx(row_value(&bs.asset_rows, "monetary").current, 67000.0);
        approx(row_value(&bs.asset_rows, "monetary").comparative, 100000.0);
        approx(row_value(&bs.asset_rows, "1601").current, 30000.0);
        approx(row_value(&bs.asset_rows, "1601").comparative, 0.0);
        approx(
            row_value(&bs.liability_equity_rows, "2001").current,
            40000.0,
        );
        approx(
            row_value(&bs.liability_equity_rows, "3001").current,
            60000.0,
        );
        // 未分配利润 = 3104 期末 + 启用月至当月累计净利润 = 0 + (-5000 + 2000) = -3000
        approx(
            row_value(&bs.liability_equity_rows, "undistributed").current,
            -3000.0,
        );
        approx(bs.asset_total, 97000.0);
        approx(bs.liability_equity_total, 97000.0);
        assert!(bs.balanced);

        // ---- 利润表 2026-02 ----
        let is = build_income_statement(&conn, "2026-02").unwrap();
        assert!(is.year_cumulative);
        approx(row_value(&is.rows, "6602").current, 5000.0);
        approx(row_value(&is.rows, "6301").current, 2000.0);
        approx(row_value(&is.rows, "operating_profit").current, -5000.0);
        approx(row_value(&is.rows, "total_profit").current, -3000.0);
        approx(row_value(&is.rows, "net_profit").current, -3000.0);
        approx(is.net_profit_month, -3000.0);
        approx(is.net_profit_year, -3000.0);
        // 无 1 月损益时本年累计与当月一致
        approx(row_value(&is.rows, "6602").comparative, 5000.0);
        approx(row_value(&is.rows, "6001").current, 0.0);

        // ---- 现金流量表 2026-02 ----
        let cf = build_cash_flow_statement(&conn, "2026-02").unwrap();
        approx(row_value(&cf.rows, "operating_inflow").current, 2000.0);
        approx(row_value(&cf.rows, "operating_outflow").current, 5000.0);
        approx(row_value(&cf.rows, "investing_inflow").current, 0.0);
        approx(row_value(&cf.rows, "investing_outflow").current, 30000.0);
        approx(row_value(&cf.rows, "financing_inflow").current, 0.0);
        approx(row_value(&cf.rows, "financing_outflow").current, 0.0);
        approx(row_value(&cf.rows, "other").current, 0.0);
        approx(cf.net_increase, -33000.0);
        assert!(cf.unclassified.is_empty());

        // ---- 启用月之前：enabled=false 全 0 ----
        let bs_prev = build_balance_sheet(&conn, "2025-12").unwrap();
        assert!(!bs_prev.enabled);
        assert!(bs_prev.asset_rows.is_empty());
        assert!(bs_prev.liability_equity_rows.is_empty());
        approx(bs_prev.asset_total, 0.0);
        approx(bs_prev.liability_equity_total, 0.0);
        let is_prev = build_income_statement(&conn, "2025-12").unwrap();
        assert!(!is_prev.year_cumulative);
        approx(is_prev.net_profit_month, 0.0);
        approx(is_prev.net_profit_year, 0.0);
        approx(row_value(&is_prev.rows, "6602").current, 0.0);
        let cf_prev = build_cash_flow_statement(&conn, "2025-12").unwrap();
        approx(cf_prev.net_increase, 0.0);
        assert!(cf_prev.unclassified.is_empty());
    }

    /// 跨月累计：2026-01 的损益凭证应计入 2026-02 报表的"本年累计"与未分配利润。
    #[test]
    fn test_reports_cross_month_cumulative_profit() {
        let conn = setup();
        save_opening_balances(
            &conn,
            "2026-01",
            &[
                OpeningBalanceRow {
                    account_code: "1002".into(),
                    debit_amount: 100000.0,
                    credit_amount: 0.0,
                },
                OpeningBalanceRow {
                    account_code: "2001".into(),
                    debit_amount: 0.0,
                    credit_amount: 40000.0,
                },
                OpeningBalanceRow {
                    account_code: "3001".into(),
                    debit_amount: 0.0,
                    credit_amount: 60000.0,
                },
            ],
        )
        .unwrap();
        // 2026-01：计提管理费用 1000（贷应付账款，不涉现金）
        insert_voucher(
            &conn,
            &manual_draft(
                "2026-01",
                1,
                vec![vline("6602", 1000.0, 0.0), vline("2202", 0.0, 1000.0)],
            ),
        )
        .unwrap();
        // 2026-02：同主场景三张凭证
        insert_voucher(
            &conn,
            &manual_draft(
                "2026-02",
                2,
                vec![vline("6602", 5000.0, 0.0), vline("1002", 0.0, 5000.0)],
            ),
        )
        .unwrap();
        insert_voucher(
            &conn,
            &manual_draft(
                "2026-02",
                3,
                vec![vline("1002", 2000.0, 0.0), vline("6301", 0.0, 2000.0)],
            ),
        )
        .unwrap();
        insert_voucher(
            &conn,
            &manual_draft(
                "2026-02",
                4,
                vec![vline("1601", 30000.0, 0.0), vline("1002", 0.0, 30000.0)],
            ),
        )
        .unwrap();

        // 1 月利润表：当月 = 本年累计（启用月即 1 月）
        let is1 = build_income_statement(&conn, "2026-01").unwrap();
        assert!(is1.year_cumulative);
        approx(row_value(&is1.rows, "6602").current, 1000.0);
        approx(row_value(&is1.rows, "6602").comparative, 1000.0);
        approx(is1.net_profit_month, -1000.0);
        approx(is1.net_profit_year, -1000.0);
        // 1 月资产负债表：未分配利润 -1000，货币资金 100000（1 月凭证不涉现金）
        let bs1 = build_balance_sheet(&conn, "2026-01").unwrap();
        approx(row_value(&bs1.asset_rows, "monetary").current, 100000.0);
        approx(
            row_value(&bs1.liability_equity_rows, "undistributed").current,
            -1000.0,
        );
        approx(
            row_value(&bs1.liability_equity_rows, "2202").current,
            1000.0,
        );
        assert!(bs1.balanced);

        // 2 月利润表：管理费用当月 5000 / 本年累计 6000；净利润当月 -3000 / 累计 -4000
        let is2 = build_income_statement(&conn, "2026-02").unwrap();
        approx(row_value(&is2.rows, "6602").current, 5000.0);
        approx(row_value(&is2.rows, "6602").comparative, 6000.0);
        approx(is2.net_profit_month, -3000.0);
        approx(is2.net_profit_year, -4000.0);

        // 2 月资产负债表：未分配利润含 1 月亏损 = -4000，等式仍平衡
        let bs2 = build_balance_sheet(&conn, "2026-02").unwrap();
        approx(
            row_value(&bs2.liability_equity_rows, "undistributed").current,
            -4000.0,
        );
        approx(bs2.asset_total, 97000.0);
        approx(bs2.liability_equity_total, 97000.0);
        assert!(bs2.balanced);
        // 年初口径：1 月的应付账款凭证不滚入 2 月报表的 comparative，
        // 2202 行年初（启用期初）= 0、期末 = 1000
        approx(
            row_value(&bs2.liability_equity_rows, "2202").current,
            1000.0,
        );
        approx(
            row_value(&bs2.liability_equity_rows, "2202").comparative,
            0.0,
        );
    }

    /// 自定义损益科目（非 12 标准编码）通过"其他未列示损益"兜底行进入利润表与净利润，
    /// 资产负债表仍平衡（Fix Round 1 Finding 2a）。
    #[test]
    fn test_reports_custom_profit_loss_fallback_row() {
        let conn = setup();
        save_opening_balances(
            &conn,
            "2026-01",
            &[
                OpeningBalanceRow {
                    account_code: "1002".into(),
                    debit_amount: 100000.0,
                    credit_amount: 0.0,
                },
                OpeningBalanceRow {
                    account_code: "3001".into(),
                    debit_amount: 0.0,
                    credit_amount: 100000.0,
                },
            ],
        )
        .unwrap();
        // 自定义损益科目 660299（profit_loss/debit，映射放行可达）
        create_account(
            &conn,
            &GlAccountInput {
                code: "660299".into(),
                name: "管理费用—其他".into(),
                category: "profit_loss".into(),
                direction: "debit".into(),
                cash_flow_category: Some("operating".into()),
                remark: None,
            },
        )
        .unwrap();
        // 手工凭证：借 660299 500 / 贷 1002 500（2026-02）
        insert_voucher(
            &conn,
            &manual_draft(
                "2026-02",
                1,
                vec![vline("660299", 500.0, 0.0), vline("1002", 0.0, 500.0)],
            ),
        )
        .unwrap();

        // 利润表：其他未列示损益 = 贷-借 = -500，计入净利润但不进营业利润/利润总额
        let is = build_income_statement(&conn, "2026-02").unwrap();
        approx(row_value(&is.rows, "other_pl").current, -500.0);
        approx(row_value(&is.rows, "other_pl").comparative, -500.0);
        approx(row_value(&is.rows, "operating_profit").current, 0.0);
        approx(row_value(&is.rows, "total_profit").current, 0.0);
        approx(row_value(&is.rows, "net_profit").current, -500.0);
        approx(is.net_profit_month, -500.0);
        approx(is.net_profit_year, -500.0);

        // 资产负债表：货币资金 99500，未分配利润 -500（含兜底行），等式平衡
        let bs = build_balance_sheet(&conn, "2026-02").unwrap();
        approx(row_value(&bs.asset_rows, "monetary").current, 99500.0);
        approx(
            row_value(&bs.liability_equity_rows, "undistributed").current,
            -500.0,
        );
        approx(bs.asset_total, 99500.0);
        approx(bs.liability_equity_total, 99500.0);
        assert!(bs.balanced);
    }

    /// 映射到 cost 类科目（5001）的凭证经"成本类科目"资产行兜底，资产负债表平衡
    /// （Fix Round 1 Finding 2b）。
    #[test]
    fn test_reports_cost_accounts_balance_sheet_row() {
        let conn = setup();
        save_opening_balances(
            &conn,
            "2026-01",
            &[
                OpeningBalanceRow {
                    account_code: "1002".into(),
                    debit_amount: 100000.0,
                    credit_amount: 0.0,
                },
                OpeningBalanceRow {
                    account_code: "3001".into(),
                    debit_amount: 0.0,
                    credit_amount: 100300.0,
                },
                OpeningBalanceRow {
                    account_code: "5001".into(),
                    debit_amount: 300.0,
                    credit_amount: 0.0,
                },
            ],
        )
        .unwrap();
        // 费用类型映射到 cost 类科目 5001（save_account_mapping 放行）
        save_account_mapping(
            &conn,
            &AccountMappingInput {
                scope: "expense_type".into(),
                key: "office".into(),
                account_code: "5001".into(),
                remark: None,
            },
        )
        .unwrap();
        // 无报销关联发票：amount=500 tax=0 → invoice_expense 凭证 借 5001 500 / 贷 2241 500
        conn.execute(
            "INSERT INTO invoices (id, invoice_code, invoice_number, amount, tax_amount, total_amount,
                 expense_type_code, employee_id, belong_month, status, created_at, updated_at)
             VALUES (1, 'A', '001', 500.0, 0.0, 500.0, 'office', 1, '2026-02', 'normal',
                     '2026-02-10', '2026-02-10')",
            [],
        )
        .unwrap();
        assert!(maybe_generate_invoice_expense_voucher(&conn, 1)
            .unwrap()
            .is_some());

        // 资产负债表：成本类科目行期末 = 300 期初 + 500 当月 = 800（资产行末尾），
        // 年初 = Σ opening_raw = 300；资产合计 100000+800，负债端 2241 500 + 3001 100300，平衡
        let bs = build_balance_sheet(&conn, "2026-02").unwrap();
        approx(row_value(&bs.asset_rows, "cost_accounts").current, 800.0);
        approx(
            row_value(&bs.asset_rows, "cost_accounts").comparative,
            300.0,
        );
        approx(row_value(&bs.asset_rows, "monetary").current, 100000.0);
        approx(row_value(&bs.liability_equity_rows, "2241").current, 500.0);
        approx(bs.asset_total, 100800.0);
        approx(bs.liability_equity_total, 100800.0);
        assert!(bs.balanced);

        // 利润表不含 cost 类科目：兜底行与净利润均为 0
        let is = build_income_statement(&conn, "2026-02").unwrap();
        approx(row_value(&is.rows, "other_pl").current, 0.0);
        approx(is.net_profit_month, 0.0);
    }

    /// 对方科目 cash_flow_category=none 的现金支出归"其他"行并进入 unclassified 明细。
    #[test]
    fn test_cash_flow_unclassified_none_category() {
        let conn = setup();
        save_opening_balances(
            &conn,
            "2026-01",
            &[
                OpeningBalanceRow {
                    account_code: "1002".into(),
                    debit_amount: 50000.0,
                    credit_amount: 0.0,
                },
                OpeningBalanceRow {
                    account_code: "3001".into(),
                    debit_amount: 0.0,
                    credit_amount: 50000.0,
                },
            ],
        )
        .unwrap();
        // 借 3103（本年利润，cfc=none）800 / 贷 1002 800
        insert_voucher(
            &conn,
            &VoucherDraft {
                belong_month: "2026-02".into(),
                voucher_date: "2026-02-28".into(),
                source_type: "bank_manual".into(),
                source_id: 1,
                remark: Some("手工凭证".into()),
                lines: vec![
                    VoucherLineDraft {
                        account_code: "3103".into(),
                        debit_amount: 800.0,
                        credit_amount: 0.0,
                        summary: Some("结转损益".into()),
                    },
                    VoucherLineDraft {
                        account_code: "1002".into(),
                        debit_amount: 0.0,
                        credit_amount: 800.0,
                        summary: Some("银行支出".into()),
                    },
                ],
            },
        )
        .unwrap();
        let cf = build_cash_flow_statement(&conn, "2026-02").unwrap();
        // 六行全 0，其他行 -800，净增加 -800
        approx(row_value(&cf.rows, "operating_inflow").current, 0.0);
        approx(row_value(&cf.rows, "operating_outflow").current, 0.0);
        approx(row_value(&cf.rows, "investing_inflow").current, 0.0);
        approx(row_value(&cf.rows, "investing_outflow").current, 0.0);
        approx(row_value(&cf.rows, "financing_inflow").current, 0.0);
        approx(row_value(&cf.rows, "financing_outflow").current, 0.0);
        approx(row_value(&cf.rows, "other").current, -800.0);
        approx(cf.net_increase, -800.0);
        // unclassified 明细：凭证号 + 对方行摘要 + 负数金额（流出）
        assert_eq!(cf.unclassified.len(), 1);
        assert_eq!(cf.unclassified[0].voucher_no, "记-202602-001");
        assert_eq!(cf.unclassified[0].summary.as_deref(), Some("结转损益"));
        approx(cf.unclassified[0].amount, -800.0);
    }

    /// 一张凭证多个对方行：现金净流出按对方行金额比例分摊（3:7 → 经营 300 / 投资 700）。
    #[test]
    fn test_cash_flow_split_allocation_across_counter_lines() {
        let conn = setup();
        save_opening_balances(
            &conn,
            "2026-01",
            &[
                OpeningBalanceRow {
                    account_code: "1002".into(),
                    debit_amount: 50000.0,
                    credit_amount: 0.0,
                },
                OpeningBalanceRow {
                    account_code: "3001".into(),
                    debit_amount: 0.0,
                    credit_amount: 50000.0,
                },
            ],
        )
        .unwrap();
        // 借 6602 300（经营）/ 借 1601 700（投资）/ 贷 1002 1000
        insert_voucher(
            &conn,
            &manual_draft(
                "2026-02",
                1,
                vec![
                    vline("6602", 300.0, 0.0),
                    vline("1601", 700.0, 0.0),
                    vline("1002", 0.0, 1000.0),
                ],
            ),
        )
        .unwrap();
        let cf = build_cash_flow_statement(&conn, "2026-02").unwrap();
        approx(row_value(&cf.rows, "operating_outflow").current, 300.0);
        approx(row_value(&cf.rows, "investing_outflow").current, 700.0);
        approx(row_value(&cf.rows, "operating_inflow").current, 0.0);
        approx(cf.net_increase, -1000.0);
        assert!(cf.unclassified.is_empty());
    }

    #[test]
    fn test_trial_balance_basic() {
        let conn = setup();
        // 期初：1001 借 1000、2211 贷 1000
        conn.execute("INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount) VALUES ('2025-01', '1001', 1000.0, 0.0)", []).unwrap();
        conn.execute("INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount) VALUES ('2025-01', '2211', 0.0, 1000.0)", []).unwrap();
        // 2025-01 发生：借 6602 / 贷 1001 各 100
        conn.execute("INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status) VALUES ('记-202501-001', '2025-01-10', '2025-01', 'bank_manual', 1, 100.0, 'active')", []).unwrap();
        let vid: i64 = conn
            .query_row(
                "SELECT id FROM vouchers WHERE voucher_no='记-202501-001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '6602', 100.0, 0.0)", [vid]).unwrap();
        conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '1001', 0.0, 100.0)", [vid]).unwrap();

        let report = build_trial_balance(&conn, "2025-01", "2025-01").unwrap();
        assert!(report.enabled);
        let cash = report.rows.iter().find(|r| r.code == "1001").unwrap();
        assert_eq!(cash.opening_debit, 1000.0);
        assert_eq!(cash.period_credit, 100.0);
        assert_eq!(cash.ending_debit, 900.0);
        assert_eq!(cash.ending_credit, 0.0);
        assert!(report.balanced);
    }

    #[test]
    fn test_trial_balance_cross_month_rolls_opening() {
        let conn = setup();
        // 期初平衡：1001 借 500、2001 贷 500（brief 原文只录借方 500 不平，balanced 断言不可能成立，按平衡期初修正）
        conn.execute("INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount) VALUES ('2025-01', '1001', 500.0, 0.0)", []).unwrap();
        conn.execute("INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount) VALUES ('2025-01', '2001', 0.0, 500.0)", []).unwrap();
        // 2025-01 发生：借 2241 / 贷 1001 各 100 -> 2025-02 查询时 1001 期初应为 400、2241 期初在借侧 100
        conn.execute("INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status) VALUES ('记-202501-001', '2025-01-10', '2025-01', 'bank_manual', 1, 100.0, 'active')", []).unwrap();
        let vid: i64 = conn
            .query_row(
                "SELECT id FROM vouchers WHERE voucher_no='记-202501-001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '1001', 0.0, 100.0)", [vid]).unwrap();
        conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '2241', 100.0, 0.0)", [vid]).unwrap();

        let report = build_trial_balance(&conn, "2025-02", "2025-02").unwrap();
        let cash = report.rows.iter().find(|r| r.code == "1001").unwrap();
        assert_eq!(cash.opening_debit, 400.0);
        // 2241 无期初、区间前净额借方 -> 滚入期初在借侧 100
        let other = report.rows.iter().find(|r| r.code == "2241").unwrap();
        assert_eq!(other.opening_debit, 100.0);
        assert_eq!(other.opening_credit, 0.0);
        assert!(report.balanced);
    }

    #[test]
    fn test_trial_balance_not_enabled_without_opening() {
        let conn = setup();
        let report = build_trial_balance(&conn, "2025-01", "2025-01").unwrap();
        assert!(!report.enabled);
        assert!(report.rows.is_empty());
    }

    // ==================== 年末结转凭证（Task 3） ====================

    #[test]
    fn test_period_close_vouchers() {
        let conn = setup();
        // 启用月 2025-01 + 收入 6001 贷 1000 / 费用 6602 借 400 的凭证
        conn.execute("INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount) VALUES ('2025-01', '1001', 0.0, 0.0)", []).unwrap();
        conn.execute("INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status) VALUES ('记-202512-001', '2025-12-05', '2025-12', 'bank_manual', 1, 1000.0, 'active')", []).unwrap();
        let vid: i64 = conn
            .query_row(
                "SELECT id FROM vouchers WHERE voucher_no='记-202512-001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '6001', 0.0, 1000.0)", [vid]).unwrap();
        conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '1001', 1000.0, 0.0)", [vid]).unwrap();
        conn.execute("INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status) VALUES ('记-202512-002', '2025-12-06', '2025-12', 'bank_manual', 2, 400.0, 'active')", []).unwrap();
        let vid2: i64 = conn
            .query_row(
                "SELECT id FROM vouchers WHERE voucher_no='记-202512-002'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '6602', 400.0, 0.0)", [vid2]).unwrap();
        conn.execute("INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount) VALUES (?1, '1001', 0.0, 400.0)", [vid2]).unwrap();

        let n = generate_period_close_vouchers(&conn, "2025-12").unwrap();
        assert_eq!(n, 2); // 结转损益 + 结转本年利润
                          // 结转后 3103 余额为 0、3104 余额 600（余额表含全部凭证口径）
        let tb = build_trial_balance(&conn, "2025-12", "2025-12").unwrap();
        let p3103 = tb.rows.iter().find(|r| r.code == "3103").unwrap();
        assert_eq!(p3103.ending_debit, 0.0);
        assert_eq!(p3103.ending_credit, 0.0);
        let p3104 = tb.rows.iter().find(|r| r.code == "3104").unwrap();
        assert_eq!(p3104.ending_credit, 600.0);
        // 报表口径排除 period_close：利润表累计收入仍 1000、费用 400
        let income = build_income_statement(&conn, "2025-12").unwrap();
        let rev = income.rows.iter().find(|r| r.key == "6001").unwrap();
        assert_eq!(rev.comparative, 1000.0);
        let exp = income.rows.iter().find(|r| r.key == "6602").unwrap();
        assert_eq!(exp.comparative, 400.0);
        // 幂等：该月已有 active period_close 凭证时返回 0
        let n2 = generate_period_close_vouchers(&conn, "2025-12").unwrap();
        assert_eq!(n2, 0);

        // 反月结作废
        let voided = void_period_close_vouchers(&conn, "2025-12").unwrap();
        assert_eq!(voided, 2);
    }

    #[test]
    fn test_period_close_skips_non_december_and_zero() {
        let conn = setup();
        conn.execute("INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount) VALUES ('2025-01', '1001', 0.0, 0.0)", []).unwrap();
        assert_eq!(generate_period_close_vouchers(&conn, "2025-11").unwrap(), 0);
        assert_eq!(generate_period_close_vouchers(&conn, "2025-12").unwrap(), 0);
        // 无损益凭证
    }
}
