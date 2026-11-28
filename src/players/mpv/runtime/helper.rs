use std::ffi::{c_int, c_ulong};
use std::fs::{File, OpenOptions, Permissions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;

use crate::playback::NativeOverlayFrame;
use crate::players::mpv::external::remove_packaged_cef_preload;
use crate::players::mpv::ipc::short_unix_path;
use crate::preferences::FullscreenBehavior;

use super::wayland::LinuxDisplayBackend;
use super::{LibMpvRuntime, LibmpvProfile};

const RESTART_WINDOW: Duration = Duration::from_secs(30);
const MAX_RESTARTS_IN_WINDOW: usize = 5;
const HELPER_START_TIMEOUT: Duration = Duration::from_secs(10);
const HELPER_OVERLAY_TIMEOUT: Duration = Duration::from_millis(250);
const HELPER_CHILD_READ_TIMEOUT: Duration = Duration::from_millis(100);
const HELPER_FAILSAFE_EXIT_DELAY: Duration = Duration::from_millis(1200);
const MAX_OVERLAY_BYTES: usize = 64 * 1024 * 1024;
const PR_SET_PDEATHSIG: c_int = 1;
const SIGTERM_NUM: c_ulong = 15;
const SIGBUS_NUM: c_int = 7;
const SIGSEGV_NUM: c_int = 11;
const F_SETFD_CMD: c_int = 2;
const WAYLAND_CHILD_FD: c_int = 3;

static HELPER_COUNTER: AtomicU64 = AtomicU64::new(1);

unsafe extern "C" {
    fn prctl(option: c_int, arg2: c_ulong, arg3: c_ulong, arg4: c_ulong, arg5: c_ulong) -> c_int;
    fn getppid() -> i32;
    fn signal(sig: c_int, handler: Option<extern "C" fn(c_int)>) -> usize;
    fn dup2(oldfd: c_int, newfd: c_int) -> c_int;
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
}

#[derive(Debug, Default)]
pub(crate) struct HelperRestartBudget {
    recent: Vec<Instant>,
}

impl HelperRestartBudget {
    pub(crate) fn record_and_allow(&mut self, now: Instant) -> bool {
        self.recent
            .retain(|seen| now.saturating_duration_since(*seen) <= RESTART_WINDOW);
        if self.recent.len() >= MAX_RESTARTS_IN_WINDOW {
            return false;
        }
        self.recent.push(now);
        true
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HelperArgs {
    pub(crate) backend: super::wayland::LinuxDisplayBackend,
    pub(crate) libmpv_path: PathBuf,
    pub(crate) ipc_path: String,
    pub(crate) wid: usize,
    pub(crate) overlay_path: PathBuf,
    pub(crate) control_path: PathBuf,
    pub(crate) parent_pid: u32,
    pub(crate) fullscreen: FullscreenBehavior,
    pub(crate) profile: LibmpvProfile,
}

#[derive(Debug)]
pub(crate) enum HelperOverlayError {
    HelperExited(NativeOverlayFrame, String),
    TimedOut(NativeOverlayFrame),
    Composition(String),
}

pub(crate) fn is_player_helper_invocation() -> bool {
    std::env::args().any(|arg| arg == "--player-helper")
}

pub(crate) fn parse_helper_args<I, S>(args: I) -> Result<HelperArgs, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut values = std::collections::HashMap::<String, String>::new();
    let mut iter = args.into_iter().skip(1);
    while let Some(arg) = iter.next() {
        let flag = arg.as_ref();
        if flag == "--player-helper" {
            continue;
        }
        if !flag.starts_with("--") {
            return Err(format!("unexpected helper argument {flag}"));
        }
        let val = iter
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        values.insert(flag.to_string(), val.as_ref().to_string());
    }
    let req = |key: &str| {
        values
            .get(key)
            .filter(|v| !v.is_empty())
            .cloned()
            .ok_or_else(|| format!("missing {key}"))
    };
    let backend = match values.get("--backend").map(String::as_str) {
        Some("wayland") => LinuxDisplayBackend::Wayland,
        Some("x11") | None => LinuxDisplayBackend::X11,
        Some(other) => return Err(format!("invalid --backend {other}")),
    };
    let wid = req("--wid")?
        .parse::<usize>()
        .map_err(|_| "invalid --wid".to_string())?;
    let parent_pid = req("--parent-pid")?
        .parse::<u32>()
        .map_err(|_| "invalid --parent-pid".to_string())?;
    let fullscreen = match req("--fullscreen")?.as_str() {
        "yes" | "fullscreen" => FullscreenBehavior::Fullscreen,
        "no" | "windowed" => FullscreenBehavior::Windowed,
        other => return Err(format!("invalid --fullscreen {other}")),
    };
    let profile = match req("--profile")?.as_str() {
        "standard" => LibmpvProfile::Standard,
        "svp" => LibmpvProfile::Svp,
        other => return Err(format!("invalid --profile {other}")),
    };
    let overlay_path = match backend {
        LinuxDisplayBackend::X11 => PathBuf::from(req("--overlay-path")?),
        LinuxDisplayBackend::Wayland => values
            .get("--overlay-path")
            .filter(|v| !v.is_empty())
            .map_or_else(PathBuf::new, PathBuf::from),
    };
    Ok(HelperArgs {
        backend,
        libmpv_path: PathBuf::from(req("--libmpv-path")?),
        ipc_path: req("--ipc-path")?,
        wid,
        overlay_path,
        control_path: PathBuf::from(req("--control-path")?),
        parent_pid,
        fullscreen,
        profile,
    })
}

pub(crate) fn run_player_helper() -> i32 {
    let args = match parse_helper_args(std::env::args()) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("player helper argument error: {error}");
            return 2;
        }
    };
    // SAFETY: `PR_SET_PDEATHSIG` requests `SIGTERM` if the parent exits, restoring `SIG_DFL` for synchronous fatal signals lets first-signal faults terminate immediately, and `getppid` takes no arguments.
    unsafe {
        let _ = prctl(PR_SET_PDEATHSIG, SIGTERM_NUM, 0, 0, 0);
        let _ = signal(SIGSEGV_NUM, None);
        let _ = signal(SIGBUS_NUM, None);
        if u32::try_from(getppid()).ok() != Some(args.parent_pid) {
            return 0;
        }
    }

    let stdin_closed = spawn_stdin_watchdog();
    let runtime_result = match args.backend {
        LinuxDisplayBackend::Wayland => LibMpvRuntime::start_wayland_linux(
            &args.libmpv_path,
            &args.ipc_path,
            args.fullscreen,
            args.profile,
        ),
        LinuxDisplayBackend::X11 => LibMpvRuntime::start_embedded_linux(
            &args.libmpv_path,
            &args.ipc_path,
            args.wid,
            args.fullscreen,
            args.profile,
        ),
    };
    let mut runtime = match runtime_result {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("player helper libmpv startup failed: {error}");
            return 1;
        }
    };

    if let Err(error) = serve_helper_control(&args, &mut runtime, &stdin_closed) {
        eprintln!("player helper control error: {error}");
    }
    spawn_exit_failsafe();
    runtime.terminate();
    0
}

fn spawn_stdin_watchdog() -> Arc<AtomicBool> {
    let closed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&closed);
    let _ = thread::Builder::new()
        .name("player-helper-stdin".to_string())
        .spawn(move || {
            let mut buf = [0_u8; 64];
            let mut stdin = std::io::stdin();
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        flag.store(true, Ordering::Release);
                        spawn_exit_failsafe();
                        break;
                    }
                    Ok(_) => {}
                }
            }
        });
    closed
}

fn spawn_exit_failsafe() {
    let _ = thread::Builder::new()
        .name("player-helper-failsafe".to_string())
        .spawn(|| {
            thread::sleep(HELPER_FAILSAFE_EXIT_DELAY);
            std::process::exit(0);
        });
}

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum HelperCommand {
    Overlay {
        #[serde(default)]
        seq: u64,
        w: u16,
        h: u16,
        dw: u16,
        dh: u16,
        len: usize,
    },
    Quit,
}

pub(super) fn read_control_json_line(
    reader: &mut BufReader<UnixStream>,
    buffer: &mut String,
) -> io::Result<Option<String>> {
    match reader.read_line(buffer) {
        Ok(0) => {
            if buffer.trim().is_empty() {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "control channel closed",
                ))
            } else {
                let text = std::mem::take(buffer);
                Ok(Some(text))
            }
        }
        Ok(_) => {
            if !buffer.ends_with('\n') {
                return Ok(None);
            }
            Ok(Some(std::mem::take(buffer)))
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn serve_helper_control(
    args: &HelperArgs,
    runtime: &mut LibMpvRuntime,
    stdin_closed: &AtomicBool,
) -> io::Result<()> {
    let mut overlay_file = if args.overlay_path.as_os_str().is_empty() {
        None
    } else {
        Some(File::open(&args.overlay_path)?)
    };
    let mut stream = UnixStream::connect(&args.control_path)?;
    let ready = json!({
        "event": "ready",
        "vo": runtime.current_vo(),
        "gpu_context": runtime.current_gpu_context(),
    });
    writeln!(stream, "{ready}")?;
    stream.flush()?;

    stream.set_read_timeout(Some(HELPER_CHILD_READ_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    let mut pixels = Vec::new();

    while !stdin_closed.load(Ordering::Acquire) && runtime.is_alive() {
        let Some(raw) = (match read_control_json_line(&mut reader, &mut line) {
            Ok(line) => line,
            Err(_) => break,
        }) else {
            continue;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<HelperCommand>(trimmed) {
            Ok(HelperCommand::Quit) => break,
            Ok(HelperCommand::Overlay {
                seq,
                w,
                h,
                dw,
                dh,
                len,
            }) => {
                let reply = if let Some(file) = overlay_file.as_mut() {
                    apply_overlay_frame(runtime, file, &mut pixels, seq, (w, h), (dw, dh), len)
                } else {
                    json!({
                        "seq": seq,
                        "ok": false,
                        "error": "Wayland helper does not accept X11 pixel overlays",
                    })
                };
                writeln!(stream, "{reply}")?;
                stream.flush()?;
            }
            Err(error) => {
                writeln!(
                    stream,
                    "{}",
                    json!({ "ok": false, "error": error.to_string() })
                )?;
                stream.flush()?;
            }
        }
    }
    Ok(())
}

fn apply_overlay_frame(
    runtime: &mut LibMpvRuntime,
    overlay_file: &mut File,
    pixels: &mut Vec<u8>,
    seq: u64,
    size: (u16, u16),
    display: (u16, u16),
    len: usize,
) -> serde_json::Value {
    let expected = usize::from(size.0) * usize::from(size.1) * 4;
    if len == 0 || len != expected || len > MAX_OVERLAY_BYTES {
        return json!({
            "seq": seq,
            "ok": false,
            "error": "invalid overlay dimensions or buffer length",
        });
    }
    pixels.resize(len, 0);
    if let Err(error) = overlay_file
        .seek(SeekFrom::Start(0))
        .and_then(|_| overlay_file.read_exact(pixels))
    {
        return json!({
            "seq": seq,
            "ok": false,
            "error": format!("overlay read failed: {error}"),
        });
    }
    match runtime.submit_overlay_pixels(pixels, size.0, size.1, display) {
        Ok(()) => json!({ "seq": seq, "ok": true }),
        Err(error) => json!({ "seq": seq, "ok": false, "error": error }),
    }
}

struct HelperSpawnSpec<'a> {
    backend: LinuxDisplayBackend,
    libmpv_path: &'a Path,
    ipc_path: &'a str,
    wid: usize,
    wayland_socket: Option<OwnedFd>,
    overlay: Option<(File, PathBuf)>,
    fullscreen: FullscreenBehavior,
    profile: LibmpvProfile,
}

pub(crate) struct HelperRuntime {
    child: Child,
    stdin: Option<ChildStdin>,
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    overlay_file: Option<File>,
    overlay_path: Option<PathBuf>,
    control_path: PathBuf,
    next_overlay_seq: u64,
    in_flight_seq: Option<u64>,
    pending_reply: String,
    pub(crate) ui_exposes_video: bool,
}

impl HelperRuntime {
    pub(crate) fn start(
        libmpv_path: &Path,
        ipc_path: &str,
        wid: usize,
        fullscreen: FullscreenBehavior,
        profile: LibmpvProfile,
    ) -> io::Result<Self> {
        let dir = preferred_runtime_dir();
        let seq = HELPER_COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let overlay_path = short_unix_path(&dir, &format!("mf-ovr-{pid:x}-{seq:x}.bgra"));
        let _ = std::fs::remove_file(&overlay_path);
        let overlay_file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(&overlay_path)?;
        Self::spawn_helper(HelperSpawnSpec {
            backend: LinuxDisplayBackend::X11,
            libmpv_path,
            ipc_path,
            wid,
            wayland_socket: None,
            overlay: Some((overlay_file, overlay_path)),
            fullscreen,
            profile,
        })
    }

    pub(crate) fn start_wayland(
        libmpv_path: &Path,
        ipc_path: &str,
        wayland_socket: OwnedFd,
        fullscreen: FullscreenBehavior,
        profile: LibmpvProfile,
    ) -> io::Result<Self> {
        Self::spawn_helper(HelperSpawnSpec {
            backend: LinuxDisplayBackend::Wayland,
            libmpv_path,
            ipc_path,
            wid: 0,
            wayland_socket: Some(wayland_socket),
            overlay: None,
            fullscreen,
            profile,
        })
    }

    fn build_helper_command(
        spec: &HelperSpawnSpec<'_>,
        control_path: &Path,
        overlay_path: Option<&PathBuf>,
        pid: u32,
    ) -> io::Result<Command> {
        let exe = std::env::current_exe()?;
        let profile_arg = match spec.profile {
            LibmpvProfile::Standard => "standard",
            LibmpvProfile::Svp => "svp",
        };
        let backend_arg = match spec.backend {
            LinuxDisplayBackend::Wayland => "wayland",
            LinuxDisplayBackend::X11 => "x11",
        };
        let mut cmd = Command::new(exe);
        cmd.arg("--player-helper")
            .arg("--backend")
            .arg(backend_arg)
            .arg("--libmpv-path")
            .arg(spec.libmpv_path)
            .arg("--ipc-path")
            .arg(spec.ipc_path)
            .arg("--wid")
            .arg(spec.wid.to_string())
            .arg("--control-path")
            .arg(control_path)
            .arg("--parent-pid")
            .arg(pid.to_string())
            .arg("--fullscreen")
            .arg(spec.fullscreen.fullscreen_arg())
            .arg("--profile")
            .arg(profile_arg)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        if let Some(path) = overlay_path {
            cmd.arg("--overlay-path").arg(path);
        }
        remove_packaged_cef_preload(&mut cmd);
        if let Some(socket_fd) = spec.wayland_socket.as_ref() {
            let raw_fd = socket_fd.as_raw_fd();
            cmd.env("WAYLAND_SOCKET", WAYLAND_CHILD_FD.to_string())
                .env_remove("WAYLAND_DISPLAY")
                .env_remove("DISPLAY");
            // SAFETY: `raw_fd` remains open in the parent until after `spawn`,
            // and `pre_exec` runs between `fork` and `exec` to publish it as fd 3 without CLOEXEC.
            unsafe {
                cmd.pre_exec(move || {
                    if raw_fd != WAYLAND_CHILD_FD && dup2(raw_fd, WAYLAND_CHILD_FD) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if fcntl(WAYLAND_CHILD_FD, F_SETFD_CMD, 0) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        Ok(cmd)
    }

    fn spawn_helper(spec: HelperSpawnSpec<'_>) -> io::Result<Self> {
        let dir = preferred_runtime_dir();
        let seq = HELPER_COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let control_path = short_unix_path(&dir, &format!("mf-hlp-{pid:x}-{seq:x}.sock"));
        let _ = std::fs::remove_file(&control_path);

        let listener = UnixListener::bind(&control_path)?;
        let _ = std::fs::set_permissions(&control_path, Permissions::from_mode(0o600));
        listener.set_nonblocking(true)?;

        let overlay_path_ref = spec.overlay.as_ref().map(|(_, path)| path.clone());
        let mut cmd =
            Self::build_helper_command(&spec, &control_path, overlay_path_ref.as_ref(), pid)?;
        let (overlay_file, overlay_path) = match spec.overlay {
            Some((file, path)) => (Some(file), Some(path)),
            None => (None, None),
        };
        let wayland_socket = spec.wayland_socket;

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_file(&control_path);
                if let Some(path) = &overlay_path {
                    let _ = std::fs::remove_file(path);
                }
                return Err(error);
            }
        };
        drop(wayland_socket);
        let stdin = child.stdin.take();

        match accept_helper(&listener, &mut child) {
            Ok(stream) => {
                let reader = BufReader::new(stream.try_clone()?);
                Ok(Self {
                    child,
                    stdin,
                    stream,
                    reader,
                    overlay_file,
                    overlay_path,
                    control_path,
                    next_overlay_seq: 1,
                    in_flight_seq: None,
                    pending_reply: String::new(),
                    ui_exposes_video: false,
                })
            }
            Err(error) => {
                if matches!(child.try_wait(), Ok(None)) {
                    let _ = child.kill();
                }
                let _ = child.wait();
                let _ = std::fs::remove_file(&control_path);
                if let Some(path) = &overlay_path {
                    let _ = std::fs::remove_file(path);
                }
                Err(error)
            }
        }
    }

    pub(crate) fn is_alive(&mut self) -> io::Result<bool> {
        self.child.try_wait().map(|status| status.is_none())
    }

    fn read_reply_value(
        &mut self,
        timeout: Option<Duration>,
    ) -> io::Result<Option<serde_json::Value>> {
        self.reader.get_ref().set_read_timeout(timeout)?;
        let raw = match read_control_json_line(&mut self.reader, &mut self.pending_reply) {
            Ok(Some(line)) => line,
            Ok(None) => return Ok(None),
            Err(error) => return Err(error),
        };
        let value = serde_json::from_str::<serde_json::Value>(raw.trim())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(Some(value))
    }

    fn drain_pending_overlay(&mut self) -> Result<(), String> {
        let Some(expected) = self.in_flight_seq else {
            return Ok(());
        };
        loop {
            match self.read_reply_value(Some(Duration::from_millis(5))) {
                Ok(Some(val)) => {
                    let seq = val.get("seq").and_then(serde_json::Value::as_u64);
                    if seq.is_some_and(|seen| seen >= expected) || seq.is_none() {
                        self.in_flight_seq = None;
                        return Ok(());
                    }
                }
                Ok(None) => return Err("previous overlay reply still pending".to_string()),
                Err(error) => return Err(error.to_string()),
            }
        }
    }

    fn await_overlay_reply(
        &mut self,
        seq: u64,
        frame: NativeOverlayFrame,
    ) -> Result<NativeOverlayFrame, HelperOverlayError> {
        let deadline = Instant::now() + HELPER_OVERLAY_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(HelperOverlayError::TimedOut(frame));
            }
            match self.read_reply_value(Some(remaining)) {
                Ok(None) => return Err(HelperOverlayError::TimedOut(frame)),
                Ok(Some(val)) => {
                    let reply_seq = val.get("seq").and_then(serde_json::Value::as_u64);
                    if reply_seq.is_some_and(|seen| seen < seq) {
                        continue;
                    }
                    self.in_flight_seq = None;
                    if val.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
                        self.ui_exposes_video = frame.exposes_video();
                        return Ok(frame);
                    }
                    let message = val
                        .get("error")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("native UI composition failed")
                        .to_string();
                    return Err(HelperOverlayError::Composition(message));
                }
                Err(error) => {
                    return Err(self.classify_io_error(frame, &error));
                }
            }
        }
    }

    pub(crate) fn present_overlay(
        &mut self,
        frame: NativeOverlayFrame,
        display: (u16, u16),
    ) -> Result<NativeOverlayFrame, HelperOverlayError> {
        frame.validate().map_err(HelperOverlayError::Composition)?;
        if display.0 == 0 || display.1 == 0 {
            return Err(HelperOverlayError::Composition(
                "native overlay display dimensions are empty".to_string(),
            ));
        }
        if !self.is_alive().unwrap_or(false) {
            return Err(HelperOverlayError::HelperExited(
                frame,
                "player helper process exited".to_string(),
            ));
        }
        if let Err(error) = self.drain_pending_overlay() {
            if !self.is_alive().unwrap_or(false) {
                return Err(HelperOverlayError::HelperExited(frame, error));
            }
            return Err(HelperOverlayError::TimedOut(frame));
        }
        let Some(overlay_file) = self.overlay_file.as_mut() else {
            return Err(HelperOverlayError::Composition(
                "Wayland helper does not accept X11 pixel overlays".to_string(),
            ));
        };
        if let Err(error) = overlay_file
            .seek(SeekFrom::Start(0))
            .and_then(|_| overlay_file.write_all(&frame.pixels))
            .and_then(|()| overlay_file.flush())
        {
            return Err(HelperOverlayError::HelperExited(frame, error.to_string()));
        }

        let seq = self.next_overlay_seq;
        self.next_overlay_seq = self.next_overlay_seq.wrapping_add(1).max(1);
        let cmd = json!({
            "cmd": "overlay",
            "seq": seq,
            "w": frame.width,
            "h": frame.height,
            "dw": display.0,
            "dh": display.1,
            "len": frame.pixels.len(),
        });
        if let Err(error) = writeln!(self.stream, "{cmd}").and_then(|()| self.stream.flush()) {
            return Err(self.classify_io_error(frame, &error));
        }
        self.in_flight_seq = Some(seq);
        self.await_overlay_reply(seq, frame)
    }

    #[cfg(test)]
    pub(super) fn new_for_test(
        child: Child,
        stream: UnixStream,
        overlay_file: File,
    ) -> io::Result<Self> {
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self {
            child,
            stdin: None,
            stream,
            reader,
            overlay_file: Some(overlay_file),
            overlay_path: None,
            control_path: PathBuf::new(),
            next_overlay_seq: 1,
            in_flight_seq: None,
            pending_reply: String::new(),
            ui_exposes_video: false,
        })
    }

    fn classify_io_error(
        &mut self,
        frame: NativeOverlayFrame,
        error: &io::Error,
    ) -> HelperOverlayError {
        if !self.is_alive().unwrap_or(false) {
            return HelperOverlayError::HelperExited(frame, error.to_string());
        }
        if matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ) {
            return HelperOverlayError::TimedOut(frame);
        }
        HelperOverlayError::HelperExited(frame, error.to_string())
    }

    pub(crate) fn request_graceful_stop(&mut self) {
        let _ = writeln!(self.stream, "{}", json!({ "cmd": "quit" }));
        let _ = self.stream.flush();
        self.stdin.take();
    }

    pub(crate) fn stop(&mut self) {
        self.stdin.take();
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.control_path);
        if let Some(path) = &self.overlay_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for HelperRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept_helper(listener: &UnixListener, child: &mut Child) -> io::Result<UnixStream> {
    let deadline = Instant::now() + HELPER_START_TIMEOUT;
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if let Some(status) = child.try_wait()? {
                    return Err(io::Error::other(format!(
                        "player helper exited during startup ({status})"
                    )));
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "player helper did not connect within 10 seconds",
                    ));
                }
                thread::sleep(Duration::from_millis(15));
            }
            Err(error) => return Err(error),
        }
    };
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(HELPER_START_TIMEOUT))?;
    stream.set_write_timeout(Some(HELPER_OVERLAY_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    let bytes = reader.read_line(&mut line)?;
    if bytes == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "player helper disconnected before ready",
        ));
    }
    let ready: serde_json::Value = serde_json::from_str(line.trim()).map_err(io::Error::other)?;
    if ready.get("event").and_then(serde_json::Value::as_str) != Some("ready") {
        return Err(io::Error::other(format!(
            "unexpected player helper handshake: {}",
            line.trim()
        )));
    }
    let vo = ready
        .get("vo")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let gpu_context = ready
        .get("gpu_context")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    tracing::info!(
        target: "mpv.render",
        pid = child.id(),
        vo,
        context = gpu_context,
        "initialized out-of-process gpu-next player helper"
    );
    stream.set_read_timeout(Some(HELPER_OVERLAY_TIMEOUT))?;
    Ok(stream)
}

fn preferred_runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}
