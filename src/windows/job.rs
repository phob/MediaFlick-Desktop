#[cfg(target_os = "windows")]
fn kill_on_close_job() -> isize {
    use std::ffi::c_void;
    use std::sync::OnceLock;

    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    static JOB: OnceLock<isize> = OnceLock::new();
    *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return 0;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        // Intentionally keep the handle open for the process lifetime: when this
        // process exits, the OS closes it, the job closes, and every assigned
        // child is terminated.
        job as isize
    })
}

/// Bind an external player child to this process's lifetime, so it is killed if
/// MediaFlick exits without (or fails) a graceful shutdown. No-op off Windows.
#[cfg(target_os = "windows")]
pub fn confine_to_app_lifetime(child: &std::process::Child) {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

    let job = kill_on_close_job();
    if job == 0 {
        return;
    }
    let assigned =
        unsafe { AssignProcessToJobObject(job as HANDLE, child.as_raw_handle() as HANDLE) };
    if assigned == 0 {
        tracing::warn!(target: "playback", "failed to confine player process to the app job object");
    }
}

#[cfg(not(target_os = "windows"))]
pub fn confine_to_app_lifetime(_child: &std::process::Child) {}
