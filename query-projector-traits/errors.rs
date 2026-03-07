// Copyright 2018 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

use std; // To refer to std::result::Result.

use rusqlite;

use core_traits::ValueTypeSet;
use db_traits::errors::DbError;
use edn::query::PlainSymbol;
use query_pull_traits::errors::PullError;

use crate::aggregates::SimpleAggregationOp;

pub type Result<T> = std::result::Result<T, ProjectorError>;

#[derive(Debug, thiserror::Error)]
pub enum ProjectorError {
    /// We're just not done yet.  Message that the feature is recognized but not yet
    /// implemented.
    #[error("not yet implemented: {0}")]
    NotYetImplemented(String),

    #[error("no possible types for value provided to {:?}", 0)]
    CannotProjectImpossibleBinding(SimpleAggregationOp),

    #[error("cannot apply projection operation {:?} to types {:?}", _0, 1)]
    CannotApplyAggregateOperationToTypes(SimpleAggregationOp, ValueTypeSet),

    #[error("invalid projection: {0}")]
    InvalidProjection(String),

    #[error("cannot project unbound variable {:?}", 0)]
    UnboundVariable(PlainSymbol),

    #[error("cannot find type for variable {:?}", 0)]
    NoTypeAvailableForVariable(PlainSymbol),

    #[error("expected {0}, got {1}")]
    UnexpectedResultsType(&'static str, &'static str),

    #[error("expected tuple of length {}, got tuple of length {}", _0, 1)]
    UnexpectedResultsTupleLength(usize, usize),

    #[error("min/max expressions: {0} (max 1), corresponding: {1}")]
    AmbiguousAggregates(usize, usize),

    // It would be better to capture the underlying `rusqlite::Error`, but that type doesn't
    // implement many useful traits, including `Clone`, `Eq`, and `PartialEq`.
    #[error("SQL error: {0}")]
    RusqliteError(String),

    #[error(transparent)]
    DbError(#[from] DbError),

    #[error(transparent)]
    PullError(#[from] PullError),
}

impl From<rusqlite::Error> for ProjectorError {
    fn from(error: rusqlite::Error) -> ProjectorError {
        ProjectorError::RusqliteError(error.to_string())
    }
}
