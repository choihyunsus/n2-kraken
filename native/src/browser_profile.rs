// Own only this launch's new temporary profile; never sweep existing Chrome directories.
use std::{
    io,
    path::{Path, PathBuf},
};
pub(super) struct BrowserProfile {
    root: PathBuf,
    path: PathBuf,
}
impl BrowserProfile {
    pub(super) fn create() -> io::Result<Self> {
        let root = std::env::temp_dir().canonicalize()?;
        let path = root.join(format!("n2-kraken-chrome-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path)?;
        Ok(Self { root, path })
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    fn remove(&self) -> io::Result<()> {
        let resolved = match self.path.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if resolved != self.path || resolved.parent() != Some(self.root.as_path()) {
            return Err(io::Error::other(
                "Browser profile path changed; cleanup refused",
            ));
        }
        std::fs::remove_dir_all(resolved)
    }
}
impl Drop for BrowserProfile {
    fn drop(&mut self) {
        const LAST_ATTEMPT: usize = 5;
        // Terminating the owned process can briefly leave Windows file handles closing.
        for attempt in 0..=LAST_ATTEMPT {
            match self.remove() {
                Ok(()) => return,
                Err(error) if attempt == LAST_ATTEMPT => {
                    tracing::warn!("Owned Chrome profile cleanup failed: {error}");
                    return;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_only_the_new_owned_profile() {
        let first = BrowserProfile::create().unwrap();
        let second = BrowserProfile::create().unwrap();
        let old_path = first.path().to_owned();
        std::fs::write(first.path().join("fixture"), "owned").unwrap();
        drop(first);
        assert!(!old_path.exists());
        assert!(second.path().is_dir());
    }
}
