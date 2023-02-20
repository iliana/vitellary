#![cfg(target_os = "linux")]

use crate::game::common::{GameObject, GAME_OBJECT_SIZE};
use crate::game::State;
use anyhow::Result;
use read_process_memory::{CopyAddress, Pid, ProcessHandle};
use std::time::Duration;

pub(super) type Handle = ProcessHandle;

const ADDRESS: usize = 0x854dc0;

pub(super) fn find_game_object(pid: Pid) -> Result<Handle> {
    Ok(ProcessHandle::try_from(pid)?)
}

pub(super) fn read_game_object(handle: &Handle) -> Result<(State, Duration)> {
    let mut buf = [0; GAME_OBJECT_SIZE];
    handle.copy_address(ADDRESS, &mut buf)?;
    Ok(GameObject::from(buf).into_state())
}
