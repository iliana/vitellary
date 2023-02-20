#![warn(clippy::pedantic)]
#![allow(clippy::assertions_on_constants, clippy::uninlined_format_args)]

mod game;

use crate::game::{Game, Update};
use anyhow::{bail, Context, Result};
use argh::FromArgs;
use env_logger::Env;
use game::Event;
use read_process_memory::Pid;
use std::io::BufRead;
use std::net::{SocketAddr, TcpListener};
use std::process::Command;
use std::time::Duration;
use tungstenite::Message;

#[allow(clippy::doc_markdown)] // lol
#[derive(FromArgs)]
/// Attach to a VVVVVV process and provide a LiveSplit One server.
struct Args {
    /// enable verbose logging output
    #[argh(switch, short = 'v')]
    verbose: bool,

    /// bind address for WebSocket (default: 127.0.0.1:5555)
    #[argh(option)]
    bind: Option<SocketAddr>,

    /// process ID of a specific VVVVVV process
    #[argh(positional)]
    pid: Option<Pid>,
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    env_logger::Builder::from_env(Env::default().default_filter_or(if args.verbose {
        "vitellary=debug"
    } else {
        "vitellary=info"
    }))
    .init();

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

    let mut game = Game::attach(pid)?;
    let (sender, receiver) = crossbeam_channel::bounded::<Update>(10);

    let bind = args.bind.unwrap_or_else(|| ([127, 0, 0, 1], 5555).into());
    let server = TcpListener::bind(bind).context("failed to bind WebSocket address")?;
    log::info!("listening on ws://{}", bind);
    std::thread::spawn(move || {
        let receiver = receiver;
        for stream in server.incoming() {
            let receiver = receiver.clone();
            std::thread::spawn(move || -> Result<()> {
                let mut websocket = tungstenite::accept(stream.unwrap())?;
                loop {
                    let update = receiver.recv()?;
                    websocket.write_message(Message::Text(format!(
                        "setgametime {}.{:09}",
                        update.time.as_secs(),
                        update.time.subsec_nanos()
                    )))?;
                    if let Some(event) = update.event {
                        websocket.write_message(Message::Text(
                            match event {
                                Event::NewGame => "start",
                                Event::Verdigris
                                | Event::Vermilion
                                | Event::Victoria
                                | Event::Violet
                                | Event::Vitellary
                                | Event::IntermissionOne
                                | Event::IntermissionTwo
                                | Event::GameComplete => "split",
                                Event::Reset => "reset",
                            }
                            .into(),
                        ))?;
                    }
                }
            });
        }
    });

    loop {
        sender.try_send(game.update()?).ok();
        std::thread::sleep(Duration::from_millis(10));
    }
}
