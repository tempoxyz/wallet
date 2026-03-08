use thiserror::Error;

#[derive(Error, Debug)]
pub enum InputError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("Invalid header: {0}")]
    InvalidHeader(String),
    #[error("Invalid output path: {0}")]
    InvalidOutputPath(String),
    #[error("Request body exceeds maximum size of {0} bytes")]
    BodyTooLarge(usize),
    #[error("Request header exceeds maximum size of {0} bytes")]
    HeaderTooLarge(usize),
    #[error("failed to read stdin: {0}")]
    ReadStdin(#[source] std::io::Error),
    #[error("failed to read file '{path}': {source}")]
    ReadFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
}
