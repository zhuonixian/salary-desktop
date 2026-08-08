use rusqlite::Connection;

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

    let social_security_base = if emp.social_security_base > 0.0 {
        emp.social_security_base
    } else {
        base_salary
    };
    let housing_fund_base = if emp.housing_fund_base > 0.0 {
        emp.housing_fund_base
    } else {
        base_salary
    };

    let social_security_personal = social_security_base * social_security_rate;
    let housing_fund_personal = housing_fund_base * housing_fund_rate;

    // Attendance deduction
    let attendance_deduction = calculate_attendance_deduction(att, daily_salary, rules);

    // Tax calculation
    let tax_threshold = rules.get("tax_threshold").copied().unwrap_or(5000.0);
    let special_deduction = emp.special_deduction;

    let taxable_income = gross_salary
        - social_security_personal
        - housing_fund_personal
        - tax_threshold
        - special_deduction;

    let tax_amount = calculate_tax(conn, taxable_income)?;

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
