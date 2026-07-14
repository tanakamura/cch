use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub command: String,
    pub argv: Option<Vec<String>>,
    pub workdir: Option<String>,
    pub share_tmp: Option<bool>,
    pub debug: Option<bool>,
    pub desktop_app: Option<bool>,
    pub dbus: Option<bool>,
    pub minimal_dbus: Option<bool>,
    pub full_dbus: Option<bool>,
    pub dbus_log: Option<bool>,
    pub dbus_talk: Option<Vec<String>>,
    pub dbus_see: Option<Vec<String>>,
    pub dbus_own: Option<Vec<String>>,
    pub dbus_call: Option<Vec<(String, String)>>,
    pub dbus_broadcast: Option<Vec<(String, String)>>,
    pub use_net: Option<bool>,
    pub inherit_path: Option<bool>,
    pub inherit_lib: Option<bool>,
    pub as_uid0: Option<bool>,
    pub inherit_tty: Option<bool>,
    pub bind: Option<Vec<String>>,
    pub bind_to: Option<Vec<(String, String)>>,
    pub dev_bind: Option<Vec<String>>,
    pub env: Option<Vec<(String, String)>>,
    pub caps: Option<Vec<String>>,
}

pub fn has_true(v: &Option<bool>) -> bool {
    v.unwrap_or(false)
}
