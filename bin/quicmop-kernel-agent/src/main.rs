use std::process::ExitCode;

use crate::service::Service;

mod cli;
mod config;
mod error;
mod netlink_loader;
mod service;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    (Service::init_and_run()
        .code()
        .unwrap_or(exitcode::UNAVAILABLE) as u8)
        .into()
}
