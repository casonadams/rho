//! The site's pages and their addresses. Pages live in the fragment (`/rho/#/docs/ui`), so GitHub
//! Pages serves the app's one page for every address and a reload or a shared link opens the page.

use crate::docs::Doc;

/// The path GitHub Pages serves the site under (`nsh.json` `web.base`).
pub const SITE_BASE: &str = "/rho/";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Home,
    Docs(Doc),
    Config,
    Ecosystem,
    NotFound,
}

impl Route {
    /// The page's address.
    pub fn href(self) -> String {
        self.href_at(None)
    }

    /// The page's address, at the heading `anchor` names.
    pub fn href_at(self, anchor: Option<&str>) -> String {
        let path = match self {
            Route::Home => return SITE_BASE.to_owned(),
            Route::Docs(doc) => format!("docs/{}", doc.slug()),
            Route::Config => "config".to_owned(),
            Route::Ecosystem => "ecosystem".to_owned(),
            Route::NotFound => "404".to_owned(),
        };
        match anchor {
            Some(anchor) => format!("{SITE_BASE}#/{path}/{anchor}"),
            None => format!("{SITE_BASE}#/{path}"),
        }
    }

    /// The page and heading an address names.
    pub fn parse(url: &str) -> (Route, Option<String>) {
        let fragment = url.split_once('#').map_or("", |(_, fragment)| fragment);
        let segments: Vec<&str> = fragment.split('/').filter(|s| !s.is_empty()).collect();
        let at = |anchor: Option<&&str>| anchor.map(|a| (*a).to_owned());
        match segments.as_slice() {
            [] => (Route::Home, None),
            ["docs"] => (Route::Docs(Doc::Overview), None),
            ["docs", slug, rest @ ..] if rest.len() <= 1 => match Doc::from_slug(slug) {
                Some(doc) => (Route::Docs(doc), at(rest.first())),
                None => (Route::NotFound, None),
            },
            ["config"] => (Route::Config, None),
            ["ecosystem"] => (Route::Ecosystem, None),
            _ => (Route::NotFound, None),
        }
    }

    /// Which top-level section of the navigation the page belongs to.
    pub fn section(self) -> Option<NavSection> {
        match self {
            Route::Home => Some(NavSection::Home),
            Route::Docs(_) => Some(NavSection::Docs),
            Route::Config => Some(NavSection::Config),
            Route::Ecosystem => Some(NavSection::Ecosystem),
            Route::NotFound => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavSection {
    Home,
    Docs,
    Config,
    Ecosystem,
}

impl NavSection {
    pub const ALL: [NavSection; 4] = [
        NavSection::Home,
        NavSection::Docs,
        NavSection::Config,
        NavSection::Ecosystem,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NavSection::Home => "Home",
            NavSection::Docs => "Docs",
            NavSection::Config => "Configuration",
            NavSection::Ecosystem => "Ecosystem",
        }
    }

    pub fn route(self) -> Route {
        match self {
            NavSection::Home => Route::Home,
            NavSection::Docs => Route::Docs(Doc::Overview),
            NavSection::Config => Route::Config,
            NavSection::Ecosystem => Route::Ecosystem,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_parse_to_pages_and_headings() {
        let cases = [
            ("https://casonadams.github.io/rho/", Route::Home, None),
            ("https://casonadams.github.io/rho/#", Route::Home, None),
            ("/rho/#/docs", Route::Docs(Doc::Overview), None),
            ("/rho/#/docs/ui/themes", Route::Docs(Doc::Ui), Some("themes")),
            ("/rho/#/docs/mcp-and-hooks/", Route::Docs(Doc::McpAndHooks), None),
            ("/rho/#/docs/nope", Route::NotFound, None),
            ("/rho/#/docs/ui/a/b", Route::NotFound, None),
            ("/rho/#/config", Route::Config, None),
            ("http://127.0.0.1:4173/rho/#/ecosystem", Route::Ecosystem, None),
            ("/rho/#/elsewhere", Route::NotFound, None),
        ];
        for (url, route, anchor) in cases {
            assert_eq!(Route::parse(url), (route, anchor.map(str::to_owned)), "{url}");
        }
    }

    #[test]
    fn every_page_round_trips_through_its_address() {
        let pages = [Route::Home, Route::Config, Route::Ecosystem]
            .into_iter()
            .chain(Doc::ALL.map(Route::Docs));
        for page in pages {
            assert_eq!(Route::parse(&page.href()), (page, None), "{}", page.href());
        }
        let at = Route::Docs(Doc::Configuration).href_at(Some("personas"));
        assert_eq!(
            Route::parse(&at),
            (Route::Docs(Doc::Configuration), Some("personas".to_owned()))
        );
    }

    #[test]
    fn the_base_matches_nsh_json() {
        let manifest = include_str!("../nsh.json");
        assert!(manifest.contains(&format!("\"base\": \"{SITE_BASE}\"")));
    }
}
