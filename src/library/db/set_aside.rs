use std::io;
use std::path::{Path, PathBuf};

fn with_sidecars(path: &Path) -> [PathBuf; 3] {
    let sidecar = |suffix: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    [path.to_path_buf(), sidecar("-wal"), sidecar("-shm")]
}

pub fn set_aside_path(path: &Path, unix_seconds: i64) -> PathBuf {
    let (year, month, day) = crate::library::civil_from_days(unix_seconds.div_euclid(86_400));
    let second = unix_seconds.rem_euclid(86_400);
    path.with_file_name(format!(
        "library-replaced-{year:04}-{month:02}-{day:02}-{:02}{:02}{:02}Z.db",
        second / 3_600,
        second / 60 % 60,
        second % 60
    ))
}

pub fn set_aside(path: &Path, backup: &Path) -> io::Result<()> {
    let files = with_sidecars(path);
    let targets = with_sidecars(backup);
    if let Some(taken) = targets.iter().find(|target| target.exists()) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", taken.display()),
        ));
    }
    let mut moved: Vec<(&PathBuf, &PathBuf)> = Vec::new();
    for (index, (file, target)) in files.iter().zip(&targets).enumerate() {
        if index > 0 && !file.exists() {
            continue;
        }
        if let Err(error) = std::fs::rename(file, target) {
            for (file, target) in moved.into_iter().rev() {
                if let Err(undo) = std::fs::rename(target, file) {
                    tracing::error!(
                        target: "library.db",
                        "could not move {} back to {}: {undo}",
                        target.display(),
                        file.display()
                    );
                }
            }
            return Err(error);
        }
        moved.push((file, target));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_aside_library_is_named_for_the_utc_second_it_moved() {
        let library = Path::new("/data/mediaflick-desktop/library.db");
        for (seconds, name) in [
            (1_795_123_456, "library-replaced-2026-11-19-212416Z.db"),
            (951_782_400, "library-replaced-2000-02-29-000000Z.db"),
            (1_795_132_799, "library-replaced-2026-11-19-235959Z.db"),
        ] {
            assert_eq!(
                set_aside_path(library, seconds),
                Path::new("/data/mediaflick-desktop").join(name),
                "{seconds}"
            );
        }
    }

    #[test]
    fn the_sidecars_carry_the_database_name_with_wal_and_shm_appended() {
        assert_eq!(
            with_sidecars(Path::new("/data/library-replaced-2026-11-19-212416Z.db")),
            [
                PathBuf::from("/data/library-replaced-2026-11-19-212416Z.db"),
                PathBuf::from("/data/library-replaced-2026-11-19-212416Z.db-wal"),
                PathBuf::from("/data/library-replaced-2026-11-19-212416Z.db-shm"),
            ]
        );
    }
}
