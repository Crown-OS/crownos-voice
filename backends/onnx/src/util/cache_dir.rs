use std::path::PathBuf;

const CACHE_SUBDIR: &str = "crownos/voice";

/// `$XDG_CACHE_HOME/crownos/voice/<name>` (or under `~/.cache`), created on demand. `None` when
/// it cannot be created or is not UTF-8, in which case compiled engines are simply not cached.
pub(crate) fn cache_dir(name: &str) -> Option<String> {
    let root = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    let dir = root.join(CACHE_SUBDIR).join(name);
    std::fs::create_dir_all(&dir).ok()?;
    dir.into_os_string().into_string().ok()
}
