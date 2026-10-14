//! HTTPS requests from a job to the hosts the manifest lists: GET, and
//! with `http.send` data sent with POST, PUT, PATCH, or DELETE.
//!
//! ```ignore
//! use angelica_sdk::http;
//!
//! // In `Plugin::run_job`; needs `http.get` and the host in `[http] hosts`.
//! let response = http::get("https://example.com/data.json")?;
//! if response.is_success() {
//!     let data: Data = response.json()?;
//! }
//! let (head, bytes) = http::Request::new("https://example.com/big.zip")
//!     .header("accept", "application/zip")
//!     .download("big.zip")?;
//! let posted = http::Request::new("https://example.com/api/notes")
//!     .header("content-type", "application/json")
//!     .send(http::Method::Post, br#"{"text":"hi"}"#)?;
//! ```
use serde::de::DeserializeOwned;

use crate::api::http as api;
pub use api::{Head, Header, Method, Request, Response};

/// Sends a GET request to `url` without headers. Called from a job.
///
/// # Errors
/// See `http.get`: a request that did not complete. Any status is a
/// response.
pub fn get(url: &str) -> Result<Response, crate::Error> {
    Request::new(url).get()
}

/// Downloads `url` into `path` in the plugin's storage when the status is
/// 2xx. Called from a job.
///
/// # Errors
/// See `http.download`.
pub fn download(url: &str, path: &str) -> Result<(Head, u64), crate::Error> {
    Request::new(url).download(path)
}

impl Request {
    /// A request for `url` without headers.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
        }
    }

    /// Adds a header: only `accept`, `accept-language`, `if-none-match`,
    /// `if-modified-since`, and `range`, and for [`Request::send`] also
    /// `authorization` and `content-type`, each once.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push(Header {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    /// Sends the request; see [`get`].
    ///
    /// # Errors
    /// See `http.get`.
    pub fn get(&self) -> Result<Response, crate::Error> {
        api::get(self)
    }

    /// Sends `body` with `method` and returns the response, whatever its
    /// status; set `content-type` with [`Request::header`]. Called from a
    /// job; needs `http.send`.
    ///
    /// # Errors
    /// See `http.send`.
    pub fn send(&self, method: Method, body: &[u8]) -> Result<Response, crate::Error> {
        api::send(method, self, body)
    }

    /// Sends the request and writes a 2xx body to `path`; see [`download`].
    ///
    /// # Errors
    /// See `http.download`.
    pub fn download(&self, path: &str) -> Result<(Head, u64), crate::Error> {
        api::download(self, path)
    }
}

impl Head {
    /// Whether the status is 2xx.
    #[must_use]
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The value of the first header named `name`, in lower case.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.name == name)
            .map(|header| header.value.as_str())
    }
}

impl Response {
    /// Whether the status is 2xx.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.head.is_success()
    }

    /// The body as UTF-8 text.
    ///
    /// # Errors
    /// `invalid-argument` when the body is not UTF-8.
    pub fn text(&self) -> Result<&str, crate::Error> {
        std::str::from_utf8(&self.body).map_err(|e| crate::Error {
            kind: crate::ErrorKind::InvalidArgument,
            message: format!("the response body is not UTF-8: {e}"),
        })
    }

    /// The body decoded from JSON.
    ///
    /// # Errors
    /// `invalid-argument` when the body is not JSON of a `T`.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, crate::Error> {
        serde_json::from_slice(&self.body).map_err(|e| crate::Error {
            kind: crate::ErrorKind::InvalidArgument,
            message: format!("the response body is not the expected JSON: {e}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, body: &[u8]) -> Response {
        Response {
            head: Head {
                status,
                headers: vec![Header {
                    name: "content-type".into(),
                    value: "application/json".into(),
                }],
            },
            body: body.to_vec(),
        }
    }

    #[test]
    fn requests_collect_their_headers() {
        let request = Request::new("https://example.com/").header("accept", "text/html");
        assert_eq!(request.url, "https://example.com/");
        assert_eq!(request.headers.len(), 1);
        assert_eq!(request.headers[0].name, "accept");
    }

    #[test]
    fn responses_report_status_headers_and_body() {
        let ok = response(204, br#"{"name":"Gil"}"#);
        assert!(ok.is_success());
        assert!(!response(304, b"").is_success());
        assert_eq!(ok.head.header("content-type"), Some("application/json"));
        assert_eq!(ok.head.header("etag"), None);
        assert_eq!(ok.text().unwrap(), r#"{"name":"Gil"}"#);
        let value: std::collections::BTreeMap<String, String> = ok.json().unwrap();
        assert_eq!(value["name"], "Gil");
    }

    #[test]
    fn bodies_that_are_not_what_was_expected_are_invalid() {
        let bad = response(200, b"\xFF");
        assert_eq!(
            bad.text().unwrap_err().kind,
            crate::ErrorKind::InvalidArgument
        );
        assert_eq!(
            bad.json::<u32>().unwrap_err().kind,
            crate::ErrorKind::InvalidArgument
        );
    }
}
