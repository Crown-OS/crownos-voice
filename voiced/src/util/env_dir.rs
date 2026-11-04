use std::path::PathBuf;

/// The directory named by environment variable `variable`, else `default`.
pub fn env_dir(variable: &str, default: &str) -> PathBuf {
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(default), PathBuf::from)
}
