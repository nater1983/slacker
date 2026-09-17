//! History: the most recent package changes, grouped by day.

use std::rc::Rc;

use adw::prelude::*;

use super::Page;
use crate::commands;
use crate::ctx::Ctx;
use crate::parse::history::{self, Kind};
use crate::widgets::{self, Tone};

const LAST: usize = 300;

pub fn page(ctx: &Ctx) -> Page {
    let (scroller, body) = widgets::page_body();
    let refresh = widgets::flat_icon_button("view-refresh-symbolic", "Reload");

    let load: Rc<dyn Fn()> = {
        let (ctx, body) = (ctx.clone(), body.clone());
        Rc::new(move || {
            widgets::clear(&body);
            body.append(&widgets::loading("Reading package history\u{2026}"));
            let body2 = body.clone();
            ctx.runner.capture(commands::history_recent(LAST), move |status, text| {
                widgets::clear(&body2);
                let h = history::parse(&text);
                if h.events.is_empty() {
                    let msg = if h.messages.is_empty() {
                        crate::runner::describe(&status, commands::Privilege::User)
                    } else {
                        h.messages.join("\n")
                    };
                    body2.append(&widgets::status_page("document-open-recent-symbolic", "No history", &msg));
                    return;
                }
                let caption = gtk::Label::builder()
                    .label(format!("The last {} changes, newest first. Changes made outside slacker are included.", h.events.len()))
                    .xalign(0.0)
                    .wrap(true)
                    .build();
                caption.add_css_class("dim-label");
                body2.append(&caption);

                let mut day = String::new();
                let mut group: Option<adw::PreferencesGroup> = None;
                for e in &h.events {
                    let d = &e.when[..10];
                    if d != day {
                        if let Some(g) = group.take() {
                            body2.append(&g);
                        }
                        day = d.to_string();
                        group = Some(widgets::group(&day, ""));
                    }
                    let (icon, class, verb) = match e.kind {
                        Kind::Installed => ("list-add-symbolic", Some("success"), "Installed"),
                        Kind::Removed => ("list-remove-symbolic", Some("error"), "Removed"),
                        Kind::Upgraded => ("go-up-symbolic", Some("info"), "Upgraded"),
                        Kind::Reinstalled => ("view-refresh-symbolic", None, "Reinstalled"),
                    };
                    let row = widgets::row(&e.name, &format!("{verb}  {}", e.detail));
                    row.add_prefix(&widgets::tile(icon, class));
                    row.add_suffix(&widgets::pill(&e.source, Tone::Neutral));
                    row.add_suffix(&widgets::dim_label(&e.when[11..]));
                    if let Some(g) = &group {
                        g.add(&row);
                    }
                }
                if let Some(g) = group {
                    body2.append(&g);
                }
                if !h.messages.is_empty() {
                    body2.append(&widgets::raw_text(&h.messages.join("\n")));
                }
            });
        })
    };
    {
        let l = load.clone();
        refresh.connect_clicked(move |_| l());
    }

    Page {
        name: "history",
        title: "History",
        subtitle: "slacker history",
        icon: "document-open-recent-symbolic",
        widget: scroller.upcast(),
        header: Some(refresh.upcast()),
        load: Some(load),
        reload_on_change: true,
    }
}
