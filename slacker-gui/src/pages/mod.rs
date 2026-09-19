//! Sidebar pages. Each page loads lazily the first time it is shown and
//! reloads after anything changes the installed system.

use std::rc::Rc;

pub mod changelog;
pub mod history;
pub mod mirrors;
pub mod overview;
pub mod packages;
pub mod repositories;
pub mod rules;
pub mod search;
pub mod updates;

pub struct Page {
    pub name: &'static str,
    pub title: &'static str,
    /// Shown under the title in the header: the slacker command behind the page.
    pub subtitle: &'static str,
    pub icon: &'static str,
    pub widget: gtk::Widget,
    /// Extra header-bar widgets shown while this page is visible.
    pub header: Option<gtk::Widget>,
    /// (Re)loads the page's data from slacker.
    pub load: Option<Rc<dyn Fn()>>,
    /// Whether the page must reload after an install, removal or upgrade.
    pub reload_on_change: bool,
}
