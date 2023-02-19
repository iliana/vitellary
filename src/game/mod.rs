mod macos;

#[cfg(not(any(target_os = "macos")))]
compile_error!("unsupported target os");

#[cfg(target_os = "macos")]
pub(crate) use macos::*;
