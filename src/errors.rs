use thiserror::Error;

pub type CsvlensResult<T> = std::result::Result<T, CsvlensError>;

/// Errors csvlens can have
#[derive(Debug, Error)]
pub enum CsvlensError {
    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Column name not found: {0}")]
    ColumnNameNotFound(String),

    #[error("Delimiter should not be empty")]
    DelimiterEmpty,

    #[error("Delimiter should be within the ASCII range: {0} is too fancy")]
    DelimiterNotAscii(char),

    #[error("Delimiter should be exactly one character (or \\t), got '{0}'")]
    DelimiterMultipleCharacters(String),

    #[error(transparent)]
    DelimiterParsing(#[from] std::char::TryFromCharError),

    #[error(transparent)]
    Csv(#[from] csv::Error),

    #[error(transparent)]
    Arrow(#[from] arrow::error::ArrowError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Notify(#[from] notify::Error),
}
