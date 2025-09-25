// Copyright 2018 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

use hyper;
use rusqlite;
use serde_json;
use std;
use std::error::Error;
use uuid;

use db_traits::errors::DbError;

#[derive(Debug, thiserror::Error)]
pub enum TolstoyError {
    #[error("Received bad response from the remote: {0}")]
    BadRemoteResponse(String),

    // TODO expand this into concrete error types
    #[error("Received bad remote state: {0}")]
    BadRemoteState(String),

    #[error("encountered more than one metadata value for key: {0}")]
    DuplicateMetadata(String),

    #[error("transaction processor didn't say it was done")]
    TxProcessorUnfinished,

    #[error("expected one, found {0} uuid mappings for tx")]
    TxIncorrectlyMapped(usize),

    #[error("encountered unexpected state: {0}")]
    UnexpectedState(String),

    #[error("not yet implemented: {0}")]
    NotYetImplemented(String),

    #[error(transparent)]
    DbError(#[from] DbError),

    #[error(transparent)]
    SerializationError(#[from] serde_json::Error),

    // It would be better to capture the underlying `rusqlite::Error`, but that type doesn't
    // implement many useful traits, including `Clone`, `Eq`, and `PartialEq`.
    #[error("SQL error: {0}, cause: {1}")]
    RusqliteError(String, String),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error(transparent)]
    UuidError(#[from] uuid::Error),

    #[error(transparent)]
    NetworkError(#[from] hyper::Error),

    #[error(transparent)]
    UriError(#[from] http::uri::InvalidUri),
}

impl From<rusqlite::Error> for TolstoyError {
    fn from(error: rusqlite::Error) -> Self {
        let cause = match error.source() {
            Some(e) => e.to_string(),
            None => "".to_string(),
        };
        TolstoyError::RusqliteError(error.to_string(), cause)
    }
}
