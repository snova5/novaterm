//! Pseudo-terminal + child process, using raw libc calls (bionic on Android,
//! glibc on desktop Linux for tests). No external crates.

use std::ffi::{c_char, c_int, c_ulong, c_void, CString};
use std::io;

const O_RDWR: c_int = 0o2;
const O_NOCTTY: c_int = 0o400;
const O_CLOEXEC: c_int = 0o2000000;
const TCGETS: c_ulong = 0x5401;
const TCSETS: c_ulong = 0x5402;
const TIOCSCTTY: c_ulong = 0x540E;
const TIOCSWINSZ: c_ulong = 0x5414;
const IUTF8: u32 = 0o40000;
const SIG_SETMASK: c_int = 2;
const EINTR: i32 = 4;
pub const SIGHUP: c_int = 1;
pub const SIGKILL: c_int = 9;

#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

extern "C" {
    fn posix_openpt(flags: c_int) -> c_int;
    fn grantpt(fd: c_int) -> c_int;
    fn unlockpt(fd: c_int) -> c_int;
    fn ptsname_r(fd: c_int, buf: *mut c_char, len: usize) -> c_int;
    fn fork() -> c_int;
    fn setsid() -> c_int;
    fn ioctl(fd: c_int, req: c_ulong, ...) -> c_int;
    fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    fn dup2(a: c_int, b: c_int) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn chdir(path: *const c_char) -> c_int;
    fn execve(path: *const c_char, argv: *const *const c_char, envp: *const *const c_char) -> c_int;
    fn _exit(code: c_int) -> !;
    fn waitpid(pid: c_int, status: *mut c_int, opts: c_int) -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, n: usize) -> isize;
    fn write(fd: c_int, buf: *const c_void, n: usize) -> isize;
    fn sigprocmask(how: c_int, set: *const c_void, old: *mut c_void) -> c_int;
}

pub struct Child {
    pub master: c_int,
    pub pid: c_int,
}

fn cstr(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap()
}

/// Spawn `program` with `argv` (argv[0] included) and `env` ("KEY=VALUE")
/// attached to a fresh pseudo-terminal of the given size.
pub fn spawn(program: &str, argv: &[String], env: &[String], cwd: &str, rows: u16, cols: u16) -> io::Result<Child> {
    unsafe {
        let master = posix_openpt(O_RDWR | O_NOCTTY | O_CLOEXEC);
        if master < 0 {
            return Err(io::Error::last_os_error());
        }
        if grantpt(master) != 0 || unlockpt(master) != 0 {
            let e = io::Error::last_os_error();
            close(master);
            return Err(e);
        }
        let mut name = [0 as c_char; 128];
        if ptsname_r(master, name.as_mut_ptr(), name.len()) != 0 {
            let e = io::Error::last_os_error();
            close(master);
            return Err(e);
        }

        // UTF-8 aware line discipline (correct backspace over multibyte chars).
        let mut tio = [0u32; 32];
        if ioctl(master, TCGETS, tio.as_mut_ptr()) == 0 {
            tio[0] |= IUTF8;
            ioctl(master, TCSETS, tio.as_ptr());
        }
        set_size(master, rows, cols);

        // Everything the child needs is allocated *before* fork:
        // after fork only async-signal-safe calls are allowed.
        let prog_c = cstr(program);
        let cwd_c = cstr(cwd);
        let argv_c: Vec<CString> = argv.iter().map(|s| cstr(s)).collect();
        let env_c: Vec<CString> = env.iter().map(|s| cstr(s)).collect();
        let mut argv_p: Vec<*const c_char> = argv_c.iter().map(|s| s.as_ptr()).collect();
        argv_p.push(std::ptr::null());
        let mut env_p: Vec<*const c_char> = env_c.iter().map(|s| s.as_ptr()).collect();
        env_p.push(std::ptr::null());
        let empty_mask = [0u64; 16];

        let pid = fork();
        if pid < 0 {
            let e = io::Error::last_os_error();
            close(master);
            return Err(e);
        }
        if pid == 0 {
            // ── child ──
            // The Android runtime blocks some signals; unblock all for the shell.
            sigprocmask(SIG_SETMASK, empty_mask.as_ptr() as *const c_void, std::ptr::null_mut());
            setsid();
            let slave = open(name.as_ptr(), O_RDWR);
            if slave < 0 {
                _exit(126);
            }
            ioctl(slave, TIOCSCTTY, 0 as c_int);
            dup2(slave, 0);
            dup2(slave, 1);
            dup2(slave, 2);
            for fd in 3..1024 {
                close(fd);
            }
            chdir(cwd_c.as_ptr());
            execve(prog_c.as_ptr(), argv_p.as_ptr(), env_p.as_ptr());
            _exit(127);
        }
        Ok(Child { master, pid })
    }
}

pub fn set_size(fd: c_int, rows: u16, cols: u16) {
    let ws = Winsize { ws_row: rows.max(1), ws_col: cols.max(1), ws_xpixel: 0, ws_ypixel: 0 };
    unsafe {
        ioctl(fd, TIOCSWINSZ, &ws as *const Winsize);
    }
}

/// Blocking read; Ok(0) on EOF/EIO (child gone).
pub fn read_fd(fd: c_int, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        let n = unsafe { read(fd, buf.as_mut_ptr() as *mut c_void, buf.len()) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(EINTR) {
            continue;
        }
        if e.raw_os_error() == Some(5) {
            return Ok(0); // EIO: slave side closed
        }
        return Err(e);
    }
}

pub fn write_all(fd: c_int, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        let n = unsafe { write(fd, data.as_ptr() as *const c_void, data.len()) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(EINTR) {
                continue;
            }
            return Err(e);
        }
        data = &data[n as usize..];
    }
    Ok(())
}

/// Wait for the child; returns a shell-style exit code (128+signal if killed).
pub fn wait_exit(pid: c_int) -> i32 {
    let mut status: c_int = 0;
    loop {
        let r = unsafe { waitpid(pid, &mut status, 0) };
        if r == pid {
            break;
        }
        if r < 0 && io::Error::last_os_error().raw_os_error() == Some(EINTR) {
            continue;
        }
        return -1;
    }
    if status & 0x7F == 0 {
        (status >> 8) & 0xFF
    } else {
        128 + (status & 0x7F)
    }
}

pub fn signal_group(pid: c_int, sig: c_int) {
    unsafe {
        // The child called setsid(), so it leads its own process group.
        if kill(-pid, sig) != 0 {
            kill(pid, sig);
        }
    }
}

pub fn close_fd(fd: c_int) {
    unsafe {
        close(fd);
    }
}
