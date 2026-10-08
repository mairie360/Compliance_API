//! Bounded pagination shared by every list endpoint (MAIR-425).
//!
//! A list route never returns a whole table: it extracts [`PageParams`] from the query string
//! (`#[utoipa::path(params(PageParams))]`), runs its SQL with `LIMIT $n OFFSET $m` from
//! [`PageParams::page`], and answers the page with the total count. The page size is clamped to
//! [`MAX_PAGE_SIZE`] whatever the client asks.

use serde::Deserialize;
use utoipa::IntoParams;

/// Page size used when `limit` is absent.
pub const DEFAULT_PAGE_SIZE: u32 = 100;
/// Largest page a client can ask for; a larger `limit` is lowered to it.
pub const MAX_PAGE_SIZE: u32 = 500;

/// Query parameters of a paginated list. Both are optional; the response carries `total` so the
/// client knows how many pages there are.
#[derive(Debug, Default, Clone, Copy, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageParams {
    /// Maximum number of items returned, from 1 to 500. Defaults to 100. `0` is raised to 1 and a
    /// value above 500 is lowered to 500.
    #[param(minimum = 1, maximum = 500, default = 100, example = 50)]
    limit: Option<u32>,
    /// Number of items skipped before the first one returned. Defaults to 0. Past the end, the page
    /// is empty but `total` is still the full count.
    #[param(minimum = 0, default = 0, example = 0)]
    offset: Option<u32>,
}

/// A page with its bounds resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub limit: u32,
    pub offset: u32,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            limit: DEFAULT_PAGE_SIZE,
            offset: 0,
        }
    }
}

impl PageParams {
    pub fn new(limit: Option<u32>, offset: Option<u32>) -> Self {
        Self { limit, offset }
    }

    pub fn page(&self) -> Page {
        Page {
            limit: self
                .limit
                .unwrap_or(DEFAULT_PAGE_SIZE)
                .clamp(1, MAX_PAGE_SIZE),
            offset: self.offset.unwrap_or(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_defaults_and_clamps() {
        assert_eq!(PageParams::default().page(), Page::default());
        assert_eq!(PageParams::new(Some(0), None).page().limit, 1);
        assert_eq!(
            PageParams::new(Some(10_000), None).page().limit,
            MAX_PAGE_SIZE
        );
        assert_eq!(
            PageParams::new(Some(20), Some(40)).page(),
            Page {
                limit: 20,
                offset: 40
            }
        );
    }
}
