//! `OneNote Viewer` desktop application composition root.

#![forbid(unsafe_code)]

mod app;
mod worker;
mod workspace;

use anyhow::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    let requested_sources = std::env::args_os().skip(1).map(PathBuf::from).collect();
    app::run(requested_sources)
}
