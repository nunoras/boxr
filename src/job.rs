use anyhow::Result;
use std::os::windows::io::AsRawHandle;
use std::process::Child;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

pub struct JobGuard(HANDLE);

impl Drop for JobGuard {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub fn guard(child: &Child) -> Result<JobGuard> {
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(anyhow::Error::from(std::io::Error::last_os_error())
                .context("creating a job object for the harness"));
        }
        let guard = JobGuard(job);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let sized = SetInformationJobObject(
            guard.0,
            JobObjectExtendedLimitInformation,
            &limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if sized == 0 {
            return Err(anyhow::Error::from(std::io::Error::last_os_error())
                .context("setting the job object to kill the harness with boxr"));
        }
        let assigned = AssignProcessToJobObject(guard.0, child.as_raw_handle() as HANDLE);
        if assigned == 0 {
            return Err(anyhow::Error::from(std::io::Error::last_os_error())
                .context("placing the harness in the job object"));
        }
        Ok(guard)
    }
}
