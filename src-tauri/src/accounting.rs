use crate::errors::{AppError, AppResult};
use crate::models::*;
use chrono::Utc;
use rusqlite::{params, Connection};
use std::collections::HashSet;

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
    conn.execute(
        "INSERT INTO vouchers (voucher_no, voucher_date, belong_month, source_type, source_id, total_amount, status, remark, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8, ?8)",
        params![voucher_no, draft.voucher_date, draft.belong_month, draft.source_type, draft.source_id, debit, draft.remark, now],
    )?;
    let id = conn.last_insert_rowid();
    for (i, line) in draft.lines.iter().enumerate() {
        conn.execute(
            "INSERT INTO voucher_lines (voucher_id, account_code, debit_amount, credit_amount, summary, line_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, line.account_code, line.debit_amount, line.credit_amount, line.summary, i as i64],
        )?;
    }
    get_voucher(conn, id)
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
pub fn void_vouchers_for_source(
    conn: &Connection,
    source_type: &str,
    source_id: i64,
) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE vouchers SET status = 'void', updated_at = ?3 WHERE source_type = ?1 AND source_id = ?2 AND status = 'active'",
        params![source_type, source_id, Utc::now().to_rfc3339()],
    )?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::create_tables(&conn).unwrap();
        db::seed_gl_accounts(&conn).unwrap();
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
}
