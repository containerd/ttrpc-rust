#![allow(dead_code)]
use std::io::Result;

#[cfg(unix)]
pub const SOCK_ADDR: &str = r"unix:///tmp/ttrpc-test";

#[cfg(windows)]
pub const SOCK_ADDR: &str = r"\\.\pipe\ttrpc-test";

#[cfg(unix)]
pub fn remove_if_sock_exist(sock_addr: &str) -> Result<()> {
    let path = sock_addr
        .strip_prefix("unix://")
        .expect("socket address is not expected");

    if std::path::Path::new(path).exists() {
        std::fs::remove_file(path)?;
    }

    Ok(())
}

#[cfg(windows)]
pub fn remove_if_sock_exist(_sock_addr: &str) -> Result<()> {
    //todo force close file handle?

    Ok(())
}
