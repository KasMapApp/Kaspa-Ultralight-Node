use thiserror::Error;

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("rpc client construction: {0}")]
    Construction(String),

    #[error("connection failed: {0}")]
    Connect(String),

    #[error("disconnection failed: {0}")]
    Disconnect(String),

    #[error("remote rpc call returned error: {0}")]
    Remote(String),

    #[error("unexpected response: {0}")]
    UnexpectedResponse(String),

    #[error("header conversion failed: {0}")]
    HeaderConversion(String),

    #[error("the required RPC method is unavailable on this node: {0}")]
    MethodUnavailable(&'static str),

    #[error("exceeded retry budget")]
    RetryExhausted,
}

pub type Result<T> = std::result::Result<T, RpcError>;
