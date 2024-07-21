use thiserror::Error;

pub type CsvlensResult<T> = std::result::Result<T, CsvlensError>;

/// Errors csvlens can have
#[derive(Debug, Error)]
pub enum CsvlensError {
    #[error("Failed to read file: {0}")]
    FileReadError(String),

    #[error("Column name not found: {0}")]
    ColumnNameNotFound(String),

    #[error("Delimiter should not be empty")]
    DelimiterEmptyError,

    #[error("Delimiter should be within the ASCII range: {0} is too fancy")]
    DelimiterNotAsciiError(char),

    #[error("Delimiter should be exactly one character (or \\t), got '{0}'")]
    DelimiterMultipleCharactersError(String),

    #[error(transparent)]
    DelimiterParseError(#[from] std::char::TryFromCharError),

    #[error(transparent)]
    CsvError(#[from] csv::Error),

    #[error(transparent)]
    ArrowError(#[from] arrow::error::ArrowError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
