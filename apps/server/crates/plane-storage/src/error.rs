// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Errors from object storage.
//!
//! Plane's `S3Storage` wrapper (`apps/api/plane/settings/storage.py`) catches
//! botocore's `ClientError` in four places and either logs-and-returns a falsy
//! value (`get_object_metadata`, `copy_object`, `upload_file`, `delete_files`)
//! or logs-and-returns `None` (`generate_presigned_url`). Everything else —
//! parameter validation, missing credentials, endpoint resolution, transport
//! errors, `S3UploadFailedError` from a failed multipart upload — propagates and
//! becomes a 500 on the Django side. This type keeps the same split:
//!
//! * an S3 *error response* (a 4xx/5xx that botocore turns into `ClientError`)
//!   is [`StorageError::S3`], which the four wrappers above swallow;
//! * everything else is returned to the caller, as Python's uncaught exception
//!   would be.
//!
//! Callers that port those wrappers must reproduce the swallowing explicitly;
//! the corresponding methods here do it for them (see [`crate::Storage`]).

/// What went wrong.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// `ValueError: Invalid endpoint: ...` from `botocore/endpoint.py:400`
    #[error("Invalid endpoint: {0}")]
    InvalidEndpoint(String),

    /// `InvalidRegionError` from `botocore/utils.py:1311`
    #[error("Provided region_name '{0}' doesn't match a supported format.")]
    InvalidRegion(String),

    /// `ParamValidationError` from the botocore parameter validator.
    #[error("Parameter validation failed: {0}")]
    InvalidParameter(String),

    /// `PartialCredentialsError` from `botocore/credentials.py`.
    #[error("Partial credentials found in {location}, missing: {missing}")]
    PartialCredentials {
        /// `explicit`, `env`, `shared-credentials-file`, ...
        location: String,
        /// The missing key name.
        missing: String,
    },

    /// `NoCredentialsError`.
    #[error("Unable to locate credentials")]
    NoCredentials,

    /// A metadata credential fetch failed (container or instance role).
    #[error("credential retrieval failed: {0}")]
    CredentialRetrieval(String),

    /// A configuration value Django parses with `int()` at the first use.
    #[error("{0}")]
    Config(String),

    /// A signed request could not be built (invalid header value, overflow).
    #[error("could not build request: {0}")]
    Request(String),

    /// The transport failed. In Python this is `EndpointConnectionError`,
    /// `ConnectTimeoutError`, `ReadTimeoutError`, ... — none of which the
    /// wrapper catches.
    #[error("request failed: {0}")]
    Transport(String),

    /// An S3 error response. botocore raises `ClientError` for these.
    #[error("S3 error {status} ({code}): {message}")]
    S3 {
        /// HTTP status code.
        status: u16,
        /// The `<Code>` element, or the status code as a string when the body
        /// is empty (which is how botocore parses a `HEAD` error).
        code: String,
        /// The `<Message>` element, or an empty string.
        message: String,
    },
}

impl StorageError {
    /// Whether botocore would surface this as `ClientError`.
    pub fn is_client_error(&self) -> bool {
        matches!(self, Self::S3 { .. })
    }

    /// The S3 `<Code>`, when this is an error response.
    pub fn s3_code(&self) -> Option<&str> {
        match self {
            Self::S3 { code, .. } => Some(code),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for StorageError {
    fn from(error: reqwest::Error) -> Self {
        Self::Transport(error.to_string())
    }
}

/// A short error code for the `<Error><Code>` element, as botocore parses it.
///
/// `botocore/parsers.py:BaseXMLResponseParser._do_error_parse` reads the XML
/// body; when there is none (a `HEAD` response, or a proxy's HTML error page)
/// `botocore/parsers.py:890` uses the HTTP status code as the code and the
/// status reason as the message.
pub(crate) fn parse_s3_error(status: u16, body: &[u8]) -> StorageError {
    let text = String::from_utf8_lossy(body);
    let code = xml_tag(&text, "Code").unwrap_or_else(|| status.to_string());
    let message = xml_tag(&text, "Message").unwrap_or_default();
    StorageError::S3 {
        status,
        code,
        message,
    }
}

/// Extract the text of the first `<tag>...</tag>` in `text`.
///
/// S3 error documents are small and well-formed; a full XML parser for two
/// elements would be a dependency for nothing. Namespaced or self-closing tags
/// never occur in an S3 error body.
fn xml_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    Some(decode_xml_entities(&text[start..end]))
}

fn decode_xml_entities(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_from_body() {
        let body = b"<?xml version=\"1.0\"?><Error><Code>NoSuchKey</Code><Message>The specified key does not exist.</Message></Error>";
        match parse_s3_error(404, body) {
            StorageError::S3 {
                status,
                code,
                message,
            } => {
                assert_eq!(status, 404);
                assert_eq!(code, "NoSuchKey");
                assert_eq!(message, "The specified key does not exist.");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn error_code_from_status_when_head_has_no_body() {
        match parse_s3_error(403, b"") {
            StorageError::S3 {
                status,
                code,
                message,
            } => {
                assert_eq!(status, 403);
                assert_eq!(code, "403");
                assert!(message.is_empty());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn error_entities_are_decoded() {
        let body = b"<Error><Code>X</Code><Message>a &amp; b &lt;c&gt;</Message></Error>";
        match parse_s3_error(400, body) {
            StorageError::S3 { message, .. } => assert_eq!(message, "a & b <c>"),
            other => panic!("unexpected {other:?}"),
        }
    }
}
