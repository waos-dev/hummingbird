use std::path::PathBuf;
#[cfg(not(target_os = "wasi"))]
use std::{ffi::OsStr, sync::OnceLock};

#[cfg(not(target_os = "wasi"))]
use directories::ProjectDirs;

#[cfg(not(target_os = "wasi"))]
static PROJECT_DIRS: OnceLock<ProjectDirs> = OnceLock::new();

#[cfg(not(target_os = "wasi"))]
pub fn project_dirs() -> &'static ProjectDirs {
    PROJECT_DIRS.get_or_init(|| {
        let legacy_dirs = directories::ProjectDirs::from("me", "william341", "muzak")
            .expect("couldn't generate project dirs (secondary)");

        if legacy_dirs.data_dir().exists() {
            return legacy_dirs;
        }

        directories::ProjectDirs::from("org", "mailliw", "hummingbird")
            .expect("couldn't generate project dirs")
    })
}

#[cfg(not(target_os = "wasi"))]
pub fn data_dir() -> PathBuf {
    project_dirs().data_dir().to_path_buf()
}

/// WaOS port: a program's own folder, which WaOS keeps for it across runs and updates.
#[cfg(target_os = "wasi")]
pub fn data_dir() -> PathBuf {
    PathBuf::from("/data")
}

#[cfg(not(target_os = "wasi"))]
pub fn log_dir() -> PathBuf {
    log_dir_in(
        project_dirs(),
        std::env::var_os("HUMMINGBIRD_LOG_DIR").as_deref(),
    )
}

/// WaOS port: in the program's cache folder, unless `HUMMINGBIRD_LOG_DIR` says otherwise.
#[cfg(target_os = "wasi")]
pub fn log_dir() -> PathBuf {
    std::env::var_os("HUMMINGBIRD_LOG_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/cache/logs"))
}

#[cfg(not(target_os = "wasi"))]
fn log_dir_in(dirs: &ProjectDirs, override_dir: Option<&OsStr>) -> PathBuf {
    override_dir
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_dir(dirs))
}

#[cfg(not(target_os = "wasi"))]
fn default_log_dir(dirs: &ProjectDirs) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        dirs.state_dir()
            .unwrap_or_else(|| dirs.data_local_dir())
            .to_path_buf()
    }

    #[cfg(not(target_os = "linux"))]
    {
        dirs.data_local_dir().to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_log_dir_uses_platform_default() {
        let dirs = ProjectDirs::from("org", "mailliw", "hummingbird").unwrap();

        #[cfg(target_os = "linux")]
        assert_eq!(
            default_log_dir(&dirs),
            dirs.state_dir().unwrap().to_path_buf()
        );

        #[cfg(not(target_os = "linux"))]
        assert_eq!(default_log_dir(&dirs), dirs.data_local_dir().to_path_buf());
    }

    #[test]
    fn log_dir_prefers_environment_override() {
        let dirs = ProjectDirs::from("org", "mailliw", "hummingbird").unwrap();
        let override_dir = std::env::temp_dir().join("hummingbird-log-override");

        assert_eq!(
            log_dir_in(&dirs, Some(override_dir.as_os_str())),
            override_dir,
        );
    }

    #[test]
    fn empty_log_dir_override_is_ignored() {
        let dirs = ProjectDirs::from("org", "mailliw", "hummingbird").unwrap();

        assert_eq!(
            log_dir_in(&dirs, Some(OsStr::new(""))),
            default_log_dir(&dirs),
        );
    }
}
