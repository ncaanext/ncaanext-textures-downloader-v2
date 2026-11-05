//! Downloading texture files from the repository over HTTP.
//!
//! Used by sync on every platform. Independent of the app window, so the
//! same engine can serve installs where git isn't available.

use crate::config::{user_agent, REPO_NAME, REPO_OWNER, SPARSE_PATH};
use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use sha1::{Digest, Sha1};
use std::time::Duration;

/// HTTP client for sync requests. Without timeouts a stalled connection
/// (Wi-Fi dropped, network changed) leaves a request waiting forever, and
/// the sync never finishes or fails. The total timeout is per request and
/// generous enough for the largest API responses on a slow connection.
pub(crate) fn http_client() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(300))
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Delays between attempts at downloading one file; one more attempt than entries
const DOWNLOAD_RETRY_DELAYS_SECS: [u64; 2] = [10, 30];

/// Download a file from GitHub raw content, retrying after network errors
/// and server errors. A client error (such as 404) fails immediately.
async fn download_file(
    client: &Client,
    relative_path: &str,
    dest_path: &Path,
    token: &Option<String>,
) -> Result<(), String> {
    let mut attempt = 0;
    loop {
        match download_file_once(client, relative_path, dest_path, token).await {
            Ok(()) => return Ok(()),
            Err((message, retryable)) => match DOWNLOAD_RETRY_DELAYS_SECS.get(attempt) {
                Some(&delay) if retryable => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                }
                _ => return Err(message),
            },
        }
    }
}

/// One download attempt. Errors carry whether a retry could help.
async fn download_file_once(
    client: &Client,
    relative_path: &str,
    dest_path: &Path,
    token: &Option<String>,
) -> Result<(), (String, bool)> {
    let url = format!(
        "https://raw.githubusercontent.com/{}/{}/main/{}/{}",
        REPO_OWNER, REPO_NAME, SPARSE_PATH, relative_path
    );

    let mut req = client
        .get(&url)
        .header("User-Agent", user_agent());

    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {}", t));
    }

    let response = req
        .send()
        .await
        .map_err(|e| (format!("Failed to download file: {}", e), true))?;

    if !response.status().is_success() {
        return Err((
            format!("Failed to download {}: HTTP {}", relative_path, response.status()),
            !response.status().is_client_error(),
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| (format!("Failed to read file content: {}", e), true))?;

    // Ensure parent directory exists
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| (format!("Failed to create directory: {}", e), false))?;
    }

    fs::write(dest_path, &bytes)
        .map_err(|e| (format!("Failed to write file: {}", e), false))?;

    Ok(())
}

/// Whether a path is protected from sync: anything in a user-customs folder
/// (the user's own textures, which the app must never touch) and hidden
/// files or folders
pub(crate) fn should_skip_path(path: &str) -> bool {
    // Skip user-customs folder
    if path.contains("user-customs") {
        return true;
    }
    // Skip hidden files/directories (starting with .)
    for component in path.split('/') {
        if component.starts_with('.') {
            return true;
        }
    }
    false
}

/// Git blob id of a file on disk
fn git_blob_sha(path: &Path) -> Option<String> {
    let data = fs::read(path).ok()?;
    let mut hasher = Sha1::new();
    hasher.update(format!("blob {}\0", data.len()).as_bytes());
    hasher.update(&data);
    Some(hex::encode(hasher.finalize()))
}

/// Whether two paths refer to the same file on disk. Copying a file onto
/// itself truncates it, so this has to recognise aliases (case differences on
/// case-insensitive filesystems, links), not just identical path strings.
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(ma), Ok(mb)) = (fs::metadata(a), fs::metadata(b)) {
            return ma.dev() == mb.dev() && ma.ino() == mb.ino();
        }
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// How many files download at once. Each raw download spends most of its
/// time waiting for the server to start responding, so running several at
/// once is what makes large downloads fast. Higher values stop helping once
/// the connection is saturated, and on a slow connection split the bandwidth
/// so thinly that the largest textures could hit the request timeout.
const DOWNLOAD_CONCURRENCY: usize = 8;

/// One file to download: its path in the repository, where it goes locally,
/// and its blob id when known.
pub(crate) struct DownloadJob {
    pub path: String,
    pub dest: PathBuf,
    pub sha: Option<String>,
}

/// Download files in parallel. Jobs that share a blob id (texture packs reuse
/// identical files heavily) are downloaded once and copied to the other
/// destinations. `on_progress(done, total, path)` runs after each file.
/// Stops at the first error; files already written stay in place, so a
/// re-run only fetches what's still missing.
pub(crate) async fn download_files(
    client: &Client,
    jobs: Vec<DownloadJob>,
    token: &Option<String>,
    mut on_progress: impl FnMut(u32, u32, &str),
) -> Result<u32, String> {
    let total = jobs.len() as u32;

    // Group by blob id; jobs without one download on their own
    let mut groups: Vec<Vec<DownloadJob>> = Vec::new();
    let mut group_of: HashMap<String, usize> = HashMap::new();
    for job in jobs {
        let existing = job.sha.as_ref().and_then(|sha| group_of.get(sha).copied());
        match existing {
            Some(index) => groups[index].push(job),
            None => {
                if let Some(sha) = &job.sha {
                    group_of.insert(sha.clone(), groups.len());
                }
                groups.push(vec![job]);
            }
        }
    }

    let mut transfers = stream::iter(groups.into_iter().map(|group| async move {
        // Backstop: the job lists already exclude protected paths
        if let Some(job) = group.iter().find(|job| should_skip_path(&job.path)) {
            return Err(format!("Refusing to write protected path: {}", job.path));
        }

        let first = &group[0];
        download_file(client, &first.path, &first.dest, token).await?;

        // Copies are only safe if the download has the content the jobs were
        // planned for; if the repository changed in between, each path is
        // downloaded on its own instead
        let rest = &group[1..];
        let verified = !rest.is_empty()
            && first.sha.as_deref().is_some()
            && git_blob_sha(&first.dest).as_deref() == first.sha.as_deref();
        for copy in rest {
            // Two jobs can resolve to the same local file; it already holds
            // the downloaded content, and copying it onto itself would
            // truncate it
            if same_file(&first.dest, &copy.dest) {
                continue;
            }
            if verified {
                if let Some(parent) = copy.dest.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create directory: {}", e))?;
                }
                fs::copy(&first.dest, &copy.dest)
                    .map_err(|e| format!("Failed to copy {} to {}: {}", first.path, copy.path, e))?;
            } else {
                download_file(client, &copy.path, &copy.dest, token).await?;
            }
        }
        Ok::<_, String>(group)
    }))
    .buffer_unordered(DOWNLOAD_CONCURRENCY);

    let mut done: u32 = 0;
    while let Some(result) = transfers.next().await {
        for job in result? {
            done += 1;
            on_progress(done, total, &job.path);
        }
    }
    Ok(done)
}
