#![cfg(target_os = "macos")]

pub(crate) use read_process_memory::Pid;

use anyhow::{anyhow, Result};
use read_process_memory::{CopyAddress, ProcessHandle};
use regex::bytes::Regex;
use std::fmt::{self, Debug};
use zerocopy::FromBytes;

const OFFSET_GAMETIME: usize = 0xb8;

pub(crate) struct Game {
    handle: ProcessHandle,
    game_object_addr: usize,
    game_object: GameObject,
    old_state: u32,
}

impl Debug for Game {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Game")
            .field("game_object_addr", &self.game_object_addr)
            .field("game_object", &self.game_object)
            .field("old_state", &self.old_state)
            .finish_non_exhaustive()
    }
}

#[derive(FromBytes)]
#[repr(C)]
struct GameObject {
    _unused1: [u8; 0x5c], // 0x00
    state: u32,           // 0x5c
    _unused2: [u8; 0x44], // 0x60
    frames: u32,          // 0xa4
    seconds: u32,         // 0xa8
    minutes: u32,         // 0xac
    hours: u32,           // 0xb0
}

const _: () = assert!(std::mem::size_of::<GameObject>() == 0xb4);

impl Debug for GameObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("state", &self.state)
            .field("frames", &self.frames)
            .field("seconds", &self.seconds)
            .field("minutes", &self.minutes)
            .field("hours", &self.hours)
            .finish_non_exhaustive()
    }
}

impl Default for GameObject {
    fn default() -> Self {
        Self {
            _unused1: [0; 0x5c],
            state: 0,
            _unused2: [0; 0x44],
            frames: 0,
            seconds: 0,
            minutes: 0,
            hours: 0,
        }
    }
}

impl Game {
    /// Set up a Mach port to a VVVVVV process and try to find the game object.
    ///
    /// This is the reason the program must be run as root; in order to get a Mach port to a process
    /// -- even if it is a child process! -- we must be running as root due to limitations on the
    /// `task_for_pid` call.
    ///
    /// Once we have a port, we need to scan the memory space for the game object. VVVVVV's game
    /// object is a global starting with v2.3.x, so theoretically it's in the same place every time,
    /// but macOS runs PIE executables with ASLR.
    ///
    /// Thanks to the [initial values][init] of `game.savetime` and `game.savearea`, and the
    /// [implementation details of short string optimizatzion][sso] in libc++, we can just search
    /// for two 3-word buffers that contain "00:00" and "nowhere" next to each other. The start of
    /// the game object is a fixed offset before the word containing "00:00".
    ///
    /// [init]: https://github.com/TerryCavanagh/VVVVVV/blob/abe3eb607711909aeb6941a471225867a94510d0/desktop_version/src/Game.cpp#L227
    /// [sso]: https://joellaity.com/2020/01/31/string.html
    pub(crate) fn attach(pid: Pid) -> Result<Game> {
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

                    let mut game = Game {
                        handle,
                        game_object_addr: address + start - OFFSET_GAMETIME,
                        game_object: GameObject::default(),
                        old_state: 0,
                    };
                    game.update()?;
                    game.old_state = game.game_object.state;
                    return Ok(game);
                }
            }
        }

        Err(anyhow!("failed to find game object"))
    }

    pub(crate) fn update(&mut self) -> Result<()> {
        self.old_state = self.game_object.state;

        let mut buf = [0; std::mem::size_of::<GameObject>()];
        self.handle.copy_address(self.game_object_addr, &mut buf)?;
        self.game_object = zerocopy::transmute!(buf);

        Ok(())
    }
}
