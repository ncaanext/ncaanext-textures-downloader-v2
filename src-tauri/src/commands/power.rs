//! Keeps the computer awake while the app is downloading.
//!
//! Hold a [`KeepAwake`] for the whole of an install or sync. If the computer
//! goes to sleep mid-download the connection drops and the transfer fails.
//!
//! This only prevents sleep caused by inactivity. Sleep the user asks for
//! (closing a laptop lid, choosing Sleep) can't be blocked by an app. Failing
//! to take the lock is not an error: the download still runs, just
//! unprotected.

/// Prevents idle sleep until dropped.
pub struct KeepAwake {
    _guard: Option<imp::Guard>,
}

impl KeepAwake {
    /// `reason` is shown by the OS in power diagnostics (e.g. `powercfg /requests`).
    pub fn new(reason: &str) -> Self {
        KeepAwake { _guard: imp::Guard::new(reason) }
    }
}

/// Windows: a power request, which applies to the whole process (unlike
/// SetThreadExecutionState, which is per thread, while async commands move
/// between threads). Execution-required keeps Modern Standby laptops running;
/// display-required keeps the screen on, as the previous behaviour did.
#[cfg(target_os = "windows")]
mod imp {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Power::{
        PowerClearRequest, PowerCreateRequest, PowerRequestDisplayRequired,
        PowerRequestExecutionRequired, PowerRequestSystemRequired, PowerSetRequest,
        POWER_REQUEST_TYPE,
    };
    use windows::Win32::System::Threading::{
        POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
    };

    const POWER_REQUEST_CONTEXT_VERSION: u32 = 0;

    const REQUESTS: [POWER_REQUEST_TYPE; 3] = [
        PowerRequestSystemRequired,
        PowerRequestExecutionRequired,
        PowerRequestDisplayRequired,
    ];

    pub struct Guard {
        handle: HANDLE,
    }

    // A power request handle isn't tied to the thread that created it.
    unsafe impl Send for Guard {}

    impl Guard {
        pub fn new(reason: &str) -> Option<Self> {
            let mut wide: Vec<u16> = reason.encode_utf16().chain(Some(0)).collect();
            let context = REASON_CONTEXT {
                Version: POWER_REQUEST_CONTEXT_VERSION,
                Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
                Reason: REASON_CONTEXT_0 {
                    SimpleReasonString: PWSTR(wide.as_mut_ptr()),
                },
            };
            // The reason string is copied during this call
            let handle = unsafe { PowerCreateRequest(&context) }.ok()?;
            for request in REQUESTS {
                let _ = unsafe { PowerSetRequest(handle, request) };
            }
            Some(Guard { handle })
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            for request in REQUESTS {
                let _ = unsafe { PowerClearRequest(self.handle, request) };
            }
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }
}

/// macOS and Linux: a helper process holds the lock while it runs. It also
/// watches this app's PID, so the lock can't outlive the app if it crashes.
#[cfg(not(target_os = "windows"))]
mod imp {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    pub struct Guard {
        child: Child,
    }

    impl Guard {
        pub fn new(reason: &str) -> Option<Self> {
            let pid = std::process::id().to_string();
            candidates(reason, &pid).into_iter().find_map(|mut cmd| {
                let mut child = cmd
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .process_group(0)
                    .spawn()
                    .ok()?;
                // A refused lock makes the helper exit at once
                std::thread::sleep(std::time::Duration::from_millis(300));
                match child.try_wait() {
                    Ok(None) => Some(Guard { child }),
                    _ => {
                        let _ = child.wait();
                        None
                    }
                }
            })
        }
    }

    #[cfg(target_os = "macos")]
    fn candidates(_reason: &str, pid: &str) -> Vec<Command> {
        // -d display, -i idle, -m disk, -s system (on AC power); -w exit with the app
        let mut cmd = Command::new("caffeinate");
        cmd.args(["-dims", "-w", pid]);
        vec![cmd]
    }

    #[cfg(not(target_os = "macos"))]
    fn candidates(reason: &str, pid: &str) -> Vec<Command> {
        // Blocking sleep is only granted to the active local session; fall
        // back to blocking idle actions alone, which any session may do.
        ["idle:sleep", "idle"]
            .into_iter()
            .map(|what| {
                let mut cmd = Command::new("systemd-inhibit");
                cmd.args([
                    &format!("--what={}", what),
                    &format!("--who={}", crate::config::APP_TITLE),
                    &format!("--why={}", reason),
                    "--mode=block",
                    "tail",
                    &format!("--pid={}", pid),
                    "-f",
                    "/dev/null",
                ]);
                cmd
            })
            .collect()
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            // Signal the whole group so the inhibitor's child goes too
            let _ = Command::new("kill")
                .args(["-TERM", "--", &format!("-{}", self.child.id())])
                .status();
            let _ = self.child.wait();
        }
    }
}
