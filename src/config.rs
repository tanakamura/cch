use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub command: String,
    pub desktop_app: Option<bool>,
    pub use_net: Option<bool>,
    pub inherit_path: Option<bool>,
    pub inherit_lib: Option<bool>,
    pub as_uid0: Option<bool>,
    pub inherit_tty: Option<bool>,
    pub bind: Option<Vec<String>>,
    pub caps: Option<Vec<String>>,
}

pub fn has_true(v: &Option<bool>) -> bool {
    v.unwrap_or(false)
}

