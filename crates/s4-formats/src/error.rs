use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{what}: unexpected end of data at offset {offset:#x}")]
    Eof { what: &'static str, offset: usize },
    #[error("{0}")]
    Format(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Fail with a formatted `Error::Format`.
#[macro_export]
macro_rules! bail {
    ($($t:tt)*) => { return Err($crate::Error::Format(format!($($t)*))) };
}
