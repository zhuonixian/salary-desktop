mod commands;
mod db;
mod errors;
mod excel;
mod models;
mod ocr;
mod salary;

use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir { file_name: None }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                ])
                .build(),
        )
        .setup(|app| {
            log::info!("Application starting...");

            // Initialize database
            let app_data_dir = match app.path().app_data_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    log::error!("Failed to resolve app data dir: {e}");
                    return Err(Box::new(e) as Box<dyn std::error::Error>);
                }
            };

            log::info!("App data dir: {}", app_data_dir.display());

            // Ensure the directory exists
            if let Err(e) = std::fs::create_dir_all(&app_data_dir) {
                log::error!("Failed to create app data dir: {e}");
            }

            let db_path = app_data_dir.to_str().unwrap_or("salary.db");
            log::info!("Database path: {db_path}");

            let conn = match db::init_db(db_path) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("Failed to initialize database: {e}");
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Database init failed: {e}"),
                    )) as Box<dyn std::error::Error>);
                }
            };

            log::info!("Database initialized successfully");
            app.manage(Mutex::new(conn));

            log::info!("Application setup complete");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Employee management
            commands::get_employees,
            commands::get_employee,
            commands::create_employee,
            commands::update_employee,
            commands::delete_employee,
            commands::search_employees,
            commands::import_employees_excel,
            // Attendance management
            commands::get_attendance_records,
            commands::import_attendance_excel,
            commands::save_attendance_records,
            commands::update_attendance_record,
            // Salary rules
            commands::get_salary_rules,
            commands::update_salary_rule,
            commands::get_tax_rules,
            commands::update_tax_rule,
            // Salary calculation
            commands::calculate_salary,
            commands::get_salary_results,
            commands::update_salary_result,
            commands::lock_salary_results,
            commands::recalculate_employee,
            // OCR
            commands::ocr_recognize,
            commands::get_ocr_batches,
            commands::confirm_ocr_results,
            // Export
            commands::export_salary_detail,
            commands::export_bank_payment_file,
            commands::export_salary_slips,
            commands::export_attendance_summary_file,
            // Dashboard
            commands::get_dashboard_summary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
