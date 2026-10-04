//! Scoped ownership for a spawned process and its descendants.
//!
//! Windows starts suspended, assigns a non-inheritable kill-on-close Job, then
//! resumes the verified owned initial thread. Assignment failures fail closed.
//! Unix starts a new process group before exec. Group ownership requires keeping
//! the leader unreaped until termination; descendants must not leave the group.

use std::io;
use std::process::{Child, ExitStatus};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout, Command};

pub(crate) struct OwnedProcess {
    child: Option<Child>,
    tree: platform::Tree,
}

impl OwnedProcess {
    pub(crate) fn spawn(command: &mut Command) -> io::Result<Self> {
        command.kill_on_drop(true);
        platform::configure(command);
        let tree = platform::Tree::new()?;
        // Own the raw child before converting pipes into Tokio handles. Tokio
        // Command::spawn converts stdio before installing its kill-on-drop
        // guard, so a failed Unix pipe registration can otherwise leak it.
        let child = command.as_std_mut().spawn()?;
        // Establish this guard before any fallible post-spawn work, including
        // assignment and resume. Its Drop handles every error path.
        let mut owned = Self {
            child: Some(child),
            tree,
        };
        owned
            .tree
            .attach(owned.child.as_ref().expect("new owned child"))?;
        Ok(owned)
    }

    pub(crate) async fn shutdown(&mut self) -> io::Result<()> {
        self.terminate_and_wait().await.map(|_| ())
    }

    /// Observe natural exit without releasing Unix PID ownership, then end the
    /// finite command's remaining descendants before collecting its true status.
    pub(crate) async fn wait(&mut self) -> io::Result<ExitStatus> {
        loop {
            if self
                .tree
                .exited(self.child.as_ref().expect("owned child until Drop"))?
            {
                return self.terminate_and_wait().await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    async fn terminate_and_wait(&mut self) -> io::Result<ExitStatus> {
        // Do not reap the Unix leader before signalling its owned group.
        self.tree.terminate()?;
        let child = self.child.as_mut().expect("owned child until Drop");
        self.tree.kill_parent(child);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let status = loop {
            let waited = child.try_wait();
            self.tree.check_wait_error(&waited);
            if let Some(status) = waited? {
                // Disarm in the same poll that reaps the leader. No await may
                // separate these operations because its PID can be reused.
                self.tree.disarm();
                break status;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "owned process termination did not finish",
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };
        self.tree.wait_terminated().await?;
        Ok(status)
    }

    pub(crate) fn take_stdin(&mut self) -> io::Result<Option<ChildStdin>> {
        self.child
            .as_mut()
            .and_then(|child| child.stdin.take())
            .map(ChildStdin::from_std)
            .transpose()
    }

    pub(crate) fn take_stdout(&mut self) -> io::Result<Option<ChildStdout>> {
        self.child
            .as_mut()
            .and_then(|child| child.stdout.take())
            .map(ChildStdout::from_std)
            .transpose()
    }

    pub(crate) fn take_stderr(&mut self) -> io::Result<Option<ChildStderr>> {
        self.child
            .as_mut()
            .and_then(|child| child.stderr.take())
            .map(ChildStderr::from_std)
            .transpose()
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        // Synchronous RAII cleanup also works when the async owner is aborted
        // or the Tokio runtime is shutting down. Never enumerate PIDs to kill.
        if let Err(error) = self.tree.terminate() {
            tracing::warn!(%error, "could not terminate owned process tree");
        }
        if let Some(mut child) = self.child.take() {
            self.tree.kill_parent(&mut child);
            self.tree.reap(child);
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessId, GetProcessIdOfThread, OpenThread, ResumeThread, WaitForSingleObject,
        CREATE_NO_WINDOW, CREATE_SUSPENDED, THREAD_QUERY_LIMITED_INFORMATION,
        THREAD_SUSPEND_RESUME,
    };

    pub(super) fn configure(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }

    pub(super) struct Tree {
        job: OwnedHandle,
    }

    impl Tree {
        pub(super) fn new() -> io::Result<Self> {
            // Null security attributes produce a non-inheritable unnamed Job.
            let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: CreateJobObjectW returned a unique owned kernel handle.
            let job = unsafe { OwnedHandle::from_raw_handle(raw) };
            // SAFETY: This C structure permits all-zero initialization.
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // Breakaway flags remain disabled. Descendants cannot request
            // escape from this Job via CREATE_BREAKAWAY_FROM_JOB.
            let set = unsafe {
                SetInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if set == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { job })
        }

        pub(super) fn attach(&mut self, child: &Child) -> io::Result<()> {
            let process = child.as_raw_handle();
            // SAFETY: Both handles remain owned and open for these operations.
            if unsafe { AssignProcessToJobObject(self.job.as_raw_handle(), process) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let pid = unsafe { GetProcessId(process) };
            if pid == 0 {
                return Err(io::Error::last_os_error());
            }
            resume_owned_thread(pid)
        }

        pub(super) fn terminate(&self) -> io::Result<()> {
            // TerminateJobObject acts on our Job handle, never a guessed PID.
            if unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub(super) fn disarm(&mut self) {
            // Keep kill-on-close armed: descendants can outlive their parent,
            // and termination may still be in progress when the parent exits.
        }

        pub(super) fn exited(&mut self, child: &Child) -> io::Result<bool> {
            // Observe the exact owned process object. Never reap by PID here.
            match unsafe { WaitForSingleObject(child.as_raw_handle(), 0) } {
                WAIT_OBJECT_0 => Ok(true),
                WAIT_TIMEOUT => Ok(false),
                _ => Err(io::Error::last_os_error()),
            }
        }

        pub(super) fn check_wait_error(&mut self, _: &io::Result<Option<ExitStatus>>) {}

        pub(super) fn kill_parent(&self, child: &mut Child) {
            let _ = child.kill();
        }

        pub(super) fn reap(&self, child: Child) {
            // Windows needs no userspace reaper. Closing a terminated process
            // handle frees its OS object once all observation handles close.
            drop(child);
        }

        pub(super) async fn wait_terminated(&self) -> io::Result<()> {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                // SAFETY: The Job stays owned while the properly-sized output
                // structure is borrowed by this synchronous query.
                let queried = unsafe {
                    QueryInformationJobObject(
                        self.job.as_raw_handle(),
                        JobObjectBasicAccountingInformation,
                        (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                        size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                        std::ptr::null_mut(),
                    )
                };
                if queried == 0 {
                    return Err(io::Error::last_os_error());
                }
                if accounting.ActiveProcesses == 0 {
                    return Ok(());
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "owned Job termination did not finish",
                    ));
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
    }

    fn resume_owned_thread(pid: u32) -> io::Result<()> {
        // Stable Rust discards CreateProcess's initial thread handle. A newly
        // CREATE_SUSPENDED process has not run user code. Locate its thread,
        // open it, and verify ownership again before resuming it. The process
        // handle held by Child prevents PID reuse during this operation.
        let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if raw == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: Snapshot handle is valid and uniquely owned.
        let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut found = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
        while found != 0 {
            if entry.th32OwnerProcessID == pid {
                let raw = unsafe {
                    OpenThread(
                        THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
                        0,
                        entry.th32ThreadID,
                    )
                };
                if raw.is_null() {
                    return Err(io::Error::last_os_error());
                }
                // SAFETY: OpenThread returned a unique owned handle.
                let thread = unsafe { OwnedHandle::from_raw_handle(raw) };
                if unsafe { GetProcessIdOfThread(thread.as_raw_handle()) } != pid {
                    return Err(io::Error::other("owned initial thread ownership changed"));
                }
                let previous = unsafe { ResumeThread(thread.as_raw_handle()) };
                if previous != 1 {
                    return Err(if previous == u32::MAX {
                        io::Error::last_os_error()
                    } else {
                        io::Error::other("owned initial thread was not suspended exactly once")
                    });
                }
                return Ok(());
            }
            found = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
        }
        Err(io::Error::other("owned suspended initial thread not found"))
    }
}

#[cfg(unix)]
mod platform {
    use super::*;

    pub(super) fn configure(command: &mut Command) {
        command.process_group(0);
    }

    pub(super) struct Tree {
        group: Option<libc::pid_t>,
        parent_owned: bool,
        reaper: std::sync::mpsc::Sender<Child>,
    }

    impl Tree {
        pub(super) fn new() -> io::Result<Self> {
            // Establish a runtime-independent reaper before spawning a child.
            // It polls owned children so one slow exit cannot block others.
            static REAPER: std::sync::OnceLock<io::Result<std::sync::mpsc::Sender<Child>>> =
                std::sync::OnceLock::new();
            let reaper = REAPER.get_or_init(|| {
                let (sender, receiver) = std::sync::mpsc::channel::<Child>();
                std::thread::Builder::new()
                    .name("mcp-process-reaper".into())
                    .spawn(move || {
                        let mut pending = Vec::<Child>::new();
                        loop {
                            // An idle app must not wake forty times a second
                            // after its last owned process has been reaped.
                            if pending.is_empty() {
                                match receiver.recv() {
                                    Ok(child) => pending.push(child),
                                    Err(_) => break,
                                }
                            }
                            match receiver.recv_timeout(std::time::Duration::from_millis(25)) {
                                Ok(child) => pending.push(child),
                                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                                Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
                                    if pending.is_empty() =>
                                {
                                    break
                                }
                                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
                            }
                            while let Ok(child) = receiver.try_recv() {
                                pending.push(child);
                            }
                            pending.retain_mut(|child| match child.try_wait() {
                                Ok(Some(_)) => false,
                                Err(error) if error.raw_os_error() == Some(libc::ECHILD) => false,
                                _ => true,
                            });
                        }
                    })?;
                Ok(sender)
            });
            match reaper {
                Ok(reaper) => Ok(Self {
                    group: None,
                    parent_owned: true,
                    reaper: reaper.clone(),
                }),
                Err(error) => Err(io::Error::new(error.kind(), error.to_string())),
            }
        }

        pub(super) fn attach(&mut self, child: &Child) -> io::Result<()> {
            let pid = child.id();
            self.group = Some(libc::pid_t::try_from(pid).map_err(io::Error::other)?);
            Ok(())
        }

        pub(super) fn terminate(&self) -> io::Result<()> {
            if let Some(group) = self.group {
                // The unreaped owned group leader reserves this ID. Never
                // signal zero (our group) or -1 (all permitted processes).
                if group <= 1 {
                    return Err(io::Error::other("invalid owned process group"));
                }
                if unsafe { libc::kill(-group, libc::SIGKILL) } == -1 {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(error);
                    }
                }
            }
            Ok(())
        }

        pub(super) fn disarm(&mut self) {
            self.group = None;
            self.parent_owned = false;
        }

        pub(super) fn exited(&mut self, child: &Child) -> io::Result<bool> {
            // POSIX waitid + WNOWAIT observes exit but retains the zombie and
            // its PID until we signal the owned group. std Child::try_wait
            // alone would reap first and permit group ID reuse.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id() as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == -1 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EINTR) {
                    return Ok(false);
                }
                if error.raw_os_error() == Some(libc::ECHILD) {
                    // An unexpected external reaper invalidates numerical
                    // ownership. Fail instead of signalling a possibly reused ID.
                    self.disarm();
                }
                return Err(error);
            }
            // Initialized zero first for OS versions that leave siginfo
            // unchanged when no exit is ready. libc exposes this on Linux/macOS.
            Ok(unsafe { info.si_pid() } == child.id() as libc::pid_t)
        }

        pub(super) fn check_wait_error(&mut self, waited: &io::Result<Option<ExitStatus>>) {
            if waited.as_ref().err().and_then(io::Error::raw_os_error) == Some(libc::ECHILD) {
                self.disarm();
            }
        }

        pub(super) fn kill_parent(&self, child: &mut Child) {
            if self.parent_owned {
                let _ = child.kill();
            }
        }

        pub(super) fn reap(&self, mut child: Child) {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            if let Err(error) = self.reaper.send(child) {
                // The prestarted reaper cannot normally disappear. Retain
                // exact ownership and reap synchronously if it ever fails.
                let mut child = error.0;
                let _ = child.wait();
            }
        }

        pub(super) async fn wait_terminated(&self) -> io::Result<()> {
            // SIGKILL was sent to the group before its leader was reaped.
            // Grandchildren belong to another reaper; process groups have no
            // handle-based completion API and escaped groups are not owned.
            Ok(())
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::io::{AsRawHandle, BorrowedHandle};
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    #[tokio::test]
    async fn preassignment_error_guard_terminates_suspended_child() {
        let exe = crate::exec::resolve_program("node")
            .expect("node required for process ownership regression");
        let mut command = Command::new(exe);
        command.args(["-e", "setInterval(() => {}, 1000)"]);
        command.kill_on_drop(true);
        platform::configure(&mut command);
        let tree = platform::Tree::new().unwrap();
        let child = command.as_std_mut().spawn().unwrap();
        // Observation only; clone the exact owned process object before guard
        // cleanup. This models a failure between spawn and Job assignment.
        let witness = unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle()) }
            .try_clone_to_owned()
            .unwrap();
        let owned = OwnedProcess {
            child: Some(child),
            tree,
        };
        drop(owned);
        assert_eq!(
            unsafe { WaitForSingleObject(witness.as_raw_handle(), 2000) },
            WAIT_OBJECT_0
        );
    }
}
