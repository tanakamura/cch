use rustache::{HashBuilder, Render};
use std::io::Cursor;

use crate::config::{Config,has_true};

pub struct Template<'a> {
    tbl: HashBuilder<'a>,
}

impl<'a> Template<'a> {
    pub fn new(config: &Config, base_dir: &std::path::Path) -> Template<'a> {
        let mut tbl = HashBuilder::new().insert("base_dir", base_dir.to_str().unwrap());

        if has_true(& config.desktop_app) {
            let xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR");

            if let Ok(xrd) = xdg_runtime_dir {
                tbl = tbl.insert("XDG_RUNTIME_DIR", xrd);
            }
        }

        let home = home::home_dir().unwrap();
        tbl = tbl.insert("HOME", home.to_str().unwrap());

        Template { tbl }
    }
}

pub fn substitute(str: &str, template: &Template) -> String {
    let mut out = Cursor::new(Vec::new());
    template.tbl.render(str, &mut out).unwrap();
    String::from_utf8(out.into_inner()).unwrap()
}
