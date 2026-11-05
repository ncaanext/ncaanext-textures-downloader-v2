// Configurable constants for the PS2 Textures Downloader
// Modify these values to adapt this app for other PS2 texture mod projects
// Note: Also update frontend/config.ts to match these values

/// Application title (also update in tauri.conf.json and frontend/config.ts).
/// Also used to build the User-Agent sent to GitHub.
pub const APP_TITLE: &str = "NCAA NEXT Textures Downloader";

/// Repository owner (GitHub username or organization)
pub const REPO_OWNER: &str = "ncaanext";

/// Name of the texture mod repository
pub const REPO_NAME: &str = "ncaa-next-27";

/// Full URL to the git repository
pub const REPO_URL: &str = "https://github.com/ncaanext/ncaa-next-27.git";

/// The target folder name (typically the PS2 game identifier like SLUS-XXXXX)
pub const SLUS_FOLDER: &str = "SLUS-21214";

/// Path within the repo to sparse checkout
pub const SPARSE_PATH: &str = "textures/SLUS-21214";

/// Temporary directory name used during clone
pub const TEMP_DIR_NAME: &str = "_temp_ncaa_repo";

/// Target size of one download chunk during installation, in unique files.
/// The installer splits the download along the repository's folders so a
/// dropped connection only costs the chunk in progress. Texture files average
/// roughly 0.5 MB, so 500 is about 250 MB per chunk; lower it for very large
/// textures or users on slow connections.
pub const CHUNK_MAX_BLOBS: usize = 500;

/// Command-line budget for the folder arguments of one chunk. Windows limits
/// `cmd.exe` command lines to 8191 characters, including the git path.
pub const CHUNK_MAX_ARG_CHARS: usize = 6000;

/// User-Agent sent with GitHub requests, derived from APP_TITLE
pub fn user_agent() -> String {
    APP_TITLE.replace(' ', "-")
}
