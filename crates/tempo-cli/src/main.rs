#![forbid(unsafe_code)]
#![deny(unused)]
#![deny(warnings)]

mod installer;
mod launcher;
mod state;

use launcher::{Launcher, LauncherError};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let launcher = Launcher::new();

    let code = match launcher.run(args) {
        Ok(code) => code,
        Err(err) => report_error(err),
    };

    std::process::exit(code);
}

fn report_error(err: LauncherError) -> i32 {
    eprintln!("{err}");
    1
}
