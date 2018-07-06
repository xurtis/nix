use std::mem;
use std::os::unix::io::RawFd;
use std::option::Option;
use libc::{self, c_int, c_void};
use {Error, Result};
use errno::Errno;
use ::unistd::Pid;

// For some functions taking with a parameter of type CloneFlags,
// only a subset of these flags have an effect.
libc_bitflags!{
    pub struct CloneFlags: c_int {
        CLONE_VM;
        CLONE_FS;
        CLONE_FILES;
        CLONE_SIGHAND;
        CLONE_PTRACE;
        CLONE_VFORK;
        CLONE_PARENT;
        CLONE_THREAD;
        CLONE_NEWNS;
        CLONE_SYSVSEM;
        CLONE_SETTLS;
        CLONE_PARENT_SETTID;
        CLONE_CHILD_CLEARTID;
        CLONE_DETACHED;
        CLONE_UNTRACED;
        CLONE_CHILD_SETTID;
        CLONE_NEWCGROUP;
        CLONE_NEWUTS;
        CLONE_NEWIPC;
        CLONE_NEWUSER;
        CLONE_NEWPID;
        CLONE_NEWNET;
        CLONE_IO;
    }
}

pub type CloneCb = Box<FnMut() -> isize + Send + 'static>;

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(missing_debug_implementations)]
pub struct CpuSet {
    cpu_set: libc::cpu_set_t,
}

impl CpuSet {
    pub fn new() -> CpuSet {
        CpuSet { cpu_set: unsafe { mem::zeroed() } }
    }

    pub fn is_set(&self, field: usize) -> Result<bool> {
        if field >= 8 * mem::size_of::<libc::cpu_set_t>() {
            Err(Error::Sys(Errno::EINVAL))
        } else {
            Ok(unsafe { libc::CPU_ISSET(field, &self.cpu_set) })
        }
    }

    pub fn set(&mut self, field: usize) -> Result<()> {
        if field >= 8 * mem::size_of::<libc::cpu_set_t>() {
            Err(Error::Sys(Errno::EINVAL))
        } else {
            Ok(unsafe { libc::CPU_SET(field, &mut self.cpu_set) })
        }
    }

    pub fn unset(&mut self, field: usize) -> Result<()> {
        if field >= 8 * mem::size_of::<libc::cpu_set_t>() {
            Err(Error::Sys(Errno::EINVAL))
        } else {
            Ok(unsafe { libc::CPU_CLR(field, &mut self.cpu_set) })
        }
    }
}

pub fn sched_setaffinity(pid: Pid, cpuset: &CpuSet) -> Result<()> {
    let res = unsafe {
        libc::sched_setaffinity(pid.into(),
                                mem::size_of::<CpuSet>() as libc::size_t,
                                &cpuset.cpu_set)
    };

    Errno::result(res).map(drop)
}

pub fn clone(cb: CloneCb,
             stack: Vec<u8>,
             flags: CloneFlags,
             signal: Option<c_int>)
             -> Result<Pid> {
    extern "C" fn callback(data: *mut c_void) -> c_int {
        let mut cb: CloneCb = unsafe { *Box::from_raw(data as *mut CloneCb) };
        cb() as c_int
    }

    let res = unsafe {
        let combined = flags.bits() | signal.unwrap_or(0);
        libc::clone(callback,
                   vec_to_stack(stack) as *mut c_void,
                   combined,
                   Box::into_raw(Box::new(cb)) as *mut c_void)
    };

    Errno::result(res).map(Pid::from_raw)
}

pub fn unshare(flags: CloneFlags) -> Result<()> {
    let res = unsafe { libc::unshare(flags.bits()) };

    Errno::result(res).map(drop)
}

pub fn setns(fd: RawFd, nstype: CloneFlags) -> Result<()> {
    let res = unsafe { libc::setns(fd, nstype.bits()) };

    Errno::result(res).map(drop)
}

/// Turns a vector into a stack pointer, forgetting about the allocation for the stack.
fn vec_to_stack(mut stack: Vec<u8>) -> *mut u8 {
    let stack_len = stack.len();
    let top_ptr: *mut u8 = stack.as_mut_ptr() as *mut u8;
    ::std::mem::forget(stack);
    unsafe {
        let base_ptr = top_ptr.offset(stack_len as isize);
        base_ptr.offset((base_ptr as usize % ::std::mem::size_of::<usize>()) as isize * -1)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use sys::wait::{waitpid, WaitStatus};
    use libc::SIGCHLD;

    fn clone_payload() -> Box<FnMut() -> isize + Send + 'static> {
        let numbers: Vec<i32> = (0..=100).into_iter().collect();
        Box::new(move || {
            assert_eq!(numbers.iter().sum::<i32>(), 5050);
            0
        })
    }

    #[test]
    fn simple_clone() {
        let mut stack = Vec::new();
        stack.resize(4096, 0u8);
        let pid = clone(
            clone_payload(),
            stack,
            CloneFlags::CLONE_VM,
            Some(SIGCHLD),
        ).expect("Executing child");

        let exit_status = waitpid(pid, None).expect("Waiting for child");
        assert_eq!(exit_status, WaitStatus::Exited(pid, 0));
    }
}
