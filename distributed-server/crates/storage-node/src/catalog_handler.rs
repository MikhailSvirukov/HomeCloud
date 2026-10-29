//! Catalog page request handling.
//!
//! SRS: `FR-CAT-007 / 1`, `FR-CAT-008 / 1`, `FR-CAT-009 / 1`,
//! `FR-CAT-013 / 2`, `FR-CAT-014 / 2`, `FR-CAT-015 / 1`,
//! `INT-NET-061 / 1`, `INT-NET-064 / 1`, `INT-NET-069 / 1`, `INT-NET-080 / 1`.

use std::collections::BTreeMap;

use crate::catalog::CatalogRequest;
use crate::catalog_error::CatalogErrorCode;
use crate::catalog_exchange::{encode_catalog_page_response, CatalogExchangeError};
use crate::catalog_service::CatalogService;
use crate::metadata::UploadId;
use crate::raft::CommittedVersion;

/// Handles decoded catalog page requests against the committed catalog.
pub struct CatalogPageHandler {
    service: CatalogService,
}

/// Failure modes of catalog page request handling.
#[derive(Debug, thiserror::Error)]
pub enum CatalogHandlerError {
    /// The request carries only one of the snapshot token and cursor positions.
    #[error("the request must carry either both catalog tokens or neither")]
    InvalidTokens,
    /// The result envelope could not be encoded.
    #[error(transparent)]
    Encode(#[from] CatalogExchangeError),
}

impl CatalogPageHandler {
    /// Creates a handler over the catalog service.
    pub fn new(service: CatalogService) -> Self {
        Self { service }
    }

    /// Handles one decoded catalog request against the committed versions and returns the encoded result envelope.
    pub fn handle(
        &mut self,
        versions: &BTreeMap<UploadId, CommittedVersion>,
        request: &CatalogRequest,
    ) -> Result<Vec<u8>, CatalogHandlerError> {
        let page = match (request.snapshot_token, request.cursor) {
            (None, None) => self.service.first_page(versions),
            (Some(snapshot_token), Some(cursor)) => self.service.next_page(snapshot_token, cursor),
            _ => return Err(CatalogHandlerError::InvalidTokens),
        };

        match page {
            Ok(page) => Ok(encode_catalog_page_response(0, Some(&page))?),
            Err(error) => {
                let code = CatalogErrorCode::from(error);
                Ok(encode_catalog_page_response(u32::from(code), None)?)
            }
        }
    }
}
