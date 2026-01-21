// Configurable constants for the PS2 Textures Downloader
// Modify these values to adapt this app for other PS2 texture mod projects

/// Name of the texture mod repository (displayed to user)
#[allow(dead_code)]
pub const REPO_NAME: &str = "ncaa-next-26";

/// Full URL to the git repository
pub const REPO_URL: &str = "https://github.com/ncaanext/ncaa-next-26.git";

/// The SLUS folder name (PS2 game identifier)
pub const SLUS_FOLDER: &str = "SLUS-21214";

/// Path within the repo to sparse checkout
pub const SPARSE_PATH: &str = "textures/SLUS-21214";

/// Temporary directory name used during clone
pub const TEMP_DIR_NAME: &str = "_temp_ncaa_repo";
