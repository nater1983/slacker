//! slacker-gui: a GTK4/libadwaita front-end for the slacker package manager.
//!
//! The GUI holds no package, resolver, GPG or history logic of its own.
//! Every piece of information comes from running the slacker binary and
//! every change goes through it: read-only commands run as the desktop user,
//! commands that modify the system run as root through pkexec.

mod commands;
mod confirm;
mod ctx;
mod edit;
mod output;
mod pages;
mod parse;
mod resolve;
mod runner;
mod widgets;
mod window;

#[cfg(test)]
mod install_tests;

use adw::prelude::*;
use gtk::{gdk, glib};

const APP_ID: &str = "nl.slackware.forge.rizitis.SlackerGui";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| {
        // Installed as hicolor/*/apps/<APP_ID>; used by X11 window managers.
        gtk::Window::set_default_icon_name(APP_ID);
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::PreferDark);
        load_style();
    });
    app.connect_activate(window::build);
    app.run()
}

fn load_style() {
    let Some(display) = gdk::Display::default() else { return };
    let mut sheets = vec![include_str!("style.css")];
    // Older libadwaita does not understand `:root` variables; only newer
    // releases take the accent from them.
    if (adw::major_version(), adw::minor_version()) >= (1, 6) {
        sheets.push(include_str!("style-vars.css"));
    }
    for css in sheets {
        let provider = gtk::CssProvider::new();
        provider.load_from_data(css);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
