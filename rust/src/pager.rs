use std::collections::HashSet;
use std::fmt;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::{Mutex, PoisonError};
use std::task::{Context, Poll, ready};

use serde::de::DeserializeOwned;

use crate::client::Client;
use crate::error::{Error, Result};

pub(crate) mod sealed {
    /// Only this crate's response types are [`Paged`](super::Paged).
    pub trait Sealed {}
}

/// A response of a paged method: it says where the next page starts and holds
/// the method's rows.
///
/// Implemented by the response types of the paged methods only (the trait is
/// sealed).
pub trait Paged: sealed::Sealed + DeserializeOwned + Send + Sync + Unpin + 'static {
    /// One row of the page (a transaction, a holder, an event, …).
    type Item: Send + Sync + Unpin + 'static;

    /// `data.nextCursor`: present while there are more pages, absent on the last.
    fn next_cursor(&self) -> Option<&str>;

    /// The page's rows.
    fn items(&self) -> &[Self::Item];

    /// The page's rows, taken out of the page.
    fn into_items(self) -> Vec<Self::Item>;
}

type PageFuture<T> = Pin<Box<dyn Future<Output = Result<T>> + Send>>;

/// Walks the pages of a paged method, one request per page.
///
/// The pager sends the same parameters with each page and passes `nextCursor`
/// back unchanged as `cursor`. It stops when `nextCursor` is absent — never on
/// the page's length: a page may hold fewer rows than `limit` and still have a
/// next one. A `nextCursor` that is empty or was seen before is an
/// `unexpected_response` error and ends the walk.
///
/// A failed request is returned as an error and the walk can go on: calling
/// [`next_page`](Self::next_page) again repeats the request. On
/// `400 invalid_cursor`, start again from the first page and discard what was
/// collected — for the feeds of one block it means the block was reorganised.
///
/// The pager owns a clone of the client, so it is `'static`, `Send` and `Sync`.
/// With the `stream` feature it is also a `futures_core::Stream` of
/// `Result<T>`.
pub struct Pager<T> {
    client: Client,
    path: String,
    query: Vec<(&'static str, String)>,
    refused: Option<Error>,
    seen: HashSet<String>,
    pages: usize,
    max_pages: Option<usize>,
    done: bool,
    // A `Mutex` only to make the pager `Sync`; it is reached through `&mut` and
    // never locked.
    in_flight: Mutex<Option<PageFuture<T>>>,
}

impl<T: Paged> Pager<T> {
    pub(crate) fn new(client: Client, path: Result<String>, query: Vec<(&'static str, String)>) -> Self {
        let (path, refused) = match path {
            Ok(path) => (path, None),
            Err(e) => (String::new(), Some(e)),
        };
        let seen = query.iter().filter(|(k, _)| *k == "cursor").map(|(_, v)| v.clone()).collect();
        Self { client, path, query, refused, seen, pages: 0, max_pages: None, done: false, in_flight: Mutex::new(None) }
    }

    /// Stops after this many pages, even when there are more.
    #[must_use]
    pub fn max_pages(mut self, max_pages: usize) -> Self {
        self.max_pages = Some(max_pages);
        self
    }

    /// Walks the rows of the pages, one by one, instead of whole pages.
    pub fn into_items(self) -> Items<T> {
        Items { pages: self, rows: Vec::new().into_iter(), yielded: 0, max_items: None }
    }

    /// The next page, or `None` once the last page has been returned (or
    /// `max_pages` reached).
    pub async fn next_page(&mut self) -> Result<Option<T>> {
        poll_fn(|cx| self.poll_next_page(cx)).await
    }

    fn poll_next_page(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<T>>> {
        if self.done {
            return Poll::Ready(Ok(None));
        }
        let in_flight = self.in_flight.get_mut().unwrap_or_else(PoisonError::into_inner);
        if in_flight.is_none() {
            if let Some(e) = self.refused.take() {
                self.done = true;
                return Poll::Ready(Err(e));
            }
            if self.max_pages.is_some_and(|max| self.pages >= max) {
                self.done = true;
                return Poll::Ready(Ok(None));
            }
            let (client, path, query) = (self.client.clone(), self.path.clone(), self.query.clone());
            *in_flight = Some(Box::pin(async move { client.get::<T>(&path, &query).await }));
        }
        let result = ready!(in_flight.as_mut().expect("a request in flight").as_mut().poll(cx));
        *in_flight = None;
        let page = result?;
        self.pages += 1;
        match page.next_cursor() {
            None => self.done = true,
            Some(cursor) if cursor.is_empty() || self.seen.contains(cursor) => {
                self.done = true;
                return Poll::Ready(Err(Error::new(200, "unexpected_response", "the answer gave an empty page cursor or repeated one")));
            }
            Some(cursor) => {
                self.seen.insert(cursor.to_string());
                self.query.retain(|(k, _)| *k != "cursor");
                self.query.push(("cursor", cursor.to_string()));
            }
        }
        Poll::Ready(Ok(Some(page)))
    }
}

impl<T> fmt::Debug for Pager<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pager")
            .field("path", &self.path)
            .field("query", &self.query)
            .field("pages", &self.pages)
            .field("max_pages", &self.max_pages)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

/// Walks the rows of a paged method one by one, fetching pages as needed.
///
/// It follows the pages exactly as [`Pager`] does. A failed request is returned
/// as an error and the walk can go on: calling [`next_item`](Self::next_item)
/// again repeats the request. With the `stream` feature it is also a
/// `futures_core::Stream` of `Result<T::Item>`.
pub struct Items<T: Paged> {
    pages: Pager<T>,
    rows: std::vec::IntoIter<T::Item>,
    yielded: usize,
    max_items: Option<usize>,
}

impl<T: Paged> Items<T> {
    /// Stops after this many rows, without fetching another page.
    #[must_use]
    pub fn max_items(mut self, max_items: usize) -> Self {
        self.max_items = Some(max_items);
        self
    }

    /// Stops after this many pages, even when there are more.
    #[must_use]
    pub fn max_pages(mut self, max_pages: usize) -> Self {
        self.pages = self.pages.max_pages(max_pages);
        self
    }

    /// The next row, or `None` once the last page is exhausted (or `max_items`
    /// or `max_pages` reached).
    pub async fn next_item(&mut self) -> Result<Option<T::Item>> {
        poll_fn(|cx| self.poll_next_item(cx)).await
    }

    fn poll_next_item(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<T::Item>>> {
        loop {
            if self.max_items.is_some_and(|max| self.yielded >= max) {
                return Poll::Ready(Ok(None));
            }
            if let Some(row) = self.rows.next() {
                self.yielded += 1;
                return Poll::Ready(Ok(Some(row)));
            }
            match ready!(self.pages.poll_next_page(cx)) {
                Ok(Some(page)) => self.rows = page.into_items().into_iter(),
                Ok(None) => return Poll::Ready(Ok(None)),
                Err(e) => return Poll::Ready(Err(e)),
            }
        }
    }
}

impl<T: Paged> fmt::Debug for Items<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Items")
            .field("pages", &self.pages)
            .field("buffered", &self.rows.len())
            .field("yielded", &self.yielded)
            .field("max_items", &self.max_items)
            .finish()
    }
}

#[cfg(feature = "stream")]
#[cfg_attr(docsrs, doc(cfg(feature = "stream")))]
impl<T: Paged> futures_core::Stream for Pager<T> {
    type Item = Result<T>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().poll_next_page(cx).map(Result::transpose)
    }
}

#[cfg(feature = "stream")]
#[cfg_attr(docsrs, doc(cfg(feature = "stream")))]
impl<T: Paged> futures_core::Stream for Items<T> {
    type Item = Result<T::Item>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().poll_next_item(cx).map(Result::transpose)
    }
}
