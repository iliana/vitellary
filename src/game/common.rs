use crate::game::State;
use std::time::Duration;
use zerocopy::FromBytes;

#[derive(Debug, FromBytes)]
#[repr(C)]
pub(super) struct GameObject {
    _unused1: [u8; 0x18], // 0x00
    room_x: u32,          // 0x18
    room_y: u32,          // 0x1c
    _unused2: [u8; 0x3c], // 0x20
    state: u32,           // 0x5c
    _unused3: [u8; 0x08], // 0x60
    gamestate: u32,       // 0x68
    _unused4: [u8; 0x38], // 0x6c
    timer: Timer<u32>,    // 0xa4
}
pub(super) const GAME_OBJECT_SIZE: usize = std::mem::size_of::<GameObject>();
const _: () = assert!(GAME_OBJECT_SIZE == 0xa4 + 16);

impl GameObject {
    pub(super) fn into_state(self) -> (State, Duration) {
        log::trace!("{:?}", self);
        (
            State {
                room: (self.room_x, self.room_y),
                gamestate: self.gamestate,
                state: self.state,
            },
            self.timer.into(),
        )
    }
}

impl From<[u8; GAME_OBJECT_SIZE]> for GameObject {
    fn from(buf: [u8; GAME_OBJECT_SIZE]) -> Self {
        zerocopy::transmute!(buf)
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
