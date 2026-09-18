//! Root actions: a confirmation that shows the exact command line, then a
//! transaction dialog with slacker's live output.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::commands::Spec;
use crate::ctx::Ctx;
use crate::output::{append, terminal_view};
use crate::runner::{describe, Status};

/// Shows `dialog` and runs `then` if the user picked `response`.
///
/// When an alert dialog closes while one of its own buttons has keyboard
/// focus, the window keeps pointing at that (now hidden) button and no key
/// reaches anything until the user clicks. So after any response the stale
/// focus is cleared, and `then` runs from idle, once the dialog is gone.
pub fn after_choice(
    dialog: &adw::AlertDialog,
    response: &'static str,
    parent: &impl IsA<gtk::Widget>,
    then: impl FnOnce() + 'static,
) {
    let window = parent.as_ref().root().and_downcast::<gtk::Window>();
    let then = Rc::new(std::cell::RefCell::new(Some(then)));
    dialog.connect_response(None, move |_, r| {
        let run = if r == response { then.borrow_mut().take() } else { None };
        let window = window.clone();
        glib::idle_add_local_once(move || {
            if let Some(w) = &window {
                if GtkWindowExt::focus(w).is_some_and(|f| !f.is_mapped()) {
                    GtkWindowExt::set_focus(w, None::<&gtk::Widget>);
                }
            }
            if let Some(f) = run {
                f();
            }
        });
    });
    dialog.present(Some(parent));
}

/// The first button below `root` whose label is `label`.
fn find_button(root: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    let mut child = root.first_child();
    while let Some(c) = child {
        if let Some(b) = c.downcast_ref::<gtk::Button>() {
            if b.label().as_deref() == Some(label) {
                return Some(b.clone());
            }
        }
        if let Some(found) = find_button(&c, label) {
            return Some(found);
        }
        child = c.next_sibling();
    }
    None
}

pub struct Action {
    pub spec: Spec,
    /// e.g. "Install vim"
    pub title: String,
    /// The confirm button, e.g. "Install"
    pub verb: String,
    pub destructive: bool,
}

/// Whether a finished root action may have changed installed packages:
/// anything except a dismissed or refused authorization, or a failure to
/// start at all.
pub fn may_have_changed(status: &Status) -> bool {
    !matches!(status, Status::Exited(126 | 127) | Status::SpawnFailed(_))
}

pub fn run_as_root(ctx: &Ctx, action: Action, on_finish: impl FnOnce(&Status) + 'static) {
    let command = ctx.runner.argv(&action.spec).join(" ");

    let dialog = adw::AlertDialog::builder()
        .heading(format!("{}?", action.title))
        .body(
            "slacker will run as root and will not ask again before making changes. \
             You may be asked for your password.",
        )
        .close_response("cancel")
        .default_response(if action.destructive { "cancel" } else { "run" })
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("run", &action.verb)]);
    dialog.set_response_appearance(
        "run",
        if action.destructive {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
    );

    let cmd = gtk::Label::builder()
        .label(&command)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .selectable(true)
        .xalign(0.0)
        .build();
    cmd.add_css_class("command-line");
    dialog.set_extra_child(Some(&cmd));

    let ctx2 = ctx.clone();
    after_choice(&dialog, "run", &ctx.window, move || transaction(&ctx2, action, on_finish));
}

fn transaction(ctx: &Ctx, action: Action, on_finish: impl FnOnce(&Status) + 'static) {
    let buffer = gtk::TextBuffer::new(None);

    let spinner = gtk::Spinner::builder().spinning(true).width_request(20).height_request(20).build();
    let state_icon = gtk::Image::builder().pixel_size(20).visible(false).build();
    let state = gtk::Label::builder()
        .label("Waiting for authorization\u{2026}")
        .xalign(0.0)
        .hexpand(true)
        .wrap(true)
        .build();
    state.add_css_class("heading");
    let status_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    status_row.add_css_class("tx-status");
    status_row.append(&spinner);
    status_row.append(&state_icon);
    status_row.append(&state);

    let close = gtk::Button::with_label("Close");
    close.add_css_class("pill");
    close.set_sensitive(false);
    let bottom = gtk::Box::builder().halign(gtk::Align::End).build();
    bottom.add_css_class("tx-bottom");
    bottom.append(&close);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.add_css_class("tx-body");
    body.append(&status_row);
    body.append(&terminal_view(&buffer));

    let header = adw::HeaderBar::builder()
        .show_end_title_buttons(false)
        .show_start_title_buttons(false)
        .build();
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&body));
    view.add_bottom_bar(&bottom);

    let dialog = adw::Dialog::builder()
        .title(glib::markup_escape_text(&action.title))
        .content_width(980)
        .content_height(660)
        .can_close(false)
        .child(&view)
        .build();
    {
        let d = dialog.clone();
        close.connect_clicked(move |_| {
            d.close();
        });
    }
    dialog.present(Some(&ctx.window));

    let started = Rc::new(std::cell::Cell::new(false));
    let privilege = action.spec.privilege;
    let (b, st, s) = (buffer.clone(), state.clone(), started.clone());
    let (ctx2, title) = (ctx.clone(), action.title.clone());
    ctx.runner.run(
        action.spec,
        move |text| {
            if !s.replace(true) {
                st.set_text("Running\u{2026}");
            }
            append(&b, text);
        },
        move |status| {
            spinner.set_visible(false);
            state_icon.set_visible(true);
            let message = describe(&status, privilege);
            let (icon_name, class) = match status.code() {
                Some(0) => ("object-select-symbolic", "success"),
                Some(20 | 50 | 100) => ("dialog-information-symbolic", "accent"),
                Some(126) => ("dialog-information-symbolic", "dim-label"),
                _ => ("dialog-error-symbolic", "error"),
            };
            state_icon.set_icon_name(Some(icon_name));
            state_icon.add_css_class(class);
            state.set_text(&message);
            if buffer.char_count() == 0 {
                append(&buffer, "(no output)");
            }
            dialog.set_can_close(true);
            close.set_sensitive(true);
            close.add_css_class("suggested-action");
            close.grab_focus();
            ctx2.toast(&format!("{title}: {message}"));
            on_finish(&status);
        },
    );
}

/// What a preview run said about the change.
pub enum Verdict {
    /// slacker is ready to write it.
    Ready,
    /// slacker flagged it; applying needs an explicit override (button label).
    Override(&'static str),
    /// Nothing would change.
    Nothing,
    /// slacker refused the change or the output was not understood.
    Failed,
}

pub struct Previewed {
    /// Same command without `--yes`: prints the plan and writes nothing.
    pub preview: Spec,
    pub action: Action,
    pub judge: fn(&str) -> Verdict,
}

/// The preview output without the unanswered prompt and its abort line.
fn preview_text(text: &str) -> String {
    text.lines()
        .filter(|l| !l.contains("[y/N]") && l.trim() != "aborted \u{2014} nothing changed")
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Runs the preview, shows slacker's own description of the change, and
/// applies it only after the user agrees.
pub fn run_previewed(ctx: &Ctx, p: Previewed, on_finish: impl FnOnce(&Status) + 'static) {
    ctx.toast("Asking slacker what would change\u{2026}");
    let ctx2 = ctx.clone();
    ctx.runner.capture(p.preview.clone(), move |status, text| {
        // Present from idle, once the runner has finished with this command
        // and re-enabled its buttons; a dialog presented from inside the
        // completion callback does not get keyboard focus.
        glib::idle_add_local_once(move || show_preview(&ctx2, p, status, text, on_finish));
    });
}

fn show_preview(
    ctx2: &Ctx,
    p: Previewed,
    status: Status,
    text: String,
    on_finish: impl FnOnce(&Status) + 'static,
) {
    {
        let verdict = if status.answered() { (p.judge)(&text) } else { Verdict::Failed };
        let shown = preview_text(&text);
        let command = ctx2.runner.argv(&p.action.spec).join(" ");

        let body = match &verdict {
            Verdict::Ready => "slacker checked this change. Nothing has been written yet.".to_string(),
            Verdict::Override(_) => {
                "slacker thinks this may be a mistake. Nothing has been written yet.".to_string()
            }
            Verdict::Nothing => "There is nothing to change.".to_string(),
            // Authorization or start-up problems are described as such;
            // anything else is slacker refusing, and its message is below.
            Verdict::Failed if matches!(status, Status::Exited(126 | 127) | Status::SpawnFailed(_)) => {
                describe(&status, p.preview.privilege)
            }
            Verdict::Failed => "slacker did not accept this change.".to_string(),
        };
        let dialog = adw::AlertDialog::builder()
            .heading(&p.action.title)
            .body(body)
            .build();

        let extra = gtk::Box::new(gtk::Orientation::Vertical, 8);
        if !shown.is_empty() {
            let l = gtk::Label::builder()
                .label(&shown)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .selectable(true)
                .xalign(0.0)
                .build();
            l.add_css_class("command-line");
            l.set_wrap(false);
            // Selectable with the mouse; never the initial focus, so it does
            // not open fully selected.
            l.set_focusable(false);
            // Long plans scroll instead of squeezing the dialog.
            // The dialog itself cannot be given a width (AdwAlertDialog has no
            // usable content width), so the plan's own box asks for the room:
            // one package per line, scrolling instead of wrapping.
            let scroller = gtk::ScrolledWindow::builder()
                .child(&l)
                .width_request(700)
                .min_content_height(240)
                .max_content_height(420)
                .hscrollbar_policy(gtk::PolicyType::Automatic)
                .vscrollbar_policy(gtk::PolicyType::Automatic)
                .build();
            extra.append(&scroller);
        }
        match verdict {
            Verdict::Ready | Verdict::Override(_) => {
                let c = gtk::Label::builder()
                    .label(&command)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .selectable(true)
                    .xalign(0.0)
                    .build();
                c.add_css_class("command-line");
                c.add_css_class("dim-label");
                c.set_focusable(false);
                extra.append(&c);
                let (label, look) = match verdict {
                    Verdict::Override(l) => (l.to_string(), adw::ResponseAppearance::Destructive),
                    _ => (
                        p.action.verb.clone(),
                        if p.action.destructive {
                            adw::ResponseAppearance::Destructive
                        } else {
                            adw::ResponseAppearance::Suggested
                        },
                    ),
                };
                dialog.add_responses(&[("cancel", "Cancel"), ("run", &label)]);
                dialog.set_response_appearance("run", look);
                dialog.set_close_response("cancel");
                dialog.set_default_response(Some("cancel"));
            }
            Verdict::Nothing | Verdict::Failed => {
                dialog.add_responses(&[("close", "Close")]);
                dialog.set_close_response("close");
                dialog.set_default_response(Some("close"));
            }
        }
        dialog.set_extra_child(Some(&extra));

        let ctx3 = ctx2.clone();
        after_choice(&dialog, "run", &ctx2.window, move || transaction(&ctx3, p.action, on_finish));
        // Put keyboard focus on the default button (Cancel, or Close when
        // there is nothing to apply), so Enter and Escape work at once.
        let default_label = if dialog.has_response("run") { "Cancel" } else { "Close" };
        let d = dialog.clone();
        glib::idle_add_local_once(move || {
            if let Some(b) = find_button(d.upcast_ref(), default_label) {
                b.grab_focus();
            }
        });
    }
}

/// Judges `slacker frozen RULE` run without `--yes`.
pub fn judge_freeze(text: &str) -> Verdict {
    if text.contains("Nothing new to add") {
        Verdict::Nothing
    } else if text.contains("look like a mistake") {
        Verdict::Override("Freeze anyway")
    } else if text.contains("About to add") {
        Verdict::Ready
    } else {
        Verdict::Failed
    }
}

/// Judges `slacker pin REPO:PKG` run without `--yes`.
pub fn judge_pin(text: &str) -> Verdict {
    if text.contains("Already pinned:") {
        Verdict::Nothing
    } else if text.contains("About to pin") {
        if text.contains("warning:") {
            Verdict::Override("Pin anyway")
        } else {
            Verdict::Ready
        }
    } else {
        Verdict::Failed
    }
}

/// Judges `install-new --dry-run` and `upgrade-all --dry-run`.
pub fn judge_plan(text: &str) -> Verdict {
    if text.contains("(dry-run: nothing changed)") {
        Verdict::Ready
    } else if text.contains("No new packages to install")
        || text.contains("Nothing to upgrade")
        || text.contains("Nothing selected")
    {
        Verdict::Nothing
    } else {
        Verdict::Failed
    }
}

/// Judges `slacker pri-repo PRIORITY NAME` run without `--yes`. A taken
/// priority or an unknown repo is an error exit, reported as Failed with
/// slacker's own message.
pub fn judge_priority(text: &str) -> Verdict {
    if text.contains("nothing to change") {
        Verdict::Nothing
    } else if text.contains("About to change") {
        Verdict::Ready
    } else {
        Verdict::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Built from the println! calls in cmd_frozen / cmd_pin, with stdin closed.
    const FREEZE_READY: &str = "About to add 1 blacklist rule(s):\n  1. \"kde/\"  \u{2192}  series 'kde' in all repos\nAdd these to the blacklist? [y/N] aborted \u{2014} nothing changed\n";
    const FREEZE_WARN: &str = "1 rule(s) look like a mistake:\n  \"@alienbobb vlc\"       no active repo 'alienbobb'\n  active repos: slackware, alienbob\ndeclare them anyway? [y/N] aborted \u{2014} nothing changed\n";
    const FREEZE_SAME: &str = "already frozen, skipping: kde/\nNothing new to add \u{2014} every given rule is already frozen.\n";
    const PIN_READY: &str = "About to pin (only source for 'vlc', ignoring priority):\n  @alienbob 100% vlc\nwrite it to the blacklist? [y/N] aborted \u{2014} nothing changed\n";
    const PIN_FROZEN: &str = "warning: 'vlc' is also frozen (blacklisted) \u{2014} the freeze wins, so the pin will have no effect until you `unfrozen` it\nAbout to pin (only source for 'vlc', ignoring priority):\n  @alienbob 100% vlc\nwrite it to the blacklist? [y/N] aborted \u{2014} nothing changed\n";

    #[test]
    fn freeze_verdicts() {
        assert!(matches!(judge_freeze(FREEZE_READY), Verdict::Ready));
        assert!(matches!(judge_freeze(FREEZE_WARN), Verdict::Override(_)));
        assert!(matches!(judge_freeze(FREEZE_SAME), Verdict::Nothing));
        assert!(matches!(judge_freeze("slacker: error: 1 problem(s), nothing changed:"), Verdict::Failed));
    }

    #[test]
    fn pin_verdicts() {
        assert!(matches!(judge_pin(PIN_READY), Verdict::Ready));
        assert!(matches!(judge_pin(PIN_FROZEN), Verdict::Override(_)));
        assert!(matches!(judge_pin("Already pinned: vlc -> alienbob\n"), Verdict::Nothing));
        assert!(matches!(judge_pin("slacker: error: no active repo 'x'"), Verdict::Failed));
    }

    #[test]
    fn plan_verdicts() {
        assert!(matches!(
            judge_plan("Upgrade (2):\n  glibc  2.44-4 \u{2192} 2.44-5\n(dry-run: nothing changed)\n"),
            Verdict::Ready
        ));
        assert!(matches!(judge_plan("No new packages to install.\n"), Verdict::Nothing));
        assert!(matches!(judge_plan("Nothing to upgrade.\n"), Verdict::Nothing));
        assert!(matches!(judge_plan("slacker: error: could not read metadata"), Verdict::Failed));
    }

    #[test]
    fn priority_verdicts() {
        // From cmd_pri_repo's println! calls.
        let ready = "About to change 'alienbob' priority: 60 \u{2192} 61\nWrite it to the repos file? [y/N] aborted \u{2014} nothing changed\n";
        assert!(matches!(judge_priority(ready), Verdict::Ready));
        assert!(matches!(
            judge_priority("'alienbob' is already at priority 60 \u{2014} nothing to change.\n"),
            Verdict::Nothing
        ));
        assert!(matches!(
            judge_priority("slacker: error: priority 79 is already used by repo 'lngn' \u{2014} pick another value"),
            Verdict::Failed
        ));
    }

    #[test]
    fn preview_drops_the_unanswered_prompt() {
        let t = preview_text(FREEZE_READY);
        assert!(!t.contains("[y/N]"));
        assert!(!t.contains("aborted"));
        assert!(t.starts_with("About to add 1 blacklist rule(s):"));
    }
}
