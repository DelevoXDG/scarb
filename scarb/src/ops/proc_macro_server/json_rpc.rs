use crate::compiler::plugin::proc_macro::ProcMacroHost;
use anyhow::Result;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize)]
pub struct Response {
    pub id: u64,
    pub value: serde_json::Value,
}

#[derive(Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    pub value: serde_json::Value,
}

pub trait Method {
    const METHOD: &str;

    type Params: DeserializeOwned;
    type Response: Serialize;

    fn handle(proc_macros: Arc<ProcMacroHost>, params: Self::Params) -> Result<Self::Response>;
}

#[derive(Serialize)]
pub struct ErrResponse {
    message: String,
}

impl ErrResponse {
    pub fn new(message: String) -> Self {
        Self { message }
    }
}
