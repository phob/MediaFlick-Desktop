use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use super::{IpcWorker, MpvEvent};

pub(in crate::players::mpv) const EVENT_CONNECTION: usize = 0;
pub(in crate::players::mpv) const COMMAND_CONNECTION: usize = 1;

const OBSERVED_PROPERTIES: usize = 15;

#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;
#[cfg(windows)]
type Stream = std::fs::File;

pub(in crate::players::mpv) struct FakeMpv {
    path: String,
    received: Receiver<(usize, Value)>,
    events: Arc<Mutex<Option<Stream>>>,
    sessions: Arc<AtomicUsize>,
    stopping: Arc<AtomicBool>,
    server: thread::JoinHandle<()>,
}

impl FakeMpv {
    pub(in crate::players::mpv) fn start(
        reply: impl Fn(&Value) -> Vec<Value> + Send + Sync + 'static,
    ) -> Self {
        let path = super::make_ipc_path();
        let (received_tx, received) = mpsc::channel();
        let events = Arc::new(Mutex::new(None));
        let sessions = Arc::new(AtomicUsize::new(0));
        let stopping = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready) = mpsc::channel();
        let server_path = path.clone();
        let server_events = Arc::clone(&events);
        let server_sessions = Arc::clone(&sessions);
        let server_stopping = Arc::clone(&stopping);
        let reply = Arc::new(reply);
        let server = thread::spawn(move || {
            let mut listener = Listener::bind(&server_path);
            let _ = ready_tx.send(());
            let mut command_threads = Vec::new();
            while !server_stopping.load(Ordering::SeqCst) {
                let Some(event_stream) = listener.accept(&server_stopping) else {
                    break;
                };
                let mut observers = BufReader::new(clone(&event_stream));
                let mut line = String::new();
                let mut observed = true;
                for _ in 0..OBSERVED_PROPERTIES {
                    line.clear();
                    if observers.read_line(&mut line).unwrap_or(0) == 0 {
                        observed = false;
                        break;
                    }
                    if let Ok(command) = serde_json::from_str(&line) {
                        let _ = received_tx.send((EVENT_CONNECTION, command));
                    }
                }
                drop(observers);
                if !observed {
                    continue;
                }
                *server_events.lock().expect("event stream") = Some(event_stream);

                let Some(command_stream) = listener.accept(&server_stopping) else {
                    break;
                };
                server_sessions.fetch_add(1, Ordering::SeqCst);
                let tx = received_tx.clone();
                let reply = Arc::clone(&reply);
                command_threads.push(thread::spawn(move || {
                    let mut commands = BufReader::new(clone(&command_stream));
                    let mut replies = command_stream;
                    let mut line = String::new();
                    loop {
                        line.clear();
                        if commands.read_line(&mut line).unwrap_or(0) == 0 {
                            return;
                        }
                        let Ok(command) = serde_json::from_str::<Value>(&line) else {
                            continue;
                        };
                        for answer in reply(&command) {
                            let mut text = answer.to_string();
                            text.push('\n');
                            if replies.write_all(text.as_bytes()).is_err() {
                                return;
                            }
                        }
                        let _ = replies.flush();
                        let _ = tx.send((COMMAND_CONNECTION, command));
                    }
                }));
            }
            for handle in command_threads {
                let _ = handle.join();
            }
        });
        ready
            .recv_timeout(Duration::from_secs(5))
            .expect("fake mpv IPC endpoint is ready");
        Self {
            path,
            received,
            events,
            sessions,
            stopping,
            server,
        }
    }

    pub(in crate::players::mpv) fn path(&self) -> &str {
        &self.path
    }

    pub(in crate::players::mpv) fn session_count(&self) -> usize {
        self.sessions.load(Ordering::SeqCst)
    }

    pub(in crate::players::mpv) fn connect(&self) -> (IpcWorker, Receiver<MpvEvent>) {
        IpcWorker::start(&self.path).expect("connect to fake mpv")
    }

    pub(in crate::players::mpv) fn next_command_on(&self, connection: usize) -> Value {
        loop {
            let (from, command) = self
                .received
                .recv_timeout(Duration::from_secs(5))
                .expect("fake mpv received a command");
            if from == connection {
                return command;
            }
        }
    }

    pub(in crate::players::mpv) fn send_event(&self, event: &Value) {
        let mut text = event.to_string();
        text.push('\n');
        let mut events = self.events.lock().expect("event stream");
        let stream = events.as_mut().expect("event connection is open");
        stream.write_all(text.as_bytes()).expect("write event");
        stream.flush().expect("flush event");
        drop(events);
    }

    pub(in crate::players::mpv) fn finish(self, worker: IpcWorker) {
        self.stopping.store(true, Ordering::SeqCst);
        self.events.lock().expect("event stream").take();
        worker.shutdown();
        let _ = super::connect_ipc(&self.path);
        let _ = self.server.join();
        super::cleanup_ipc_path(&self.path);
    }

    pub(in crate::players::mpv) fn stop(self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.events.lock().expect("event stream").take();
        let _ = super::connect_ipc(&self.path);
        let _ = self.server.join();
        super::cleanup_ipc_path(&self.path);
    }
}

fn clone(stream: &Stream) -> Stream {
    stream.try_clone().expect("clone fake mpv connection")
}

#[cfg(unix)]
struct Listener(std::os::unix::net::UnixListener);

#[cfg(unix)]
impl Listener {
    fn bind(path: &str) -> Self {
        let listener = std::os::unix::net::UnixListener::bind(path).expect("bind fake mpv socket");
        listener
            .set_nonblocking(true)
            .expect("nonblocking fake mpv socket");
        Self(listener)
    }

    fn accept(&mut self, stopping: &AtomicBool) -> Option<Stream> {
        while !stopping.load(Ordering::SeqCst) {
            match self.0.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    return Some(stream);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return None,
            }
        }
        None
    }
}

#[cfg(windows)]
struct Listener {
    name: Vec<u16>,
    next: Option<Stream>,
}

#[cfg(windows)]
impl Listener {
    fn bind(path: &str) -> Self {
        let name: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let next = Some(Self::instance(&name));
        Self { name, next }
    }

    fn instance(name: &[u16]) -> Stream {
        use std::os::windows::io::FromRawHandle;

        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
        use windows_sys::Win32::System::Pipes::{
            CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
            PIPE_WAIT,
        };

        // SAFETY: `name` is a NUL-terminated wide string that outlives the
        // call, and null security attributes select the defaults.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                std::ptr::null(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE, "create fake mpv pipe");
        // SAFETY: the handle is a new pipe instance nothing else owns.
        unsafe { Stream::from_raw_handle(handle) }
    }

    fn accept(&mut self, stopping: &AtomicBool) -> Option<Stream> {
        use std::os::windows::io::AsRawHandle;

        use windows_sys::Win32::Foundation::{ERROR_PIPE_CONNECTED, GetLastError};
        use windows_sys::Win32::System::Pipes::ConnectNamedPipe;

        if stopping.load(Ordering::SeqCst) {
            return None;
        }
        let stream = self
            .next
            .take()
            .unwrap_or_else(|| Self::instance(&self.name));
        // SAFETY: `stream` owns a live pipe instance; a null OVERLAPPED
        // makes this a blocking wait for one client.
        let connected = unsafe { ConnectNamedPipe(stream.as_raw_handle(), std::ptr::null_mut()) };
        // SAFETY: reads this thread's last error, set by ConnectNamedPipe.
        let already = connected == 0 && unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        assert!(connected != 0 || already, "accept fake mpv client");
        (!stopping.load(Ordering::SeqCst)).then_some(stream)
    }
}
