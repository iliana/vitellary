use read_process_memory::{CopyAddress, Pid, ProcessHandle};
use std::io::Write;
use std::ops::Range;

#[cfg(target_os = "macos")]
const RANGE: Range<usize> = 0x1_0000_0000..0x2_0000_0000;
#[cfg(not(target_os = "macos"))]
const RANGE: Range<usize> = 0..0x1_0000_0000;

fn main() {
    let pid: Pid = std::env::args()
        .nth(1)
        .expect("usage: vitellary-scan PID")
        .parse()
        .expect("could not parse pid");
    let handle = ProcessHandle::try_from(pid).expect("could not get handle from pid");
    let mut buf = [0; 4096];
    let mut stdout = std::io::stdout().lock();
    for addr in RANGE.step_by(buf.len()) {
        if handle.copy_address(addr, &mut buf).is_ok() {
            stdout.write_all(&buf).unwrap();
        } else {
            stdout.write_all(&[0; 4096]).unwrap();
        }
    }
}
