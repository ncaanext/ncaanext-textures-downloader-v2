use super::chunking::{self, Chunk};
use super::power::KeepAwake;
use crate::config::{
    CHUNK_MAX_ARG_CHARS, CHUNK_MAX_BLOBS, REPO_URL, SLUS_FOLDER, SPARSE_PATH, TEMP_DIR_NAME,
};
use regex::Regex;
use serde::Serialize;
use std::io::{BufReader, Read as IoRead};
use std::path::PathBuf;
#[cfg(not(target_os = "windows"))]
use std::process::{Command, Stdio};
#[cfg(target_os = "windows")]
use std::process::Command;
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Emitter, Window};

// Track running process PIDs so we can kill them on app exit
static RUNNING_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Kill all tracked processes (called on app exit)
pub fn cleanup_processes() {
    if let Ok(pids) = RUNNING_PIDS.lock() {
        for pid in pids.iter() {
            #[cfg(target_os = "windows")]
            {
                // Use taskkill to kill the process tree
                let _ = Command::new("taskkill")
                    .args(["/F", "/T", "/PID", &pid.to_string()])
                    .output();
            }
            #[cfg(not(target_os = "windows"))]
            {
                // On Unix, kill the process group
                let _ = Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
            }
        }
    }
}

#[derive(Clone, Serialize)]
pub struct ProgressPayload {
    pub stage: String,
    pub message: String,
    pub percent: Option<u32>,
}

/// Get the path to git executable
/// On Windows x64, use bundled MinGit if available
/// On Windows ARM, require system git
/// On macOS, use system git
fn get_git_path() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let is_arm = cfg!(target_arch = "aarch64");

        // On x64, check for bundled MinGit first
        if !is_arm {
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(exe_dir) = exe_path.parent() {
                    // Try multiple possible resource paths
                    let paths_to_try = [
                        // Full nested path
                        exe_dir.join("resources").join("mingit").join("x64").join("cmd").join("git.exe"),
                        // Flattened cmd folder
                        exe_dir.join("resources").join("cmd").join("git.exe"),
                        // Direct in resources
                        exe_dir.join("resources").join("git.exe"),
                    ];

                    for mingit_path in &paths_to_try {
                        if mingit_path.exists() {
                            return Ok(mingit_path.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }

        // Fall back to system git
        if Command::new("git").arg("--version").output().is_ok() {
            return Ok("git".to_string());
        }

        // Build error message based on architecture
        if is_arm {
            Err("Git not found. On Windows ARM, please install Git manually from https://git-scm.com/download/win".to_string())
        } else {
            let mut err_msg = String::from("Git not found. Searched locations:\n");
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(exe_dir) = exe_path.parent() {
                    err_msg.push_str(&format!("  - {}\\resources\\mingit\\x64\\cmd\\git.exe\n", exe_dir.display()));
                    err_msg.push_str(&format!("  - {}\\resources\\cmd\\git.exe\n", exe_dir.display()));
                    err_msg.push_str(&format!("  - {}\\resources\\git.exe\n", exe_dir.display()));
                }
            }
            err_msg.push_str("  - System PATH\n");
            err_msg.push_str("\nPlease reinstall the app or install Git from https://git-scm.com/download/win");
            Err(err_msg)
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // On macOS/Linux, check for system git
        if Command::new("git").arg("--version").output().is_ok() {
            return Ok("git".to_string());
        }

        #[cfg(target_os = "macos")]
        return Err("Git not found. Please install Xcode Command Line Tools by running: xcode-select --install".to_string());
        #[cfg(not(target_os = "macos"))]
        return Err("Git not found. Please install git with your distribution's package manager (for example: sudo apt install git)".to_string());
    }
}

/// Check if git is available
#[tauri::command]
pub fn check_git_installed() -> Result<bool, String> {
    match get_git_path() {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Get the git installation error message (for display to user)
#[tauri::command]
pub fn get_git_error() -> String {
    match get_git_path() {
        Ok(_) => String::new(),
        Err(e) => e,
    }
}

/// Strip ANSI escape codes from a string
fn strip_ansi_codes(s: &str) -> String {
    let ansi_re = Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap();
    ansi_re.replace_all(s, "").to_string()
}

/// Detect the stage and percentage from git output
fn detect_git_stage(line: &str) -> (Option<&'static str>, Option<u32>) {
    let percent_re = Regex::new(r"(\d+)%").ok();
    let percent = percent_re
        .as_ref()
        .and_then(|re| re.captures(line))
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse().ok());

    if line.contains("Receiving objects:") {
        return (Some("downloading"), percent);
    }
    if line.contains("Updating files:") {
        return (Some("extracting"), percent);
    }
    if line.contains("Resolving deltas:") {
        return (Some("downloading"), percent);
    }
    if line.contains("Compressing objects:") {
        return (Some("compressing"), percent);
    }
    if line.contains("Enumerating objects:") || line.contains("Counting objects:") {
        return (Some("compressing"), percent);
    }
    if line.contains("remote:") {
        return (Some("compressing"), percent);
    }

    (None, percent)
}

/// Map a git progress percentage into a slice of the overall progress bar.
/// Without a range the raw percentage is used. With one, only "Receiving
/// objects" moves the bar, so per-chunk counters don't make it jump around.
fn scale_percent(line: &str, percent: Option<u32>, range: Option<(u32, u32)>) -> Option<u32> {
    match range {
        None => percent,
        Some((lo, hi)) if line.contains("Receiving objects:") => {
            percent.map(|p| lo + hi.saturating_sub(lo) * p.min(100) / 100)
        }
        Some(_) => None,
    }
}

/// Read output handling both \r and \n as line terminators
/// Git uses \r to update progress on the same line
/// When detect_stages is false, always uses default_stage
/// Returns the last few lines of output for error reporting
fn read_output_with_progress<R: IoRead>(
    reader: R,
    window: &Window,
    default_stage: &str,
    detect_stages: bool,
    recent_lines: Option<Arc<Mutex<Vec<String>>>>,
    percent_range: Option<(u32, u32)>,
) {
    let mut buf_reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    let mut byte = [0u8; 1];

    loop {
        match buf_reader.read(&mut byte) {
            Ok(0) => break, // EOF
            Ok(_) => {
                if byte[0] == b'\r' || byte[0] == b'\n' {
                    if !buffer.is_empty() {
                        if let Ok(line) = String::from_utf8(buffer.clone()) {
                            let line = strip_ansi_codes(line.trim());
                            if !line.is_empty() {
                                // Store recent lines for error reporting
                                if let Some(ref lines) = recent_lines {
                                    if let Ok(mut lines) = lines.lock() {
                                        lines.push(line.clone());
                                        // Keep only the last 10 lines
                                        if lines.len() > 10 {
                                            lines.remove(0);
                                        }
                                    }
                                }

                                let (detected_stage, percent) = detect_git_stage(&line);
                                let percent = scale_percent(&line, percent, percent_range);
                                let stage = if detect_stages {
                                    detected_stage.unwrap_or(default_stage)
                                } else {
                                    default_stage
                                };

                                let _ = window.emit(
                                    "install-progress",
                                    ProgressPayload {
                                        stage: stage.to_string(),
                                        message: line,
                                        percent,
                                    },
                                );
                            }
                        }
                        buffer.clear();
                    }
                } else {
                    buffer.push(byte[0]);
                }
            }
            Err(_) => break,
        }
    }

    // Handle any remaining data in buffer
    if !buffer.is_empty() {
        if let Ok(line) = String::from_utf8(buffer) {
            let line = strip_ansi_codes(line.trim());
            if !line.is_empty() {
                // Store recent lines for error reporting
                if let Some(ref lines) = recent_lines {
                    if let Ok(mut lines) = lines.lock() {
                        lines.push(line.clone());
                        if lines.len() > 10 {
                            lines.remove(0);
                        }
                    }
                }

                let (detected_stage, percent) = detect_git_stage(&line);
                let percent = scale_percent(&line, percent, percent_range);
                let stage = if detect_stages {
                    detected_stage.unwrap_or(default_stage)
                } else {
                    default_stage
                };

                let _ = window.emit(
                    "install-progress",
                    ProgressPayload {
                        stage: stage.to_string(),
                        message: line,
                        percent,
                    },
                );
            }
        }
    }
}

/// Run a git command under a pseudo-terminal (via `script`) on macOS/Linux,
/// so git prints progress as it would in a terminal. If `script` is missing,
/// git runs directly: it still works, just with less progress output.
/// When detect_stages is false, always uses default_stage instead of detecting from output
/// percent_range maps git's progress into part of the overall bar (see scale_percent)
/// Returns Ok(true) on success, Ok(false) on failure with error details, or Err on spawn failure
#[cfg(not(target_os = "windows"))]
fn run_git_with_pty(
    git_path: &str,
    args: &[&str],
    working_dir: &PathBuf,
    window: &Window,
    default_stage: &str,
    detect_stages: bool,
    percent_range: Option<(u32, u32)>,
) -> Result<(bool, String), String> {
    // `script` sends everything (including git's stderr) to its stdout
    let (mut cmd, via_pty) = match pty_command(git_path, args)
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => (child, true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let child = Command::new(git_path)
                .args(args)
                .current_dir(working_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to start command: {}", e))?;
            (child, false)
        }
        Err(e) => return Err(format!("Failed to start command: {}", e)),
    };

    // Collect recent output for error reporting
    let recent_lines = Arc::new(Mutex::new(Vec::<String>::new()));

    let output: Option<Box<dyn IoRead>> = if via_pty {
        cmd.stdout.take().map(|o| Box::new(o) as Box<dyn IoRead>)
    } else {
        cmd.stderr.take().map(|o| Box::new(o) as Box<dyn IoRead>)
    };
    if let Some(output) = output {
        read_output_with_progress(output, window, default_stage, detect_stages, Some(recent_lines.clone()), percent_range);
    }

    let status = cmd
        .wait()
        .map_err(|e| format!("Command failed: {}", e))?;

    // Get recent output for error message
    let error_context = recent_lines.lock()
        .map(|lines| lines.join("\n"))
        .unwrap_or_default();

    Ok((status.success(), error_context))
}

/// `script` invocation that runs git under a pseudo-terminal without saving
/// a transcript. The BSD (macOS) and util-linux versions take different
/// arguments.
#[cfg(target_os = "macos")]
fn pty_command(git_path: &str, args: &[&str]) -> Command {
    let mut cmd = Command::new("script");
    cmd.args(["-q", "/dev/null", git_path]).args(args);
    cmd
}

/// util-linux `script` takes the command as a single shell string, so every
/// argument is single-quoted (folder names contain `&`, `(`, `)` and spaces).
/// SHELL is pinned because `script` runs the string with the user's shell.
/// `-e` passes git's exit code through.
#[cfg(all(unix, not(target_os = "macos")))]
fn pty_command(git_path: &str, args: &[&str]) -> Command {
    let line = std::iter::once(git_path)
        .chain(args.iter().copied())
        .map(|arg| format!("'{}'", arg.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");
    let mut cmd = Command::new("script");
    cmd.args(["-q", "-e", "-c", &line, "/dev/null"]).env("SHELL", "/bin/sh");
    cmd
}

/// Quote an argument for the ConPTY command line. Anything beyond a plain set
/// of characters is quoted, since the command may run through `cmd.exe /c`,
/// where unquoted `&`, `(` and `)` (common in texture folder names) are
/// operators. Paths never contain `"` on Windows.
#[cfg(target_os = "windows")]
fn quote_arg(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=@+".contains(c));
    if plain {
        arg.to_string()
    } else {
        format!("\"{}\"", arg)
    }
}

/// Run a git command on Windows using ConPTY for proper progress output
/// When detect_stages is false, always uses default_stage instead of detecting from output
/// percent_range maps git's progress into part of the overall bar (see scale_percent)
/// Returns Ok((true, _)) on success, Ok((false, error_context)) on failure, or Err on spawn failure
#[cfg(target_os = "windows")]
fn run_git_with_pty(
    git_path: &str,
    args: &[&str],
    working_dir: &PathBuf,
    window: &Window,
    default_stage: &str,
    detect_stages: bool,
    percent_range: Option<(u32, u32)>,
) -> Result<(bool, String), String> {
    use conpty::spawn;
    use std::io::Read as _;

    let working_dir_str = working_dir.to_string_lossy().to_string();

    // Build command arguments
    // For clone command, replace "." destination with full path
    // For other commands, use -C flag to set working directory
    let is_clone = args.first() == Some(&"clone");

    let full_args: Vec<String> = if is_clone {
        // For clone, replace "." with the full path
        args.iter().map(|arg| {
            if *arg == "." {
                format!("\"{}\"", working_dir_str)
            } else {
                quote_arg(arg)
            }
        }).collect()
    } else {
        // For other commands, use -C flag
        let mut v: Vec<String> = vec![
            "-C".to_string(),
            format!("\"{}\"", working_dir_str),
        ];
        for arg in args {
            v.push(quote_arg(arg));
        }
        v
    };

    // Build command line - use cmd.exe /c wrapper when path has spaces
    // ConPTY doesn't handle quoted executable paths correctly
    let command_line = if git_path.contains(' ') {
        format!("cmd.exe /c \"\"{}\" {}\"", git_path, full_args.join(" "))
    } else {
        format!("{} {}", git_path, full_args.join(" "))
    };

    // Spawn process using ConPTY (Windows Pseudo Console)
    // This makes git think it's connected to a real terminal
    let mut proc = spawn(&command_line)
        .map_err(|e| format!("Failed to spawn process with ConPTY: {}", e))?;

    // Track the PID so we can kill it if the app closes
    let pid = proc.pid();
    if let Ok(mut pids) = RUNNING_PIDS.lock() {
        pids.push(pid);
    }

    // Read output from the PTY in a separate thread
    // This prevents blocking if the PTY doesn't send EOF properly
    let output = proc
        .output()
        .map_err(|e| format!("Failed to get process output: {}", e))?;

    let window_clone = window.clone();
    let default_stage_owned = default_stage.to_string();

    // Collect recent output for error reporting
    let recent_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let recent_lines_clone = recent_lines.clone();

    let reader_handle = std::thread::spawn(move || {
        let mut output = output;
        let mut buffer = [0u8; 1];
        let mut line_buffer = Vec::new();

        loop {
            match output.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let byte = buffer[0];
                    if byte == b'\r' || byte == b'\n' {
                        if !line_buffer.is_empty() {
                            if let Ok(line) = String::from_utf8(line_buffer.clone()) {
                                let line = line.trim().to_string();
                                if !line.is_empty() {
                                    // Store recent lines for error reporting
                                    if let Ok(mut lines) = recent_lines_clone.lock() {
                                        lines.push(line.clone());
                                        if lines.len() > 10 {
                                            lines.remove(0);
                                        }
                                    }

                                    let (detected_stage, percent) = detect_git_stage(&line);
                                    let percent = scale_percent(&line, percent, percent_range);
                                    let stage = if detect_stages {
                                        detected_stage.unwrap_or(&default_stage_owned)
                                    } else {
                                        &default_stage_owned
                                    };

                                    let _ = window_clone.emit(
                                        "install-progress",
                                        ProgressPayload {
                                            stage: stage.to_string(),
                                            message: line,
                                            percent,
                                        },
                                    );
                                }
                            }
                            line_buffer.clear();
                        }
                    } else {
                        line_buffer.push(byte);
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Wait for process to exit (this returns even if reader is still running)
    let exit_code = proc
        .wait(None)
        .map_err(|e| format!("Failed to wait for process: {}", e))?;

    // Remove PID from tracking list
    if let Ok(mut pids) = RUNNING_PIDS.lock() {
        pids.retain(|&p| p != pid);
    }

    // Drop proc to close the PTY, which should cause the reader to get EOF
    drop(proc);

    // Give the reader thread a short time to finish reading any buffered output
    // Don't block forever - if it's stuck, just move on
    let join_timeout = std::time::Duration::from_secs(2);
    let start = std::time::Instant::now();
    while !reader_handle.is_finished() && start.elapsed() < join_timeout {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // Don't call join() - if thread is stuck, let it be orphaned

    // Get recent output for error message
    let mut error_context = recent_lines.lock()
        .map(|lines| lines.join("\n"))
        .unwrap_or_default();

    // If command failed, include the command line for debugging
    if exit_code != 0 {
        error_context.push_str(&format!("\n\n[Debug] Command: {}", command_line));
    }

    // Exit code 0 means success
    Ok((exit_code == 0, error_context))
}

/// Emit an install progress event
fn emit_progress(window: &Window, stage: &str, message: &str, percent: Option<u32>) {
    let _ = window.emit(
        "install-progress",
        ProgressPayload {
            stage: stage.to_string(),
            message: message.to_string(),
            percent,
        },
    );
}

/// Run git without a PTY and capture stdout. Ok only when git succeeds.
fn git_capture(git_path: &str, working_dir: &PathBuf, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(git_path);
    cmd.arg("-C").arg(working_dir).args(args);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().map_err(|e| format!("Failed to run git: {}", e))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// List every file under SPARSE_PATH with its blob id. Reads only tree
/// objects, which the partial clone already has, so nothing is downloaded.
fn list_textures(git_path: &str, repo: &PathBuf) -> Result<Vec<u8>, String> {
    git_capture(git_path, repo, &["ls-tree", "-r", "-z", "HEAD", "--", SPARSE_PATH])
}

/// If `repo` holds an interrupted install of this project, clean up after
/// any killed git process and return its file listing. Anything unexpected
/// means "not resumable", and the caller starts over with a fresh clone.
fn resumable_listing(git_path: &str, repo: &PathBuf) -> Option<Vec<u8>> {
    if !repo.join(".git").is_dir() {
        return None;
    }
    let config = |key: &str| {
        git_capture(git_path, repo, &["config", "--get", key])
            .ok()
            .map(|out| String::from_utf8_lossy(&out).trim().to_string())
    };
    if config("remote.origin.url").as_deref() != Some(REPO_URL)
        || config("core.sparseCheckout").as_deref() != Some("true")
    {
        return None;
    }
    clear_stale_git_files(repo);
    // A clone killed after recording the commit but before writing its index
    // passes the checks above, but sparse-checkout then has no entries to
    // check out and silently produces nothing
    let indexed = git_capture(git_path, repo, &["ls-files", "--", SPARSE_PATH])
        .map(|out| !out.is_empty())
        .unwrap_or(false);
    if !indexed {
        return None;
    }
    list_textures(git_path, repo).ok().filter(|listing| !listing.is_empty())
}

/// Remove what a killed git process leaves behind: lock files that make
/// every later command fail, and partial pack downloads that can be
/// gigabytes. Only called while no git process is running in this clone.
fn clear_stale_git_files(repo: &PathBuf) {
    let git_dir = repo.join(".git");
    let _ = fs::remove_file(git_dir.join("index.lock"));
    let _ = fs::remove_file(git_dir.join("shallow.lock"));
    let _ = fs::remove_file(git_dir.join("info").join("sparse-checkout.lock"));
    if let Ok(entries) = fs::read_dir(git_dir.join("objects").join("pack")) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("tmp_") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// Fresh metadata-only clone into `temp_path`, replacing anything there
fn clone_repository(git_path: &str, temp_path: &PathBuf, window: &Window) -> Result<(), String> {
    if temp_path.exists() {
        fs::remove_dir_all(temp_path)
            .map_err(|e| format!("Failed to clean temp directory: {}", e))?;
    }

    // Create temp directory (only on macOS - on Windows, git clone will create it)
    #[cfg(not(target_os = "windows"))]
    fs::create_dir_all(temp_path)
        .map_err(|e| format!("Failed to create temp directory: {}", e))?;

    // Stage 1: Clone with sparse checkout (this is quick - just metadata)
    let _ = window.emit(
        "install-progress",
        ProgressPayload {
            stage: "cloning".to_string(),
            message: "Initializing repository...".to_string(),
            percent: Some(0),
        },
    );

    let (clone_success, clone_output) = run_git_with_pty(
        git_path,
        &[
            "clone",
            // Stored in the clone's config, so it also covers later fetches
            "--config",
            STALL_LOW_SPEED_LIMIT,
            "--config",
            STALL_LOW_SPEED_TIME,
            "--depth=1",
            "--filter=blob:none",
            "--sparse",
            "--progress",
            REPO_URL,
            ".",
        ],
        temp_path,
        window,
        "cloning",
        false, // Don't detect stages - keep showing "Initializing repository..."
        None,
    )?;

    if !clone_success {
        let _ = fs::remove_dir_all(temp_path);
        let error_msg = if clone_output.is_empty() {
            "Git clone has failed. Please check your internet connection.".to_string()
        } else {
            format!("Git clone has failed:\n{}", clone_output)
        };
        return Err(error_msg);
    }

    Ok(())
}

/// Treat a download that stays below 1 KB/s for 60 seconds as a dropped
/// connection. Without this, git waits indefinitely on a stalled connection
/// (Wi-Fi off, network changed), so the retry never happens.
const STALL_LOW_SPEED_LIMIT: &str = "http.lowSpeedLimit=1000";
const STALL_LOW_SPEED_TIME: &str = "http.lowSpeedTime=60";

/// Delays between attempts at one download step; one more attempt than entries
/// (long enough for a laptop's network to reconnect after waking)
const RETRY_DELAYS_SECS: [u64; 3] = [10, 30, 60];

/// Run a download step, retrying after a dropped connection. A failed step
/// changes nothing (git rolls back the sparse patterns), and blobs from
/// earlier steps stay on disk, so a retry only repeats this step.
fn run_git_with_retries(
    git_path: &str,
    args: &[&str],
    repo: &PathBuf,
    window: &Window,
    percent_range: Option<(u32, u32)>,
    what: &str,
) -> Result<(), String> {
    // Passed on every run too, for clones made before the setting existed
    let mut full_args = vec!["-c", STALL_LOW_SPEED_LIMIT, "-c", STALL_LOW_SPEED_TIME];
    full_args.extend_from_slice(args);

    let mut attempt = 0;
    loop {
        clear_stale_git_files(repo);
        let (success, output) = run_git_with_pty(
            git_path,
            &full_args,
            repo,
            window,
            "downloading",
            false,
            percent_range,
        )?;
        if success {
            return Ok(());
        }
        let Some(&delay) = RETRY_DELAYS_SECS.get(attempt) else {
            return Err(output);
        };
        attempt += 1;
        emit_progress(
            window,
            "downloading",
            &format!(
                "Connection interrupted while downloading {}. Retrying in {}s (attempt {} of {})...",
                what,
                delay,
                attempt + 1,
                RETRY_DELAYS_SECS.len() + 1
            ),
            None,
        );
        std::thread::sleep(Duration::from_secs(delay));
    }
}

/// Arguments for a cone-mode `sparse-checkout` command. Git rejects folder
/// names containing `* ? [ ] \` as if they were patterns, failing that chunk
/// on every attempt; with `--skip-checks` it escapes them and matches the
/// names exactly. The flag is only added when a name needs it, because older
/// git versions don't recognise it.
fn sparse_checkout_args<'a>(subcommand: &'a str, dirs: &[&'a str]) -> Vec<&'a str> {
    const GLOB_CHARS: &[char] = &['*', '?', '[', ']', '\\'];
    let mut args = vec!["sparse-checkout", subcommand];
    if dirs.iter().any(|dir| dir.contains(GLOB_CHARS)) {
        args.push("--skip-checks");
    }
    args.extend_from_slice(dirs);
    args
}

/// Short label for a chunk: its first folder (relative to SPARSE_PATH)
fn describe_chunk(chunk: &Chunk) -> String {
    let prefix = format!("{}/", SPARSE_PATH);
    let first = chunk.dirs.first().map(String::as_str).unwrap_or(SPARSE_PATH);
    let first = first.strip_prefix(&prefix).unwrap_or(first);
    match chunk.dirs.len() {
        0 | 1 => first.to_string(),
        n => format!("{} and {} more folder{}", first, n - 1, if n == 2 { "" } else { "s" }),
    }
}

fn interrupted_message(parts_done: usize, total_parts: usize, output: &str) -> String {
    format!(
        "Download interrupted after {} of {} parts. The completed parts have been kept: \
         click Start Installation again to resume where it left off.\n\n{}",
        parts_done, total_parts, output
    )
}

/// Run the git sparse checkout installation
#[tauri::command]
/// Returns the SHA of the installed commit
pub async fn start_installation(textures_dir: String, window: Window) -> Result<String, String> {
    let git_path = get_git_path()?;
    let textures_path = PathBuf::from(&textures_dir);
    let temp_path = textures_path.join(TEMP_DIR_NAME);
    let final_path = textures_path.join(SLUS_FOLDER);

    // Idle sleep drops the connection mid-download
    let _awake = KeepAwake::new("Downloading textures");

    // Emit initial progress
    let _ = window.emit(
        "install-progress",
        ProgressPayload {
            stage: "preparing".to_string(),
            message: "Preparing installation...".to_string(),
            percent: Some(0),
        },
    );

    // Resume an earlier, interrupted download if one is present; otherwise
    // start from a fresh clone (metadata only - file contents come later)
    let listing = match resumable_listing(&git_path, &temp_path) {
        Some(listing) => {
            emit_progress(
                &window,
                "preparing",
                "Found an unfinished download. Resuming where it left off...",
                Some(0),
            );
            listing
        }
        None => {
            clone_repository(&git_path, &temp_path, &window)?;
            // A listing failure only costs the chunking: fall back to one chunk
            list_textures(&git_path, &temp_path).unwrap_or_default()
        }
    };

    // Record the commit actually being installed, so the first sync picks up
    // anything committed after it (including while a resumed download waited)
    let commit_sha = git_capture(&git_path, &temp_path, &["rev-parse", "HEAD"])
        .map(|out| String::from_utf8_lossy(&out).trim().to_string())
        .map_err(|e| format!("Could not read the downloaded commit: {}", e))?;

    // Stage 2: Download file contents in chunks - THIS IS THE MAIN DOWNLOAD
    let mut chunks = chunking::plan_chunks(
        SPARSE_PATH,
        &chunking::parse_ls_tree(&listing),
        CHUNK_MAX_BLOBS,
        CHUNK_MAX_ARG_CHARS,
    );
    if chunks.is_empty() {
        chunks.push(Chunk { dirs: vec![SPARSE_PATH.to_string()], blobs: 1 });
    }
    let total_parts = chunks.len();
    let total_blobs = chunks.iter().map(|c| c.blobs).sum::<usize>().max(1);
    let mut done_blobs = 0;

    emit_progress(
        &window,
        "downloading",
        &format!("Starting download of {} in {} parts...", SPARSE_PATH, total_parts),
        Some(0),
    );

    for (index, chunk) in chunks.iter().enumerate() {
        let lo = (done_blobs * 100 / total_blobs) as u32;
        let hi = ((done_blobs + chunk.blobs) * 100 / total_blobs) as u32;
        let part = format!("part {} of {}", index + 1, total_parts);
        emit_progress(
            &window,
            "downloading",
            &format!("Downloading {}: {}", part, describe_chunk(chunk)),
            Some(lo),
        );

        let dirs: Vec<&str> = chunk.dirs.iter().map(String::as_str).collect();
        let args = sparse_checkout_args("add", &dirs);
        run_git_with_retries(&git_path, &args, &temp_path, &window, Some((lo, hi)), &part)
            .map_err(|output| interrupted_message(index, total_parts, &output))?;

        done_blobs += chunk.blobs;
    }

    // Switch to the plain folder pattern. Everything should already be
    // present, so this normally downloads nothing - but it guarantees the
    // result is complete even if the plan missed something.
    emit_progress(&window, "extracting", "Checking that every file is present...", Some(100));
    run_git_with_retries(
        &git_path,
        &sparse_checkout_args("set", &[SPARSE_PATH]),
        &temp_path,
        &window,
        Some((100, 100)),
        "the remaining files",
    )
    .map_err(|output| interrupted_message(total_parts, total_parts, &output))?;

    // Stage 3: Move folder to final location
    let _ = window.emit(
        "install-progress",
        ProgressPayload {
            stage: "moving".to_string(),
            message: format!("Moving {} to final location...", SLUS_FOLDER),
            percent: Some(0),
        },
    );

    let source_path = temp_path.join("textures").join(SLUS_FOLDER);

    if !source_path.exists() {
        let _ = fs::remove_dir_all(&temp_path);
        return Err(format!(
            "Expected folder {} not found in repository",
            SPARSE_PATH
        ));
    }

    // Move the folder
    fs::rename(&source_path, &final_path)
        .map_err(|e| format!("Failed to move folder to final location: {}", e))?;

    // Stage 4: Cleanup
    let _ = window.emit(
        "install-progress",
        ProgressPayload {
            stage: "cleanup".to_string(),
            message: "Cleaning up temporary files...".to_string(),
            percent: Some(0),
        },
    );

    fs::remove_dir_all(&temp_path)
        .map_err(|e| format!("Failed to clean up temp directory: {}", e))?;

    // Done!
    let _ = window.emit(
        "install-progress",
        ProgressPayload {
            stage: "complete".to_string(),
            message: format!(
                "Installation complete! Textures installed to: {}",
                final_path.display()
            ),
            percent: Some(100),
        },
    );

    Ok(commit_sha)
}
