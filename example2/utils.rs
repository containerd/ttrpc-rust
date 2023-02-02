#![allow(dead_code)]
use std::fs;
use std::io::Result;
use std::path::Path;

pub const SOCK_ADDR: &str = "unix:///tmp/ttrpc-test";

pub fn remove_if_sock_exist(sock_addr: &str) -> Result<()> {
    let path = sock_addr
        .strip_prefix("unix://")
        .expect("socket address is not expected");

    if Path::new(path).exists() {
        fs::remove_file(&path)?;
    }

    Ok(())
}
