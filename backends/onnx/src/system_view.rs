use std::ffi::OsString;
use std::path::Path;

/// The parts of the filesystem that execution-provider detection reads.
pub(crate) trait SystemView {
    fn exists(&self, path: &Path) -> bool;
    fn entry_names(&self, dir: &Path) -> Vec<OsString>;
    /// File name of what the symlink at `link` points to.
    fn link_target_name(&self, link: &Path) -> Option<OsString>;
}

/// The real filesystem.
pub(crate) struct HostSystem;

impl SystemView for HostSystem {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn entry_names(&self, dir: &Path) -> Vec<OsString> {
        std::fs::read_dir(dir).map_or_else(
            |_| Vec::new(),
            |entries| entries.flatten().map(|entry| entry.file_name()).collect(),
        )
    }

    fn link_target_name(&self, link: &Path) -> Option<OsString> {
        Some(std::fs::read_link(link).ok()?.file_name()?.to_owned())
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use super::SystemView;

    /// A filesystem of listed files and device-to-driver links.
    #[derive(Default)]
    pub(crate) struct FakeSystem {
        files: Vec<PathBuf>,
        driver_links: Vec<(PathBuf, &'static str)>,
    }

    impl FakeSystem {
        pub(crate) fn with_files(mut self, paths: &[&str]) -> Self {
            self.files.extend(paths.iter().map(PathBuf::from));
            self
        }

        /// Adds a sysfs class entry such as `/sys/class/accel/accel0` bound to `driver`.
        pub(crate) fn with_device(mut self, class_entry: &str, driver: &'static str) -> Self {
            let entry = PathBuf::from(class_entry);
            self.driver_links
                .push((entry.join("device/driver"), driver));
            self.files.push(entry);
            self
        }
    }

    impl SystemView for FakeSystem {
        fn exists(&self, path: &Path) -> bool {
            self.files.iter().any(|file| file.starts_with(path))
        }

        fn entry_names(&self, dir: &Path) -> Vec<OsString> {
            self.files
                .iter()
                .filter(|file| file.parent() == Some(dir))
                .filter_map(|file| file.file_name().map(ToOwned::to_owned))
                .collect()
        }

        fn link_target_name(&self, link: &Path) -> Option<OsString> {
            self.driver_links
                .iter()
                .find(|(path, _)| path == link)
                .map(|(_, driver)| OsString::from(driver))
        }
    }
}
