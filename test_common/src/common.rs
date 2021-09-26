#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO error")]
    IoError(#[from] std::io::Error),
    #[error("UTF8 error")]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error("Serde error")]
    SerdeError(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
