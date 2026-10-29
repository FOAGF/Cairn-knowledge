//! Windows only (M8, D-55): the sidecar workers must end with Cairn, even if Cairn crashes or is
//! killed from the Task Manager. A *Job Object* with `KILL_ON_JOB_CLOSE` does that: every worker is
//! put into one job, and Windows ends all processes of the job when its last handle closes, which
//! happens when Cairn's own process ends, however it ends. (Linux needs nothing: the workers read
//! stdin, and see it close.)
//!
//! The job handle is created on first use and never closed by us; it lives as long as the app.
//! A process can be in a job already (a launcher, a test harness); since Windows 8 jobs nest, so
//! this works there too.

use std::io;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};

/// The job's handle. Only used through Win32 calls, which are thread-safe.
struct Job(HANDLE);

// SAFETY: a job handle is a kernel object reference, usable from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

static JOB: OnceLock<Result<Job, String>> = OnceLock::new();

fn create() -> Result<Job, String> {
    // SAFETY: plain Win32 calls with valid arguments; the handle is closed here on failure.
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(format!("CreateJobObjectW: {}", io::Error::last_os_error()));
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const info).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 {
            let error = io::Error::last_os_error();
            CloseHandle(job);
            return Err(format!("SetInformationJobObject: {error}"));
        }
        Ok(Job(job))
    }
}

/// Puts the process into the app's job. Call it right after spawning.
pub fn assign(process: std::os::windows::io::RawHandle) -> Result<(), String> {
    let job = JOB.get_or_init(create).as_ref().map_err(Clone::clone)?;
    // SAFETY: both handles are valid; the process handle belongs to the caller's `Child`.
    let ok = unsafe { AssignProcessToJobObject(job.0, process as HANDLE) };
    if ok == 0 {
        return Err(format!(
            "AssignProcessToJobObject: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}
