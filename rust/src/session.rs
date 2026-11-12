//! A running shell: PTY child + reader thread feeding the terminal emulator.

use crate::pty;
use crate::term::Terminal;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

struct Status {
    dirty: bool,
    exit: Option<i32>,
}

struct Io {
    master: i32,
    open: bool,
}

pub struct Session {
    pub term: Mutex<Terminal>,
    status: Mutex<Status>,
    cv: Condvar,
    io: Mutex<Io>,
    pid: i32,
}

impl Session {
    pub fn start(program: &str, argv: &[String], env: &[String], cwd: &str, rows: u16, cols: u16) -> std::io::Result<Arc<Session>> {
        let child = pty::spawn(program, argv, env, cwd, rows, cols)?;
        let s = Arc::new(Session {
            term: Mutex::new(Terminal::new(rows as usize, cols as usize)),
            status: Mutex::new(Status { dirty: false, exit: None }),
            cv: Condvar::new(),
            io: Mutex::new(Io { master: child.master, open: true }),
            pid: child.pid,
        });
        let reader = Arc::clone(&s);
        thread::Builder::new()
            .name("novaterm-pty".into())
            .spawn(move || reader.read_loop(child.master))?;
        Ok(s)
    }

    fn read_loop(self: Arc<Self>, fd: i32) {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match pty::read_fd(fd, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let replies = {
                        let mut t = self.term.lock().unwrap();
                        t.feed(&buf[..n]);
                        std::mem::take(&mut t.responses)
                    };
                    if !replies.is_empty() {
                        self.write(&replies);
                    }
                    self.mark_dirty();
                }
            }
        }
        let code = pty::wait_exit(self.pid);
        {
            let mut io = self.io.lock().unwrap();
            if io.open {
                pty::close_fd(io.master);
                io.open = false;
            }
        }
        let mut st = self.status.lock().unwrap();
        st.exit = Some(code);
        st.dirty = true;
        self.cv.notify_all();
    }

    fn mark_dirty(&self) {
        let mut st = self.status.lock().unwrap();
        st.dirty = true;
        self.cv.notify_all();
    }

    pub fn write(&self, data: &[u8]) {
        let io = self.io.lock().unwrap();
        if io.open {
            let _ = pty::write_all(io.master, data);
        }
    }

    pub fn resize(&self, rows: u16, cols: u16) {
        self.term.lock().unwrap().resize(rows as usize, cols as usize);
        let io = self.io.lock().unwrap();
        if io.open {
            pty::set_size(io.master, rows, cols);
        }
        drop(io);
        self.mark_dirty();
    }

    pub fn set_theme(&self, theme: crate::term::Theme) {
        self.term.lock().unwrap().theme = theme;
        self.mark_dirty();
    }

    /// Feed text straight into the emulator (used for local status messages).
    pub fn feed_local(&self, data: &[u8]) {
        self.term.lock().unwrap().feed(data);
        self.mark_dirty();
    }

    /// Block until the screen changed or the process exited.
    /// Returns -1 for "screen changed, still running", otherwise the exit code.
    /// Output bursts are coalesced (~one frame) so the UI isn't flooded.
    pub fn wait_update(&self) -> i32 {
        let mut st = self.status.lock().unwrap();
        while !st.dirty {
            st = self.cv.wait(st).unwrap();
        }
        if st.exit.is_none() {
            drop(st);
            thread::sleep(Duration::from_millis(8));
            st = self.status.lock().unwrap();
        }
        st.dirty = false;
        st.exit.unwrap_or(-1)
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.status.lock().unwrap().exit
    }

    /// SIGHUP the shell's process group; escalate to SIGKILL if it lingers.
    pub fn hangup(self: &Arc<Self>) {
        if self.exit_code().is_some() {
            return;
        }
        pty::signal_group(self.pid, pty::SIGHUP);
        let me = Arc::clone(self);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(1500));
            // Only while not yet reaped, so a recycled pid is never hit.
            let st = me.status.lock().unwrap();
            if st.exit.is_none() {
                pty::signal_group(me.pid, pty::SIGKILL);
            }
        });
    }
}
