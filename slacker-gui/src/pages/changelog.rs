//! Changelog: `slacker show-changelog [REPO]`, read-only. slacker always
//! fetches it fresh, so this page runs only when asked.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use super::Page;
use crate::commands::{self, Privilege};
use crate::ctx::Ctx;
use crate::output;
use crate::parse::repos;
use crate::runner::describe;
use crate::widgets;

/// A ChangeLog can be tens of thousands of lines; past this the page stops
/// filling and says so.
const MAX_LINES: usize = 5_000;

pub fn page(ctx: &Ctx) -> Page {
    let (scroller, body) = widgets::page_body();

    let names = gtk::StringList::new(&["official (tracked)"]);
    let repo = gtk::DropDown::builder().model(&names).sensitive(false).build();
    let show = gtk::Button::with_label("Show Changelog");
    show.add_css_class("suggested-action");
    show.add_css_class("pill");
    ctx.runner.watch(&show);
    ctx.runner.watch(&repo);

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let label = gtk::Label::new(Some("Repository"));
    label.add_css_class("dim-label");
    repo.set_hexpand(true);
    controls.append(&label);
    controls.append(&repo);
    controls.append(&show);
    body.append(&controls);

    let hint = gtk::Label::builder()
        .label("The ChangeLog is fetched fresh from the repository each time.")
        .xalign(0.0)
        .wrap(true)
        .build();
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");
    body.append(&hint);

    let area = gtk::Box::new(gtk::Orientation::Vertical, 12);
    area.set_vexpand(true);
    area.append(&widgets::status_page(
        "document-open-recent-symbolic",
        "Repository changelog",
        "Pick a repository and read what it published, newest first.",
    ));
    body.append(&area);

    // The repository names come from slacker, as the user.
    {
        let (repo, names) = (repo.clone(), names.clone());
        ctx.runner.capture(commands::list_repos(), move |_, text| {
            let parsed = repos::parse(&text);
            let mut list = vec!["official (tracked)".to_string()];
            list.extend(parsed.repos.iter().map(|r| r.name.clone()));
            let refs: Vec<&str> = list.iter().map(String::as_str).collect();
            names.splice(0, names.n_items(), &refs);
            repo.set_selected(0);
            repo.set_sensitive(true);
        });
    }

    {
        let (ctx, area, repo) = (ctx.clone(), area.clone(), repo.clone());
        show.connect_clicked(move |_| {
            // Position 0 is slacker's own default: no argument.
            let name = (repo.selected() > 0)
                .then(|| {
                    repo.selected_item()
                        .and_downcast::<gtk::StringObject>()
                        .map(|s| s.string().to_string())
                })
                .flatten();
            load(&ctx, &area, name);
        });
    }

    Page {
        name: "changelog",
        title: "Changelog",
        subtitle: "slacker show-changelog",
        icon: "text-x-generic-symbolic",
        widget: scroller.upcast(),
        header: None,
        load: None,
        reload_on_change: false,
    }
}

fn load(ctx: &Ctx, area: &gtk::Box, repo: Option<String>) {
    widgets::clear(area);
    let shown = repo.clone().unwrap_or_else(|| "the official repository".to_string());
    area.append(&widgets::loading(&format!("Fetching the ChangeLog of {shown}\u{2026}")));

    let buffer = gtk::TextBuffer::new(None);
    let lines = Rc::new(RefCell::new(0usize));
    let started = Rc::new(std::cell::Cell::new(false));
    let (area2, area3) = (area.clone(), area.clone());
    let (b, l, s) = (buffer.clone(), lines.clone(), started.clone());
    ctx.runner.run(
        commands::show_changelog(repo.as_deref()),
        move |text| {
            if !s.replace(true) {
                widgets::clear(&area2);
                area2.append(&output::terminal_view(&b));
            }
            let mut n = l.borrow_mut();
            if *n >= MAX_LINES {
                return;
            }
            let room = MAX_LINES - *n;
            let taken: Vec<&str> = text.lines().take(room).collect();
            *n += taken.len();
            output::append(&b, &taken.join("\n"));
            if *n >= MAX_LINES {
                output::append(
                    &b,
                    &format!("\n[first {MAX_LINES} lines shown; read the rest with `slacker show-changelog` in a terminal]"),
                );
            }
        },
        move |status| {
            if started.get() {
                return;
            }
            widgets::clear(&area3);
            area3.append(&widgets::status_page(
                "dialog-error-symbolic",
                "No changelog",
                &describe(&status, Privilege::User),
            ));
        },
    );
}
