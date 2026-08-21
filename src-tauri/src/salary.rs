use rusqlite::{params, Connection};

use crate::db::*;
use crate::errors::{AppError, AppResult};
use crate::models::*;

/// Calculate monthly salary for all active employees.
/// Returns the list of calculated SalaryResult.
pub fn calculate_monthly_salary(month: &str, conn: &Connection) -> AppResult<Vec<SalaryResult>> {
    let employees = get_employees(conn)?;
    let attendance_map = build_attendance_map(conn, month)?;
    let rules = build_rules_map(conn)?;

    let meal_allowance = get_rule_value(conn, "meal_allowance").unwrap_or(0.0);
    let transport_allowance = get_rule_value(conn, "transport_allowance").unwrap_or(0.0);

    let mut results = Vec::new();

    for emp in &employees {
        if emp.status != "active" {
            continue;
        }

        let att = attendance_map.get(&emp.employee_no);

        let result = calculate_single_employee(
            month,
            emp,
            att,
            &rules,
            conn,
            meal_allowance,
            transport_allowance,
        )?;

        save_salary_result(conn, &result)?;
        results.push(result);
    }

    if !results.is_empty() {
        log_operation(
            conn,
            "calculate_salary",
            &format!("计算{month}月工资，共{}人", results.len()),
            "system",
            None,
        )?;
    }

    Ok(results)
}

/// Recalculate salary for a single employee.
pub fn recalculate_single(
    month: &str,
    employee_no: &str,
    conn: &Connection,
) -> AppResult<SalaryResult> {
    let emp = conn.query_row(
        "SELECT id, employee_no, name, department, position, id_card, phone, bank_account, bank_name, hire_date, status, base_salary, position_salary, performance_salary, social_security_base, housing_fund_base, special_deduction, remark, created_at, updated_at FROM employees WHERE employee_no = ?1",
        rusqlite::params![employee_no],
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
    ).map_err(|e| AppError::NotFound(format!("员工{employee_no}未找到: {e}")))?;

    let attendance_map = build_attendance_map(conn, month)?;
    let att = attendance_map.get(employee_no);
    let rules = build_rules_map(conn)?;

    let meal_allowance = get_rule_value(conn, "meal_allowance").unwrap_or(0.0);
    let transport_allowance = get_rule_value(conn, "transport_allowance").unwrap_or(0.0);

    let result = calculate_single_employee(
        month,
        &emp,
        att,
        &rules,
        conn,
        meal_allowance,
        transport_allowance,
    )?;

    save_salary_result(conn, &result)?;
    Ok(result)
}

fn build_attendance_map(
    conn: &Connection,
    month: &str,
) -> AppResult<std::collections::HashMap<String, AttendanceRecord>> {
    let records = get_attendance_records(conn, month)?;
    let mut map = std::collections::HashMap::new();
    for r in records {
        map.insert(r.employee_no.clone(), r);
    }
    Ok(map)
}

fn build_rules_map(conn: &Connection) -> AppResult<std::collections::HashMap<String, f64>> {
    let rules = get_salary_rules(conn)?;
    let mut map = std::collections::HashMap::new();
    for r in &rules {
        if r.enabled == 1 {
            map.insert(r.rule_key.clone(), r.rule_value);
        }
    }
    Ok(map)
}

/// 累计预扣法：累计应纳税所得额×预扣率-速算扣除-累计已预扣（max 0）。
/// 历史月份（含旧月度算法结果）自然作为"已预扣"基数，启用当月平滑。
/// 注意：历史月的专项附加未落库，按当月值×月数近似（员工专项附加年度内不变）。
pub fn calculate_cumulative_tax(
    conn: &Connection,
    employee_no: &str,
    month: &str,
    gross: f64,
    ss_personal: f64,
    hf_personal: f64,
    special_deduction: f64,
    threshold: f64,
) -> AppResult<f64> {
    let year_prefix = format!("{}-%", &month[..4]);
    let (prev_gross, prev_ss, prev_tax, prev_count): (f64, f64, f64, i64) = conn
        .query_row(
            "SELECT COALESCE(SUM(gross_salary),0), COALESCE(SUM(social_security_personal + housing_fund_personal),0),
                    COALESCE(SUM(tax_amount),0), COUNT(*)
             FROM salary_monthly_results
             WHERE employee_no = ?1 AND salary_month LIKE ?2 AND salary_month < ?3 AND status != 'void'",
            params![employee_no, year_prefix, month],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap_or((0.0, 0.0, 0.0, 0));
    let months = (prev_count + 1) as f64;
    let cumulative_taxable = (prev_gross + gross)
        - (prev_ss + ss_personal + hf_personal)
        - threshold * months
        - special_deduction * months;
    if cumulative_taxable <= 0.0 {
        return Ok(0.0);
    }
    let rules = get_cumulative_tax_rules(conn)?;
    let mut annual_tax = 0.0;
    for rule in &rules {
        let max = rule.max_amount.unwrap_or(f64::MAX);
        if cumulative_taxable > rule.min_amount && cumulative_taxable <= max {
            annual_tax = (cumulative_taxable * rule.tax_rate - rule.quick_deduction).max(0.0);
            break;
        }
    }
    Ok((annual_tax - prev_tax).max(0.0))
}

fn calculate_single_employee(
    month: &str,
    emp: &Employee,
    att: Option<&AttendanceRecord>,
    rules: &std::collections::HashMap<String, f64>,
    conn: &Connection,
    default_meal: f64,
    default_transport: f64,
) -> AppResult<SalaryResult> {
    let base_salary = emp.base_salary;
    let position_salary = emp.position_salary;
    let performance_salary = emp.performance_salary;

    // Overtime calculation
    let daily_salary = base_salary / 21.75;
    let hourly_salary = daily_salary / 8.0;
    let overtime_rate = rules.get("overtime_rate").copied().unwrap_or(1.5);

    let overtime_hours = att.map(|a| a.overtime_hours).unwrap_or(0.0);
    let overtime_salary = hourly_salary * overtime_hours * overtime_rate;

    // Allowances
    let meal_allowance = default_meal;
    let transport_allowance = default_transport;
    let other_allowance = 0.0;

    // Gross salary
    let gross_salary = base_salary
        + position_salary
        + performance_salary
        + overtime_salary
        + meal_allowance
        + transport_allowance
        + other_allowance;

    // Social security and housing fund
    let social_security_rate = rules.get("social_security_rate").copied().unwrap_or(0.105);
    let housing_fund_rate = rules.get("housing_fund_rate").copied().unwrap_or(0.12);

    // 社保公积金：优先取年度台账（基数按上下限 clamp），无台账回退员工基数/全局费率
    let profile_year: i64 = month.get(0..4).and_then(|y| y.parse().ok()).unwrap_or(0);
    let profile: Option<SocialInsuranceProfile> = conn
        .query_row(
            "SELECT id, employee_no, profile_year, ss_base, hf_base, ss_employer_rate,
                    ss_personal_rate, hf_employer_rate, hf_personal_rate, remark, created_at, updated_at
             FROM social_insurance_profiles WHERE employee_no = ?1 AND profile_year = ?2",
            params![emp.employee_no, profile_year],
            |r| {
                Ok(SocialInsuranceProfile {
                    id: r.get(0)?,
                    employee_no: r.get(1)?,
                    profile_year: r.get(2)?,
                    ss_base: r.get(3)?,
                    hf_base: r.get(4)?,
                    ss_employer_rate: r.get(5)?,
                    ss_personal_rate: r.get(6)?,
                    hf_employer_rate: r.get(7)?,
                    hf_personal_rate: r.get(8)?,
                    remark: r.get(9)?,
                    created_at: r.get(10)?,
                    updated_at: r.get(11)?,
                })
            },
        )
        .ok();
    let (ss_min, ss_max, hf_min, hf_max) = get_social_base_limits(conn)?;
    let (
        social_security_base,
        housing_fund_base,
        ss_personal_rate,
        hf_personal_rate,
        ss_employer_rate,
        hf_employer_rate,
    ) = match &profile {
        Some(p) => (
            clamp_base(p.ss_base, ss_min, ss_max),
            clamp_base(p.hf_base, hf_min, hf_max),
            p.ss_personal_rate,
            p.hf_personal_rate,
            p.ss_employer_rate,
            p.hf_employer_rate,
        ),
        None => (
            if emp.social_security_base > 0.0 {
                emp.social_security_base
            } else {
                base_salary
            },
            if emp.housing_fund_base > 0.0 {
                emp.housing_fund_base
            } else {
                base_salary
            },
            social_security_rate,
            housing_fund_rate,
            0.0,
            0.0,
        ),
    };

    let social_security_personal = social_security_base * ss_personal_rate;
    let housing_fund_personal = housing_fund_base * hf_personal_rate;
    let social_security_employer = social_security_base * ss_employer_rate;
    let housing_fund_employer = housing_fund_base * hf_employer_rate;

    // Attendance deduction
    let attendance_deduction = calculate_attendance_deduction(att, daily_salary, rules);

    // Tax calculation
    let tax_threshold = rules.get("tax_threshold").copied().unwrap_or(5000.0);
    let special_deduction = emp.special_deduction;

    // 个税改累计预扣法：历史已存记录自动作为"已预扣"基数
    let tax_amount = calculate_cumulative_tax(
        conn,
        &emp.employee_no,
        month,
        gross_salary,
        social_security_personal,
        housing_fund_personal,
        special_deduction,
        tax_threshold,
    )?;

    // Other deduction (from existing record if any)
    let other_deduction = 0.0;

    // Net salary
    let net_salary = gross_salary
        - social_security_personal
        - housing_fund_personal
        - attendance_deduction
        - tax_amount
        - other_deduction;

    Ok(SalaryResult {
        id: 0,
        salary_month: month.to_string(),
        employee_no: emp.employee_no.clone(),
        name: Some(emp.name.clone()),
        department: emp.department.clone(),
        base_salary,
        position_salary,
        performance_salary,
        overtime_salary: (overtime_salary * 100.0).round() / 100.0,
        meal_allowance,
        transport_allowance,
        other_allowance,
        gross_salary: (gross_salary * 100.0).round() / 100.0,
        social_security_personal: (social_security_personal * 100.0).round() / 100.0,
        housing_fund_personal: (housing_fund_personal * 100.0).round() / 100.0,
        social_security_employer: (social_security_employer * 100.0).round() / 100.0,
        housing_fund_employer: (housing_fund_employer * 100.0).round() / 100.0,
        attendance_deduction: (attendance_deduction * 100.0).round() / 100.0,
        tax_amount: (tax_amount * 100.0).round() / 100.0,
        other_deduction,
        net_salary: (net_salary * 100.0).round() / 100.0,
        status: "calculated".to_string(),
        locked: 0,
        remark: None,
        created_at: None,
        updated_at: None,
    })
}

fn calculate_attendance_deduction(
    att: Option<&AttendanceRecord>,
    daily_salary: f64,
    rules: &std::collections::HashMap<String, f64>,
) -> f64 {
    let att = match att {
        Some(a) => a,
        None => return 0.0,
    };

    let late_penalty = rules.get("late_penalty").copied().unwrap_or(20.0);
    let early_leave_penalty = rules.get("early_leave_penalty").copied().unwrap_or(20.0);
    let personal_leave_rate = rules.get("personal_leave_rate").copied().unwrap_or(1.0);
    let sick_leave_rate = rules.get("sick_leave_rate").copied().unwrap_or(0.5);
    let absent_rate = rules.get("absent_rate").copied().unwrap_or(2.0);

    let late_deduction = (att.late_count as f64) * late_penalty;
    let early_deduction = (att.early_leave_count as f64) * early_leave_penalty;
    let personal_deduction = att.personal_leave_days * daily_salary * personal_leave_rate;
    let sick_deduction = att.sick_leave_days * daily_salary * sick_leave_rate;
    let absent_deduction = att.absent_days * daily_salary * absent_rate;

    late_deduction + early_deduction + personal_deduction + sick_deduction + absent_deduction
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_cumulative_tax_january_equals_monthly() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        insert_default_data(&conn).unwrap();
        // 1 月无历史：累计=当月。收入 105000、无扣除 → 应税 100000 → 100000*0.10-2520=7480（与旧月度算法首月一致）
        let tax =
            calculate_cumulative_tax(&conn, "E001", "2026-01", 105000.0, 0.0, 0.0, 0.0, 5000.0)
                .unwrap();
        assert_eq!(tax, 7480.0);
    }

    #[test]
    fn test_cumulative_tax_progresses_over_months() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        insert_default_data(&conn).unwrap();
        // 1-6 月已存：每月应税 10000、已预扣每月 10000*0.03=300 → 累计已缴 1800
        for m in 1..=6 {
            conn.execute(
                "INSERT INTO salary_monthly_results (salary_month, employee_no, gross_salary, social_security_personal, housing_fund_personal, tax_amount, status, locked)
                 VALUES (?1, 'E002', 15000.0, 0.0, 0.0, 300.0, 'approved', 1)",
                [format!("2026-{m:02}")],
            )
            .unwrap();
        }
        // 7 月同收入：累计应税 70000 → 70000*0.10-2520=4480；已缴 1800 → 当月 2680
        let tax =
            calculate_cumulative_tax(&conn, "E002", "2026-07", 15000.0, 0.0, 0.0, 0.0, 5000.0)
                .unwrap();
        assert_eq!(tax, 2680.0);
    }

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        insert_default_data(&conn).unwrap();
        conn.execute(
            "INSERT INTO employees (employee_no, name, status, base_salary, position_salary, performance_salary, social_security_base, housing_fund_base, special_deduction)
             VALUES ('E001', '张三', 'active', 10000.0, 0.0, 0.0, 0.0, 0.0, 0.0)",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_salary_uses_profile_and_clamp() {
        let conn = setup();
        // 2026 台账：ss_base 8000（超上限 7000 → clamp），单位率 0.24/0.12
        conn.execute(
            "INSERT INTO social_insurance_profiles (employee_no, profile_year, ss_base, hf_base, ss_employer_rate, ss_personal_rate, hf_employer_rate, hf_personal_rate)
             VALUES ('E001', 2026, 8000.0, 8000.0, 0.24, 0.105, 0.12, 0.12)",
            [],
        )
        .unwrap();
        set_social_base_limits(&conn, 4590.0, 7000.0, 0.0, 0.0).unwrap();

        let results = calculate_monthly_salary("2026-01", &conn).unwrap();
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.social_security_personal, 735.0); // 7000 * 0.105
        assert_eq!(r.social_security_employer, 1680.0); // 7000 * 0.24
        assert_eq!(r.housing_fund_employer, 960.0); // 8000 * 0.12（hf 无上限）
        assert_eq!(r.housing_fund_personal, 960.0); // 8000 * 0.12

        // 单位部分随结果落库，可重新读出
        let saved = get_salary_result_by_employee(&conn, "2026-01", "E001").unwrap();
        assert_eq!(saved.social_security_employer, 1680.0);
        assert_eq!(saved.housing_fund_employer, 960.0);
    }

    #[test]
    fn test_salary_falls_back_without_profile() {
        let conn = setup();
        // 无 2027 台账：基数回退 base_salary，费率回退全局默认，单位部分为 0
        let results = calculate_monthly_salary("2027-01", &conn).unwrap();
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.social_security_personal, 1050.0); // 10000 * 0.105
        assert_eq!(r.housing_fund_personal, 1200.0); // 10000 * 0.12
        assert_eq!(r.social_security_employer, 0.0);
        assert_eq!(r.housing_fund_employer, 0.0);

        let saved = get_salary_result_by_employee(&conn, "2027-01", "E001").unwrap();
        assert_eq!(saved.social_security_employer, 0.0);
        assert_eq!(saved.housing_fund_employer, 0.0);
    }
}
