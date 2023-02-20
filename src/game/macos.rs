#![cfg(target_os = "macos")]

use crate::game::State;
use anyhow::{anyhow, Result};
use debug_ignore::DebugIgnore;
use read_process_memory::{CopyAddress, Pid, ProcessHandle};
use regex::bytes::Regex;
use std::time::Duration;
use zerocopy::FromBytes;

#[derive(Debug)]
pub(super) struct Handle {
    process: DebugIgnore<ProcessHandle>,
    addr: usize,
}

#[derive(Debug, FromBytes)]
#[repr(C)]
struct GameObject {
    _unused1: [u8; 0x18],     // 0x00
    room_x: u32,              // 0x18
    room_y: u32,              // 0x1c
    _unused2: [u8; 0x3c],     // 0x20
    state: u32,               // 0x5c
    _unused3: [u8; 0x08],     // 0x60
    gamestate: u32,           // 0x68
    _unused4: [u8; 0x38],     // 0x6c
    timer: super::Timer<u32>, // 0xa4
}
const _: () = assert!(std::mem::size_of::<GameObject>() == 0xa4 + 16);

const OFFSET_GAMETIME: usize = 0xb8;

/// Set up a Mach port to a VVVVVV process and try to find the game object.
///
/// This is the reason this program must run as root on macOS; in order to get a Mach port to a
/// process -- even if it is a child process! -- we must be running as root due to limitations on
/// the `task_for_pid` call.
///
/// Once we have a port, we need to scan the memory space for the game object. VVVVVV's game object
/// is a global starting with v2.3.x, so theoretically it's in the same place every time, but macOS
/// runs PIE executables with ASLR.
///
/// Thanks to the [initial values][init] of `game.savetime` and `game.savearea`, and the
/// [implementation details of short string optimizatzion][sso] in libc++, we can just search for
/// two 3-word buffers that contain "00:00" and "nowhere" next to each other. The start of the game
/// object is a fixed offset before the word containing "00:00".
///
/// [init]: https://github.com/TerryCavanagh/VVVVVV/blob/abe3eb607711909aeb6941a471225867a94510d0/desktop_version/src/Game.cpp#L227
/// [sso]: https://joellaity.com/2020/01/31/string.html
pub(super) fn find_game_object(pid: Pid) -> Result<Handle> {
    let handle = ProcessHandle::try_from(pid).map_err(|_| {
        // The `std::io::Error` returned here is useless, because the read-process-memory crate
        // assumes errno is being set. That's not how this platform works!
        anyhow!(
            "failed to get mach handle for pid {} (are you running as root?)",
            pid
        )
    })?;

    let regex = Regex::new(r"00:00\x00{18}.nowhere").unwrap();
    let mut buf = [0; 4096];
    for address in (0x1_0000_0000..0x1_4000_0000).step_by(
        // Overlap ranges by 5 words just in case it straddles a boundary.
        buf.len() - 0x28,
    ) {
        if handle.copy_address(address, &mut buf).is_ok() {
            if let Some(m) = regex.find(&buf) {
                // macOS libc++ differs in `_LIBCPP_ALTERNATE_STRING_LAYOUT` between x86_64
                // and aarch64; on the former, the first byte contains the is_long bit. We
                // just want the start of the word where "00:00" showed up.
                let start = m.start() - (m.start() % 8);
                return Ok(Handle {
                    process: DebugIgnore(handle),
                    addr: address + start - OFFSET_GAMETIME,
                });
            }
        }
    }

    Err(anyhow!("failed to find game object"))
}

pub(super) fn read_game_object(handle: &Handle) -> Result<(State, Duration)> {
    let mut buf = [0; std::mem::size_of::<GameObject>()];
    handle.process.copy_address(handle.addr, &mut buf)?;
    let game: GameObject = zerocopy::transmute!(buf);
    log::trace!("{:?}", game);
    Ok((
        State {
            room: (game.room_x, game.room_y),
            gamestate: game.gamestate,
            state: game.state,
        },
        game.timer.into(),
    ))
}
