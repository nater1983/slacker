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
use crate::output::{clean, Feed};
pub use crate::output::Chunk;

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

/// How many lines of the shared command log are kept.
const LOG_MAX_LINES: i32 = 4_000;

/// How often a line that is still being redrawn is shown.
const PROGRESS_EVERY: std::time::Duration = std::time::Duration::from_millis(150);

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

type OnOutput = Rc<dyn Fn(&str, Chunk)>;
type OnDone = Box<dyn FnOnce(Status)>;

struct Job {
    spec: Spec,
    on_output: OnOutput,
    on_done: OnDone,
    /// Present when someone will answer the command's questions; without it
    /// stdin is closed at once and slacker takes its own default answers.
    input: Option<Input>,
}

/// The stdin of a running command, for answering the questions slacker asks.
/// The caller creates it; the runner connects it once the command actually
/// starts (it may wait in the queue first) and disconnects it when the
/// command ends.
#[derive(Clone, Default)]
pub struct Input(Rc<RefCell<Option<Live>>>);

struct Live {
    stdin: gio::OutputStream,
    /// The unfinished line: the question slacker is waiting on.
    pending: Rc<RefCell<Vec<u8>>>,
    /// Writes a finished line to the caller and to the log.
    echo: Rc<dyn Fn(&str)>,
}

impl Input {
    pub fn new() -> Self {
        Self::default()
    }

    /// Answers the question slacker is waiting on. As on a terminal, the
    /// answer appears at the end of the question's line, which it finishes.
    pub fn send(&self, answer: &str) -> bool {
        let live = self.0.borrow().as_ref().map(|l| (l.stdin.clone(), l.pending.clone(), l.echo.clone()));
        let Some((stdin, pending, echo)) = live else { return false };
        let question = std::mem::take(&mut *pending.borrow_mut());
        echo(&format!("{}{answer}", clean(&question)));
        stdin
            .write_all(format!("{answer}\n").as_bytes(), gio::Cancellable::NONE)
            .is_ok()
    }

    /// No more answers: slacker reads end of input, and every question it
    /// still asks gets its own default (No, abort, keep).
    pub fn close(&self) {
        if let Some(live) = self.0.borrow_mut().take() {
            let _ = live.stdin.close(gio::Cancellable::NONE);
        }
    }

    pub fn is_open(&self) -> bool {
        self.0.borrow().is_some()
    }
}

#[derive(Clone)]
pub struct Runner(Rc<Inner>);

struct Inner {
    slacker: PathBuf,
    busy: Cell<bool>,
    queue: RefCell<VecDeque<Job>>,
    log: Feed,
    spinner: gtk::Spinner,
    watched: RefCell<Vec<glib::WeakRef<gtk::Widget>>>,
}

impl Runner {
    pub fn new(slacker: PathBuf, spinner: gtk::Spinner) -> Self {
        Self(Rc::new(Inner {
            slacker,
            busy: Cell::new(false),
            queue: RefCell::new(VecDeque::new()),
            log: Feed::capped(&gtk::TextBuffer::new(None), LOG_MAX_LINES),
            spinner,
            watched: RefCell::new(Vec::new()),
        }))
    }

    pub fn slacker(&self) -> &Path {
        &self.0.slacker
    }

    /// Every command and its output, for the Command output dialog.
    pub fn log(&self) -> &gtk::TextBuffer {
        self.0.log.buffer()
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

    /// Queues a command whose questions nobody answers: its stdin is closed
    /// at once, so slacker takes the default for anything it asks.
    pub fn run(
        &self,
        spec: Spec,
        on_output: impl Fn(&str, Chunk) + 'static,
        on_done: impl FnOnce(Status) + 'static,
    ) {
        self.queue(spec, None, Rc::new(on_output), Box::new(on_done));
    }

    /// Queues a command and keeps its stdin open for `input` to answer.
    pub fn run_answering(
        &self,
        spec: Spec,
        input: &Input,
        on_output: impl Fn(&str, Chunk) + 'static,
        on_done: impl FnOnce(Status) + 'static,
    ) {
        self.queue(spec, Some(input.clone()), Rc::new(on_output), Box::new(on_done));
    }

    fn queue(&self, spec: Spec, input: Option<Input>, on_output: OnOutput, on_done: OnDone) {
        self.0.queue.borrow_mut().push_back(Job { spec, on_output, on_done, input });
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
            move |chunk, kind| {
                // A progress line is redrawn in place and never part of the
                // finished output a parser reads.
                if kind == Chunk::Progress {
                    return;
                }
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
        self.log_line(&format!("$ {}", argv.join(" ")));

        let this = self.clone();
        glib::spawn_future_local(async move {
            let status = this.execute(&argv, job.on_output, job.input).await;
            this.log_line(&format!("[{}]\n", describe(&status, job.spec.privilege)));
            (job.on_done)(status);
            this.next();
        });
    }

    async fn execute(&self, argv: &[String], on_output: OnOutput, input: Option<Input>) -> Status {
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

        // The unfinished last line, shared with the timer below and with
        // `Input::send`, which finishes it with the answer.
        let pending: Rc<RefCell<Vec<u8>>> = Rc::default();
        let timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();

        match (&input, proc.stdin_pipe()) {
            (Some(inp), Some(stdin)) => {
                let (this, out) = (self.clone(), on_output.clone());
                *inp.0.borrow_mut() = Some(Live {
                    stdin,
                    pending: pending.clone(),
                    echo: Rc::new(move |line: &str| {
                        out(line, Chunk::Line);
                        this.0.log.push(line, Chunk::Line);
                    }),
                });
            }
            // Nobody will answer: slacker sees end of input at once instead
            // of reading from whatever terminal started the GUI.
            (None, Some(stdin)) => {
                let _ = stdin.close(gio::Cancellable::NONE);
            }
            (_, None) => {}
        }

        if let Some(stdout) = proc.stdout_pipe() {
            loop {
                match stdout.read_bytes_future(READ_CHUNK, glib::Priority::DEFAULT).await {
                    Ok(bytes) if bytes.is_empty() => break,
                    Ok(bytes) => {
                        let complete = {
                            let mut p = pending.borrow_mut();
                            p.extend_from_slice(&bytes);
                            p.iter().rposition(|&b| b == b'\n').map(|pos| {
                                let rest = p.split_off(pos + 1);
                                std::mem::replace(&mut *p, rest)
                            })
                        };
                        if let Some(c) = complete {
                            self.emit(&c, &*on_output, Chunk::Line);
                        }
                        // What is left has no newline: a download counter
                        // being redrawn, or a question slacker is now
                        // waiting on. Either way nothing more may arrive
                        // until it changes or is answered, so it is shown
                        // from a timer rather than on the next read.
                        if !pending.borrow().is_empty() && timer.borrow().is_none() {
                            let (this, out, p, t) =
                                (self.clone(), on_output.clone(), pending.clone(), timer.clone());
                            let id = glib::timeout_add_local_once(PROGRESS_EVERY, move || {
                                // Fired: the source is gone, so only forget it.
                                t.borrow_mut().take();
                                let bytes = p.borrow().clone();
                                if !bytes.is_empty() {
                                    this.emit(&bytes, &*out, Chunk::Progress);
                                }
                            });
                            *timer.borrow_mut() = Some(id);
                        }
                    }
                    Err(e) => {
                        self.log_line(&format!("[reading output failed: {e}]"));
                        break;
                    }
                }
            }
            if let Some(id) = timer.borrow_mut().take() {
                id.remove();
            }
            let rest = std::mem::take(&mut *pending.borrow_mut());
            if !rest.is_empty() {
                // A last line the command never terminated with a newline.
                self.emit(&rest, &*on_output, Chunk::Line);
            }
        }
        // The command is done reading; no answer can reach it any more.
        if let Some(inp) = &input {
            inp.close();
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

    fn emit(&self, raw: &[u8], on_output: &dyn Fn(&str, Chunk), kind: Chunk) {
        let text = clean(raw);
        if kind == Chunk::Progress && text.trim().is_empty() {
            return;
        }
        on_output(&text, kind);
        self.0.log.push(&text, kind);
    }

    fn log_line(&self, text: &str) {
        self.0.log.line(text);
    }
}
