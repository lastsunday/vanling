#![cfg(unix)]

use std::{env, os::unix::process::CommandExt, process::Command};

use tracing::{debug, info};

use crate::utils;

#[cold]
pub fn restart_process() -> ! {
    let exe = unsafe { utils::sys::current_exe().expect("program path must be available") };
    let envs = env::vars();
    let args = env::args().skip(1);
    debug!(?exe, ?args, ?envs, "Restart");

    info!("Restart");

    let error = Command::new(exe).args(args).envs(envs).exec();
    panic!("{error:?}");
}
