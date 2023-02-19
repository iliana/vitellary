#![warn(clippy::pedantic)]
#![allow(clippy::assertions_on_constants, clippy::uninlined_format_args)]

mod game;

use crate::game::{Game, Pid};
use anyhow::{bail, Context, Result};
use argh::FromArgs;
use std::io::BufRead;
use std::process::Command;

#[allow(clippy::doc_markdown)] // lol
#[derive(FromArgs)]
/// Attach to a VVVVVV process and provide a LiveSplit server.
struct Args {
    /// process ID of a specific VVVVVV process
    #[argh(positional)]
    pid: Option<Pid>,
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let pid = if let Some(pid) = args.pid {
        pid
    } else {
        let output = Command::new("pgrep")
            .args(["-n", "VVVVVV"])
            .output()
            .context("failed to run pgrep")?;
        if output.status.success() {
            output
                .stdout
                .lines()
                .next()
                .expect("pgrep returned 0 with no output")
                .expect("pgrep output invalid UTF-8")
                .parse()?
        } else if output.status.code() == Some(1) {
            bail!("no VVVVVV process found");
        } else {
            bail!("pgrep failed with {}", output.status);
        }
    };

    let game = Game::attach(pid)?;
    println!("{:?}", game);
    Ok(())
}