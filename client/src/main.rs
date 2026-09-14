#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::process::ExitCode;

use client::program;

fn main() -> ExitCode {
    ExitCode::from(program().0 as u8)
}
