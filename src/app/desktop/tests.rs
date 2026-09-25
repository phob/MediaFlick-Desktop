use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "mediaflick-desktop-test-{}",
            crate::app::ids::random_hex(8)
        )))
    }

    fn entry(&self) -> PathBuf {
        self.0
            .join(format!("applications/{APP_DESKTOP_ID}.desktop"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn registers_identity_and_icon_and_updates_moved_executable() -> io::Result<()> {
    let fixture = Fixture::new();
    register_at(
        &fixture.0,
        &[],
        Path::new("/checkout/build/mediaflick-desktop"),
    )?;
    let entry = std::fs::read_to_string(fixture.entry())?;
    assert!(entry.contains("NoDisplay=true\n"));
    assert!(entry.contains("/checkout/build/mediaflick-desktop"));
    let icon = fixture.0.join("mediaflick-desktop/desktop/app-icon.svg");
    assert_eq!(std::fs::read(&icon)?, ICON);
    assert!(entry.contains(&format!("Icon={}\n", icon.display())));
    register_at(
        &fixture.0,
        std::slice::from_ref(&fixture.0),
        Path::new("/checkout/build/mediaflick-desktop"),
    )?;
    assert!(fixture.entry().exists());
    register_at(
        &fixture.0,
        &[],
        Path::new("/moved/build/mediaflick-desktop"),
    )?;
    let updated = std::fs::read_to_string(fixture.entry())?;
    assert!(updated.contains("/moved/build/mediaflick-desktop"));
    assert!(!updated.contains("/checkout/build/mediaflick-desktop"));
    Ok(())
}

#[test]
fn preserves_user_installed_entry() -> io::Result<()> {
    let fixture = Fixture::new();
    write_changed(&fixture.entry(), ENTRY.as_bytes())?;
    register_at(&fixture.0, &[], Path::new("/build/mediaflick-desktop"))?;
    assert_eq!(std::fs::read_to_string(fixture.entry())?, ENTRY);
    assert!(!fixture.0.join("mediaflick-desktop").exists());
    Ok(())
}

#[test]
fn system_install_supersedes_generated_fallback() -> io::Result<()> {
    let fixture = Fixture::new();
    let system = Fixture::new();
    write_changed(&system.entry(), ENTRY.as_bytes())?;
    register_at(
        &fixture.0,
        std::slice::from_ref(&system.0),
        Path::new("/build/app"),
    )?;
    assert!(!fixture.entry().exists());
    register_at(&fixture.0, &[], Path::new("/build/app"))?;
    assert!(fixture.entry().exists());
    register_at(
        &fixture.0,
        std::slice::from_ref(&system.0),
        Path::new("/build/app"),
    )?;
    assert!(!fixture.entry().exists());
    assert_eq!(std::fs::read_to_string(system.entry())?, ENTRY);
    Ok(())
}

#[test]
fn appimage_launcher_uses_persistent_archive() -> io::Result<()> {
    let fixture = Fixture::new();
    let archive = fixture.0.join("MediaFlick.AppImage");
    write_changed(&archive, b"archive")?;
    let mounted = Path::new("/tmp/.mount_MediaFlick/usr/bin/mediaflick-desktop");
    assert_eq!(launch_path(Some(&archive), mounted), archive);
    assert_eq!(
        launch_path(Some(Path::new("relative.AppImage")), mounted),
        mounted
    );
    assert_eq!(
        launch_path(Some(&fixture.0.join("missing")), mounted),
        mounted
    );
    assert_eq!(launch_path(None, mounted), mounted);
    Ok(())
}

#[test]
fn staged_launcher_preloads_only_the_cef_beside_the_executable() -> io::Result<()> {
    let fixture = Fixture::new();
    let executable = fixture.0.join("mediaflick-desktop");
    let icon = fixture.0.join("icon.svg");
    let unstaged = desktop_entry(&executable, &icon)?;
    assert!(!unstaged.contains("LD_PRELOAD"), "{unstaged}");

    let cef = fixture.0.join("libcef.so");
    write_changed(&cef, b"fixture")?;
    let staged = desktop_entry(&executable, &icon)?;
    let directory = fixture.0.display();
    let cef = cef.display();
    for assignment in [
        format!("LD_LIBRARY_PATH={directory}"),
        format!("LD_PRELOAD={cef}"),
        format!("MEDIAFLICK_DESKTOP_CEF_PRELOAD={cef}"),
    ] {
        assert!(staged.contains(&assignment), "{assignment} in {staged}");
    }
    Ok(())
}

#[test]
fn registration_errors_leave_existing_entry_intact() -> io::Result<()> {
    let fixture = Fixture::new();
    register_at(&fixture.0, &[], Path::new("/build/app"))?;
    let entry = std::fs::read(fixture.entry())?;
    assert!(register_at(&fixture.0, &[], Path::new("relative")).is_err());
    assert_eq!(std::fs::read(fixture.entry())?, entry);
    Ok(())
}

#[test]
#[ignore = "requires desktop-file-validate and /usr/bin/python3 with PyGObject"]
fn desktop_shell_resolves_and_launches_registered_identity() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let fixture = Fixture::new();
    // Exercise both desktop-string escaping and Exec argument/field-code
    // escaping through GIO, including characters meaningful to a shell.
    let executable = fixture
        .0
        .join("space = %f $HOME `id` \"quote\" \\ newline\n/app");
    write_changed(
        &executable,
        b"#!/bin/sh\nprintf launched > \"$DESKTOP_TEST_OUTPUT\"\n",
    )?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
    register_at(&fixture.0, &[], &executable)?;
    assert!(
        Command::new("desktop-file-validate")
            .arg(fixture.entry())
            .status()?
            .success()
    );
    let result = Command::new("/usr/bin/python3")
        .args([
            "-c",
            r#"
import os, time
from pathlib import Path
from gi.repository import Gio
app = Gio.DesktopAppInfo.new('io.github.phob.MediaFlickDesktop.desktop')
assert app is not None
assert app.get_name() == 'MediaFlick Desktop'
assert app.get_nodisplay()
assert app.get_startup_wm_class() == 'io.github.phob.MediaFlickDesktop'
assert app.get_icon().get_file().query_exists(None)
assert app.launch([], None)
output = Path(os.environ['DESKTOP_TEST_OUTPUT'])
for _ in range(100):
    if output.exists() and output.read_text() == 'launched':
        break
    time.sleep(0.05)
else:
    raise AssertionError('registered launcher did not execute the exact path')
"#,
        ])
        .env("XDG_DATA_HOME", &fixture.0)
        .env("XDG_DATA_DIRS", fixture.0.join("system"))
        .env("DESKTOP_TEST_OUTPUT", fixture.0.join("launched"))
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
