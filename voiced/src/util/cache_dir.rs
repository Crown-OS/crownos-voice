use std::path::PathBuf;

const CACHE_HOME_VARIABLE: &str = "XDG_CACHE_HOME";
const HOME_VARIABLE: &str = "HOME";
const DEFAULT_CACHE_SUBDIR: &str = ".cache";

/// `$XDG_CACHE_HOME`, else `$HOME/.cache`; `None` when neither is an absolute path.
pub fn user_cache_dir() -> Option<PathBuf> {
    let absolute = |variable| {
        std::env::var_os(variable)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    absolute(CACHE_HOME_VARIABLE)
        .or_else(|| absolute(HOME_VARIABLE).map(|home| home.join(DEFAULT_CACHE_SUBDIR)))
}
