// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Early diagnostic: write to a temp file to verify Rust code is running
    if let Some(dir) = dirs::data_dir() {
        let log_path = dir.join("com.salary.desktop").join("startup.log");
        let _ = std::fs::create_dir_all(log_path.parent().unwrap());
        let msg = format!(
            "[{}] main() started\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")
        );
        let _ = std::fs::write(&log_path, &msg);
        let _ = std::fs::write(
            dir.join("com.salary.desktop").join("startup.log"),
            format!("{msg}log_path: {}\n", log_path.display()),
        );
    }

    // Also write to a well-known temp location
    let tmp_log = std::env::temp_dir().join("salary-desktop-startup.log");
    let _ = std::fs::write(
        &tmp_log,
        format!(
            "[{}] salary-desktop main() started\nexe: {:?}\ntemp_dir: {:?}\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
            std::env::current_exe(),
            std::env::temp_dir(),
        ),
    );

    app_lib::run();

    // Write after run() returns (should not happen normally)
    let _ = std::fs::write(
        std::env::temp_dir().join("salary-desktop-startup.log"),
        format!(
            "[{}] app_lib::run() returned or panicked\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")
        ),
    );
}
