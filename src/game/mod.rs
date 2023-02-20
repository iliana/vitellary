mod linux;
mod macos;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use macos as imp;

use anyhow::Result;
use debug_ignore::DebugIgnore;
use read_process_memory::{Pid, ProcessHandle};
use std::ops::RangeInclusive;
use std::time::Duration;
use zerocopy::FromBytes;

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

impl Game {
    pub(crate) fn attach(pid: Pid) -> Result<Game> {
        let (handle, game_object_addr) = imp::find_game_object(pid)?;
        log::info!("attached to pid {}", pid);
        Ok(Game {
            handle: DebugIgnore(handle),
            game_object_addr,
            old: State::new(),
            cur: State::new(),
        })
    }

    pub(crate) fn update(&mut self) -> Result<Update> {
        let (state, time) = imp::read_game_object(&self.handle, self.game_object_addr)?;
        if self.old.state == u32::MAX {
            self.old = state.clone();
            self.cur = state;
        } else {
            self.old = std::mem::replace(&mut self.cur, state);
        }

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
                "state: {} -> {} @ {:?}",
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

#[derive(Debug, FromBytes)]
struct Timer<T> {
    frames: T,
    seconds: T,
    minutes: T,
    hours: T,
}

impl<T> From<Timer<T>> for Duration
where
    u64: From<T>,
    u32: From<T>,
{
    fn from(timer: Timer<T>) -> Duration {
        Duration::new(
            u64::from(timer.hours) * 3600
                + u64::from(timer.minutes) * 60
                + u64::from(timer.seconds),
            1_000_000_000u32 / 30 * u32::from(timer.frames),
        )
    }
}
