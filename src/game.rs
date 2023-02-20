#[cfg(not(target_os = "macos"))]
compile_error!("unsupported target os");

pub(crate) use read_process_memory::Pid;

use anyhow::{anyhow, Result};
use debug_ignore::DebugIgnore;
use read_process_memory::{CopyAddress, ProcessHandle};
use regex::bytes::Regex;
use std::ops::RangeInclusive;
use std::time::Duration;
use zerocopy::FromBytes;

const OFFSET_GAMETIME: usize = 0xb8;

const PLAYING_STATES: [u32; 3] = [0, 4, 5];
const SPLITS: [(Event, RangeInclusive<u32>); 8] = [
    (Event::Verdigris, 3006..=3011),
    (Event::Vermilion, 3060..=3065),
    (Event::Victoria, 3040..=3045),
    (Event::Violet, 4091..=4099),
    (Event::Vitellary, 3020..=3025),
    (Event::IntermissionOne, 3085..=3087),
    (Event::IntermissionTwo, 3080..=3082),
    (Event::GameComplete, 3503..=3509),
];

#[derive(Debug)]
pub(crate) struct Game {
    handle: DebugIgnore<ProcessHandle>,
    game_object_addr: usize,
    old: State,
    cur: State,
}

#[derive(Debug, Clone, PartialEq)]
struct State {
    room: (u32, u32),
    gamestate: u32,
    state: u32,
}

impl State {
    fn new() -> State {
        State {
            room: (u32::MAX, u32::MAX),
            gamestate: u32::MAX,
            state: u32::MAX,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Update {
    pub(crate) time: Duration,
    pub(crate) event: Option<Event>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Event {
    NewGame,
    Verdigris,
    Vermilion,
    Victoria,
    Violet,
    Vitellary,
    IntermissionOne,
    IntermissionTwo,
    GameComplete,
    Reset,
}

#[derive(Debug, FromBytes)]
#[repr(C)]
struct GameObject {
    _unused1: [u8; 0x18], // 0x00
    room_x: u32,          // 0x18
    room_y: u32,          // 0x1c
    _unused2: [u8; 0x3c], // 0x20
    state: u32,           // 0x5c
    _unused3: [u8; 0x08], // 0x60
    gamestate: u32,       // 0x68
    _unused4: [u8; 0x38], // 0x6c
    frames: u32,          // 0xa4
    seconds: u32,         // 0xa8
    minutes: u32,         // 0xac
    hours: u32,           // 0xb0
}
const _: () = assert!(std::mem::size_of::<GameObject>() == 0xb4);

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
                        handle: DebugIgnore(handle),
                        game_object_addr: address + start - OFFSET_GAMETIME,
                        old: State::new(),
                        cur: State::new(),
                    };
                    game.update()?;
                    log::info!("attached to pid {}", pid);
                    return Ok(game);
                }
            }
        }

        Err(anyhow!("failed to find game object"))
    }

    pub(crate) fn update(&mut self) -> Result<Update> {
        let mut buf = [0; std::mem::size_of::<GameObject>()];
        self.handle.copy_address(self.game_object_addr, &mut buf)?;
        let game: GameObject = zerocopy::transmute!(buf);
        log::trace!("{:?}", game);

        let state = State {
            room: (game.room_x, game.room_y),
            gamestate: game.gamestate,
            state: game.state,
        };
        if self.old.state == u32::MAX {
            self.old = state.clone();
            self.cur = state;
        } else {
            self.old = std::mem::replace(&mut self.cur, state);
        }

        let time = Duration::new(
            u64::from(game.hours) * 3600 + u64::from(game.minutes) * 60 + u64::from(game.seconds),
            1_000_000_000u32 / 30 * game.frames,
        );

        if self.old.room != self.cur.room {
            log::debug!(
                "room: {:?} -> {:?} @ {:?}",
                self.old.room,
                self.cur.room,
                time
            );
        }
        if self.old.gamestate != self.cur.gamestate {
            log::debug!(
                "gamestate: {} -> {} @ {:?}",
                self.old.gamestate,
                self.cur.gamestate,
                time
            );
        }
        if self.old.state != self.cur.state {
            log::debug!(
                "gamestate: {} -> {} @ {:?}",
                self.old.state,
                self.cur.state,
                time
            );
        }

        if PLAYING_STATES.contains(&self.cur.gamestate)
            && !PLAYING_STATES.contains(&self.old.gamestate)
        {
            return Ok(Update {
                time: Duration::ZERO,
                event: Some(Event::NewGame),
            });
        }
        if !PLAYING_STATES.contains(&self.cur.gamestate)
            && PLAYING_STATES.contains(&self.old.gamestate)
        {
            return Ok(Update {
                time,
                event: Some(Event::Reset),
            });
        }

        // `state` increments to 3006 prior to the switch case that jumps to the correct state. This
        // can cause `Event::Verdigris` to fire one cycle before the correct event. Check we're in
        // the right room ("Murdering Twinmaker" @ (115, 100)) and enforce no event if we're not.
        let event = if self.cur.state == 3006 && self.cur.room != (115, 100) {
            log::debug!("ignoring state 3006");
            None
        } else {
            SPLITS.into_iter().find_map(|(event, range)| {
                (range.contains(&self.cur.state) && !range.contains(&self.old.state))
                    .then_some(event)
            })
        };

        Ok(Update { time, event })
    }
}
