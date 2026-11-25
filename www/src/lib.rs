//! The rho website: one nsh app for every page, published to GitHub Pages.

mod config_ref;
mod docs;
mod ecosystem;
mod route;
mod ui;

use nsh_core::prelude::*;

#[native_module("rho-www/app")]
fn native(registry: &mut Registry) {
    registry.register::<App>().assets(assets!());
}

/// The whole site: header, the page the address names, footer.
#[component]
pub fn App() -> impl IntoElement {
    ui::root()
}
