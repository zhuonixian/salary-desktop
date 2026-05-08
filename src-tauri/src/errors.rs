use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Excel读取错误: {0}")]
    ExcelRead(#[from] calamine::Error),

    #[error("Excel写入错误: {0}")]
    ExcelWrite(#[from] rust_xlsxwriter::XlsxError),

    #[error("JSON序列化错误: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("未找到记录: {0}")]
    NotFound(String),

    #[error("OCR识别错误: {0}")]
    Ocr(String),

    #[error("参数错误: {0}")]
    InvalidParam(String),

    #[error("{0}")]
    General(String),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
