use std::fmt;
use std::marker::PhantomData;

use serde::de::DeserializeOwned;

use crate::client::Client;
use crate::error::Error;

/// A response of a paged method: it says where the next page starts.
pub trait Paged {
    /// `data.nextCursor`: present while there are more pages, absent on the last.
    fn next_cursor(&self) -> Option<&str>;
}

/// Walks the pages of a paged method, one request per [`next_page`](Self::next_page).
///
/// The pager sends the same parameters with each page and passes `nextCursor`
/// back unchanged as `cursor`. It stops when `nextCursor` is absent — never on the
/// page's length: a page may hold fewer rows than `limit` and still have a next
/// one. A failed request is returned as an error; calling `next_page` again
/// repeats it.
pub struct Pager<T> {
    client: Client,
    path: String,
    query: Vec<(&'static str, String)>,
    done: bool,
    _page: PhantomData<fn() -> T>,
}

impl<T> Pager<T> {
    pub(crate) fn new(client: Client, path: String, query: Vec<(&'static str, String)>) -> Self {
        Self { client, path, query, done: false, _page: PhantomData }
    }
}

impl<T: DeserializeOwned + Paged> Pager<T> {
    /// The next page, or `None` once the last page has been returned.
    pub async fn next_page(&mut self) -> Result<Option<T>, Error> {
        if self.done {
            return Ok(None);
        }
        let page: T = self.client.get(&self.path, &self.query).await?;
        match page.next_cursor() {
            Some(cursor) if !cursor.is_empty() => {
                let cursor = cursor.to_string();
                self.query.retain(|(k, _)| *k != "cursor");
                self.query.push(("cursor", cursor));
            }
            _ => self.done = true,
        }
        Ok(Some(page))
    }
}

impl<T> fmt::Debug for Pager<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pager").field("path", &self.path).field("query", &self.query).field("done", &self.done).finish()
    }
}
