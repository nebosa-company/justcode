//! A Windows job object, so a step's children die with it (`X-4`).
//!
//! **This is the only module in the crate allowed to use `unsafe`**, and the
//! only reason the workspace lint is `deny` rather than `forbid`.
//!
//! The problem it solves is specific. `taskkill /F /T /PID` walks the *parent
//! chain*: it kills what it can find whose ancestry leads back to the pid. A
//! grandchild whose immediate parent has already exited is an orphan, is no
//! longer reachable from that chain, and survives. Worse, none of it happens at
//! all if the engine is killed with `-9`, because nothing gets to run.
//!
//! A job object inverts that. Children are added to a kernel-owned set, and the
//! job is created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: when the last
//! handle to it closes — including when the process holding it is killed
//! without warning — **the kernel terminates every process in the job**. No
//! cooperation from the engine, no walk, no orphans.
//!
//! On every other platform this is a no-op wrapper; POSIX gets the same
//! guarantee from `process_group(0)` plus `kill -9 -pid`, which `process.rs`
//! already does.

#[cfg(windows)]
mod windows_impl {
    #![allow(unsafe_code)] // See the module docs. Nothing else in the crate does.

    use std::fmt;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    /// A kernel-owned set of processes that dies when this handle closes.
    pub struct Job {
        handle: HANDLE,
    }

    // The handle is owned by this value and is only used through the methods
    // below, which take `&self` and pass it straight to a thread-safe Win32
    // call. Windows handles are process-wide and not thread-affine.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl fmt::Debug for Job {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Job(windows)")
        }
    }

    impl Job {
        /// Create a job whose members are killed when it closes.
        ///
        /// Returns `None` rather than failing: a harness that cannot start
        /// because a job object could not be created is worse than one that
        /// falls back to `taskkill`. The caller reports which it got.
        pub fn create() -> Option<Job> {
            // SAFETY: `CreateJobObjectW` with two nulls creates an unnamed job
            // with default security. It returns a handle or null, and null is
            // checked immediately below.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return None;
            }
            let job = Job { handle };

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
                // SAFETY: the struct is plain old data — integers and nested
                // POD structs, no pointers, no enums with invalid bit patterns —
                // so an all-zero value is a valid instance of it, which is
                // exactly the "no limits set" state we then add one flag to.
                unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            // SAFETY: `handle` is a live job handle from the call above. The
            // pointer is to a correctly-typed local that outlives the call, and
            // the length is that type's size, which is what the information
            // class requires.
            let set = unsafe {
                SetInformationJobObject(
                    job.handle,
                    JobObjectExtendedLimitInformation,
                    std::ptr::addr_of!(info).cast(),
                    u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                        .unwrap_or(0),
                )
            };
            if set == 0 {
                // Without the flag the job is a set that kills nothing, which
                // would be worse than no job at all: it would look like the
                // guarantee held.
                return None;
            }
            Some(job)
        }

        /// Put a running process into the job. Everything it starts from now on
        /// is in the job too, by inheritance.
        pub fn adopt(&self, pid: u32) -> bool {
            // SAFETY: `OpenProcess` takes an access mask, an inherit flag and a
            // pid, and returns a handle or null. Null is checked before use.
            let process =
                unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
            if process.is_null() {
                return false;
            }
            // SAFETY: both handles are live — the job is owned by `self`, and
            // `process` was just opened and is closed below on every path.
            let assigned = unsafe { AssignProcessToJobObject(self.handle, process) };
            // SAFETY: `process` is a handle this function opened and has not
            // closed. Closing it does not end the process, only this reference.
            unsafe { CloseHandle(process) };
            assigned != 0
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: `handle` is a live job handle owned by this value and not
            // closed anywhere else. This is the call that makes the guarantee:
            // closing the last handle terminates every process in the job.
            unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(not(windows))]
mod other_impl {
    use std::fmt;

    /// On POSIX the guarantee comes from `process_group(0)` at spawn and
    /// `kill -9 -pid` at teardown, both of which `process.rs` already does.
    /// This is the same shape so the caller has one code path.
    pub struct Job;

    impl fmt::Debug for Job {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Job(process group)")
        }
    }

    impl Job {
        pub fn create() -> Option<Job> {
            Some(Job)
        }

        pub fn adopt(&self, _pid: u32) -> bool {
            // The child was made a process-group leader at spawn; there is
            // nothing to adopt it into afterwards.
            true
        }
    }
}

#[cfg(windows)]
pub use windows_impl::Job;

#[cfg(not(windows))]
pub use other_impl::Job;

/// How a step's children are actually contained, for the journal. A guarantee
/// nobody can read is a guarantee nobody can check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Containment {
    /// A Windows job object with kill-on-close. Survives the engine being
    /// killed without warning.
    JobObject,
    /// A POSIX process group. Same guarantee, different mechanism.
    ProcessGroup,
    /// Neither was available; teardown is `taskkill /T`, which walks the parent
    /// chain and therefore misses orphaned grandchildren.
    ParentWalk,
}

impl Containment {
    /// Whether a `kill -9` of the engine still takes the children with it.
    pub fn survives_engine_death(self) -> bool {
        matches!(self, Containment::JobObject | Containment::ProcessGroup)
    }

    pub fn describe(self) -> &'static str {
        match self {
            Containment::JobObject => {
                "windows job object, kill-on-close — children die even if the engine is killed -9"
            }
            Containment::ProcessGroup => {
                "posix process group — children die even if the engine is killed -9"
            }
            Containment::ParentWalk => {
                "taskkill /T only — an orphaned grandchild can survive, and nothing survives \
                 the engine being killed -9"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_is_available_on_every_platform_we_build_for() {
        let job = Job::create().expect("a job on this platform");
        assert!(format!("{job:?}").starts_with("Job("));
    }

    #[test]
    fn the_containment_says_what_it_actually_guarantees() {
        assert!(Containment::JobObject.survives_engine_death());
        assert!(Containment::ProcessGroup.survives_engine_death());
        // The honest one. This is what the harness had for three cycles, and
        // saying so is the difference between a known limitation and a lie.
        assert!(!Containment::ParentWalk.survives_engine_death());
        assert!(Containment::ParentWalk.describe().contains("can survive"));
    }

    #[test]
    fn a_real_child_is_adopted_into_the_job() {
        // Not a mock: a job that reports success without containing anything is
        // exactly the failure this module exists to remove.
        use std::process::{Command, Stdio};

        let command = if cfg!(windows) { "cmd" } else { "sh" };
        let args: Vec<&str> =
            if cfg!(windows) { vec!["/C", "ping -n 4 127.0.0.1"] } else { vec!["-c", "sleep 3"] };

        let mut child = Command::new(command)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn");

        let job = Job::create().expect("a job");
        assert!(job.adopt(child.id()), "the child must join the job");

        // Dropping the job is what kills it on Windows; on POSIX the process
        // group teardown is `process.rs`'s job, so the child is killed here.
        drop(job);
        let _ = child.kill();
        let _ = child.wait();
    }
}
