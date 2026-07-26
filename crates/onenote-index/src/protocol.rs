//! Versioned JSON Lines messages for non-Rust index clients.

use crate::{IndexProgress, SearchHit, SearchQuery, SourceStatus};
use onenote_core::SourceId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Current JSON Lines protocol generation.
pub const PROTOCOL_VERSION: u32 = 1;

/// One client request line.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RequestEnvelope {
    /// Protocol generation used to decode the envelope.
    pub protocol_version: u32,
    /// Opaque caller-provided correlation identifier.
    pub request_id: String,
    /// Requested operation.
    #[serde(flatten)]
    pub request: Request,
}

/// Supported protocol operations.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Request {
    /// Negotiate one protocol generation before all other operations.
    Hello {
        /// Generations the client can consume.
        supported_versions: Vec<u32>,
    },
    /// Parse and transactionally replace one native source.
    IndexSource {
        /// Native `.one` or `.onetoc2` source.
        path: PathBuf,
    },
    /// Remove one indexed source.
    RemoveSource {
        /// Stable source identity.
        source_id: SourceId,
    },
    /// Execute a bounded structured search.
    Search {
        /// Query shared with the Rust API.
        query: SearchQuery,
    },
    /// List published source generations.
    ListSources,
    /// Run database consistency checks.
    Verify,
    /// Request cancellation of a queued or active operation.
    Cancel {
        /// Request ID of the operation to cancel.
        target_request_id: String,
    },
    /// Finish queued work and close the adapter.
    Shutdown,
}

/// One server response or stream event line.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResponseEnvelope {
    /// Selected protocol generation.
    pub protocol_version: u32,
    /// Request this message belongs to.
    pub request_id: String,
    /// Response or stream event.
    #[serde(flatten)]
    pub response: Response,
}

impl ResponseEnvelope {
    /// Construct a version-1 response.
    pub fn new(request_id: impl Into<String>, response: Response) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            response,
        }
    }

    /// Construct a structured failure.
    pub fn error(
        request_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            request_id,
            Response::Error {
                code: code.into(),
                message: message.into(),
            },
        )
    }
}

/// Supported protocol responses and incremental events.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Response {
    /// Successful protocol negotiation.
    Hello {
        /// Generation selected by the server.
        selected_version: u32,
    },
    /// Incremental source-ingestion progress.
    Progress {
        /// Current page progress.
        progress: IndexProgress,
    },
    /// A source generation was published.
    Indexed {
        /// Published source status.
        source: SourceStatus,
    },
    /// Source removal completed.
    Removed {
        /// Whether the source existed.
        removed: bool,
    },
    /// One bounded result batch.
    Results {
        /// Ranked results in this batch.
        hits: Vec<SearchHit>,
        /// Whether this is the last batch.
        complete: bool,
    },
    /// Current published sources.
    Sources {
        /// Source generations.
        sources: Vec<SourceStatus>,
    },
    /// Integrity verification passed.
    Verified,
    /// Cancellation flag was set for a known operation.
    Cancelled {
        /// Target request.
        target_request_id: String,
        /// Whether the target was queued or active.
        accepted: bool,
    },
    /// Structured operation failure.
    Error {
        /// Stable machine-readable error category.
        code: String,
        /// Human-readable bounded detail.
        message: String,
    },
    /// Adapter shutdown completed.
    Goodbye,
}

#[cfg(test)]
mod tests {
    use super::{Request, RequestEnvelope, Response, ResponseEnvelope, PROTOCOL_VERSION};

    #[test]
    fn request_and_response_have_stable_tagged_shapes() {
        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "r1".to_owned(),
            request: Request::Hello {
                supported_versions: vec![1],
            },
        };
        assert_eq!(
            serde_json::to_string(&request).expect("request JSON"),
            r#"{"protocol_version":1,"request_id":"r1","operation":"hello","supported_versions":[1]}"#
        );
        let response = ResponseEnvelope::new(
            "r1",
            Response::Hello {
                selected_version: 1,
            },
        );
        assert_eq!(
            serde_json::to_string(&response).expect("response JSON"),
            r#"{"protocol_version":1,"request_id":"r1","event":"hello","selected_version":1}"#
        );
    }
}
