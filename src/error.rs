use std::io;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Source(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Drift(String),
    #[error("{operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("{0}")]
    Internal(String),
}

impl Error {
    pub fn code(&self) -> u8 {
        match self {
            Self::Invalid(_) => 2,
            Self::Source(_) => 3,
            Self::Conflict(_) => 4,
            Self::Drift(_) => 5,
            Self::Io { .. } | Self::Internal(_) => 1,
        }
    }
}

pub trait IoContext<T> {
    fn context(self, operation: &'static str) -> Result<T>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn context(self, operation: &'static str) -> Result<T> {
        self.map_err(|source| Error::Io { operation, source })
    }
}
