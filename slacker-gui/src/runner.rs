//! Runs slacker without blocking the GTK main loop and streams its output.
//!
//! Commands run one at a time, in the order they were asked for: slacker
//! takes its own lock for changes, and queueing here means a page refresh
//! never collides with an install.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};

use crate::commands::{Privilege, Spec};
use crate::output::{append, clean};

pub const PKEXEC: &str = "/usr/bin/pkexec";

/// A fixed environment so the output always has the same form. slacker
/// prints no colour and no banner when stdout is not a terminal; NO_COLOR is
/// set as well, although pkexec does not pass it on.
const ENV: [(&str, &str); 4] = [
    ("LC_ALL", "C"),
    ("LANG", "C"),
    ("TERM", "dumb"),
    ("NO_COLOR", "1"),
];

const READ_CHUNK: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub enum Status {
    Exited(i32),
    Signaled,
    SpawnFailed(String),
}

impl Status {
    /// slacker ran and answered: 0 ok, 20 nothing found, 50 upgraded
    /// itself, 100 updates pending (slackpkg-compatible codes).
    pub fn answered(&self) -> bool {
        matches!(self, Status::Exited(0 | 20 | 50 | 100))
    }

    pub fn code(&self) -> Option<i32> {
        match self {
            Status::Exited(c) => Some(*c),
            _ => None,
        }
    }
}

pub fn describe(status: &Status, privilege: Privilege) -> String {
    match (status, privilege) {
        (Status::Exited(0), _) => "Finished".to_string(),
        (Status::Exited(20), _) => "Nothing found".to_string(),
        (Status::Exited(50), _) => "slacker upgraded itself; run the same action again".to_string(),
        (Status::Exited(100), _) => "Updates are pending".to_string(),
        (Status::Exited(126), Privilege::Root) => {
            "Authentication was dismissed; nothing was run".to_string()
        }
        (Status::Exited(127), Privilege::Root) => {
            "Not authorized, or pkexec could not start slacker".to_string()
        }
        (Status::Exited(n), _) => format!("slacker exited with status {n}"),
        (Status::Signaled, _) => "Stopped by a signal".to_string(),
        (Status::SpawnFailed(e), _) => format!("Could not start: {e}"),
    }
}

type OnOutput = Box<dyn Fn(&str)>;
type OnDone = Box<dyn FnOnce(Status)>;

struct Job {
    spec: Spec,
    on_output: OnOutput,
    on_done: OnDone,
}

#[derive(Clone)]
pub struct Runner(Rc<Inner>);

struct Inner {
    slacker: PathBuf,
    busy: Cell<bool>,
    queue: RefCell<VecDeque<Job>>,
    log: gtk::TextBuffer,
    spinner: gtk::Spinner,
    watched: RefCell<Vec<glib::WeakRef<gtk::Widget>>>,
}

impl Runner {
    pub fn new(slacker: PathBuf, spinner: gtk::Spinner) -> Self {
        Self(Rc::new(Inner {
            slacker,
            busy: Cell::new(false),
            queue: RefCell::new(VecDeque::new()),
            log: gtk::TextBuffer::new(None),
            spinner,
            watched: RefCell::new(Vec::new()),
        }))
    }

    pub fn slacker(&self) -> &Path {
        &self.0.slacker
    }

    /// Every command and its output, for the Command output dialog.
    pub fn log(&self) -> &gtk::TextBuffer {
        &self.0.log
    }

    pub fn argv(&self, spec: &Spec) -> Vec<String> {
        let mut argv = Vec::with_capacity(spec.args.len() + 2);
        if spec.privilege == Privilege::Root {
            argv.push(PKEXEC.to_string());
        }
        argv.push(self.0.slacker.to_string_lossy().into_owned());
        argv.extend(spec.args.iter().cloned());
        argv
    }

    /// The widget is insensitive while any command runs. Used for buttons
    /// that start system changes.
    pub fn watch(&self, widget: &impl IsA<gtk::Widget>) {
        widget.set_sensitive(!self.0.busy.get());
        let mut w = self.0.watched.borrow_mut();
        w.retain(|r| r.upgrade().is_some());
        w.push(widget.upcast_ref::<gtk::Widget>().downgrade());
    }

    fn set_busy(&self, busy: bool) {
        self.0.busy.set(busy);
        self.0.spinner.set_visible(busy);
        self.0.spinner.set_spinning(busy);
        for w in self.0.watched.borrow().iter().filter_map(|r| r.upgrade()) {
            w.set_sensitive(!busy);
        }
    }

    /// Queues a command; output arrives in chunks of whole lines.
    pub fn run(
        &self,
        spec: Spec,
        on_output: impl Fn(&str) + 'static,
        on_done: impl FnOnce(Status) + 'static,
    ) {
        self.0.queue.borrow_mut().push_back(Job {
            spec,
            on_output: Box::new(on_output),
            on_done: Box::new(on_done),
        });
        if !self.0.busy.get() {
            self.next();
        }
    }

    /// Queues a command and hands over its whole output at the end.
    pub fn capture(&self, spec: Spec, on_done: impl FnOnce(Status, String) + 'static) {
        let text = Rc::new(RefCell::new(String::new()));
        let t = text.clone();
        self.run(
            spec,
            move |chunk| {
                let mut s = t.borrow_mut();
                s.push_str(chunk);
                s.push('\n');
            },
            move |status| {
                let out = text.take();
                on_done(status, out);
            },
        );
    }

    fn next(&self) {
        let Some(job) = self.0.queue.borrow_mut().pop_front() else {
            self.set_busy(false);
            return;
        };
        self.set_busy(true);
        let argv = self.argv(&job.spec);
        append(&self.0.log, &format!("$ {}", argv.join(" ")));

        let this = self.clone();
        glib::spawn_future_local(async move {
            let status = this.execute(&argv, &*job.on_output).await;
            append(&this.0.log, &format!("[{}]\n", describe(&status, job.spec.privilege)));
            (job.on_done)(status);
            this.next();
        });
    }

    async fn execute(&self, argv: &[String], on_output: &dyn Fn(&str)) -> Status {
        let launcher = gio::SubprocessLauncher::new(
            gio::SubprocessFlags::STDIN_PIPE
                | gio::SubprocessFlags::STDOUT_PIPE
                | gio::SubprocessFlags::STDERR_MERGE,
        );
        for (k, v) in ENV {
            launcher.setenv(k, v, true);
        }
        let args: Vec<&OsStr> = argv.iter().map(OsStr::new).collect();
        let proc = match launcher.spawn(&args) {
            Ok(p) => p,
            Err(e) => return Status::SpawnFailed(e.to_string()),
        };

        // Nobody answers prompts: slacker sees end-of-file on stdin at once
        // instead of reading from whatever terminal started the GUI.
        if let Some(stdin) = proc.stdin_pipe() {
            let _ = stdin.close(gio::Cancellable::NONE);
        }

        if let Some(stdout) = proc.stdout_pipe() {
            let mut pending: Vec<u8> = Vec::new();
            loop {
                match stdout.read_bytes_future(READ_CHUNK, glib::Priority::DEFAULT).await {
                    Ok(bytes) if bytes.is_empty() => break,
                    Ok(bytes) => {
                        pending.extend_from_slice(&bytes);
                        if let Some(pos) = pending.iter().rposition(|&b| b == b'\n') {
                            let rest = pending.split_off(pos + 1);
                            let complete = std::mem::replace(&mut pending, rest);
                            self.emit(&complete, on_output);
                        }
                    }
                    Err(e) => {
                        append(&self.0.log, &format!("[reading output failed: {e}]"));
                        break;
                    }
                }
            }
            if !pending.is_empty() {
                self.emit(&pending, on_output);
            }
        }

        if let Err(e) = proc.wait_future().await {
            return Status::SpawnFailed(e.to_string());
        }
        if proc.has_exited() {
            Status::Exited(proc.exit_status())
        } else {
            Status::Signaled
        }
    }

    fn emit(&self, raw: &[u8], on_output: &dyn Fn(&str)) {
        let text = clean(raw);
        on_output(&text);
        append(&self.0.log, &text);
    }
}
