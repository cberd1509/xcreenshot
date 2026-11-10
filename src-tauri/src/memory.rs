//! This process's memory, for the log: how much the app keeps once captures close (#55).

/// Bytes resident and the footprint (what Activity Monitor shows as Memory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub resident: u64,
    pub footprint: u64,
}

/// In whole MB, for the log.
impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "resident {} MB, footprint {} MB",
            mb(self.resident),
            mb(self.footprint)
        )
    }
}

/// Bytes in MB, rounded.
fn mb(bytes: u64) -> u64 {
    bytes.saturating_add(1 << 19) >> 20
}

/// This process's memory now; `None` where it isn't read (Windows for now).
#[cfg(target_os = "macos")]
pub fn now() -> Option<Usage> {
    unsafe extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut [u64; 12]) -> i32;
    }
    /// `RUSAGE_INFO_V0`: `rusage_info_v0`, 96 bytes. As u64s: the UUID (0–1), user and system
    /// time, wakeups (2–5), page-ins, wired size (6–7), then resident size (8) and the
    /// footprint (9).
    const RUSAGE_INFO_V0: i32 = 0;
    let pid = i32::try_from(std::process::id()).ok()?;
    let mut info = [0u64; 12];
    // SAFETY: `info` is a writable buffer the size of `rusage_info_v0`.
    let read = unsafe { proc_pid_rusage(pid, RUSAGE_INFO_V0, &raw mut info) } == 0;
    read.then_some(Usage {
        resident: info[8],
        footprint: info[9],
    })
}

/// This process's memory now; `None` where it isn't read (Windows for now).
#[cfg(not(target_os = "macos"))]
pub fn now() -> Option<Usage> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_whole_megabytes() {
        let usage = Usage {
            resident: 90 << 20,
            footprint: (57 << 20) + (1 << 19),
        };
        assert_eq!(usage.to_string(), "resident 90 MB, footprint 58 MB");
        assert_eq!(mb(u64::MAX), u64::MAX >> 20, "never overflows");
    }

    #[test]
    fn reads_this_process_on_macos() {
        match now() {
            Some(u) => assert!(u.resident > 0 && u.footprint > 0, "{u:?}"),
            None => assert_ne!(std::env::consts::OS, "macos"),
        }
    }
}
