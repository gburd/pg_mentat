// Copyright 2016 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

#![allow(dead_code)]



use std::collections::{BTreeMap, BTreeSet};

use rusqlite;

use edn::entities::TempId;

use core_traits::{Entid, KnownEntid, TypedValue, ValueType};

pub type Result<T> = ::std::result::Result<T, DbError>;

// TODO Error/ErrorKind pair
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CardinalityConflict {
    /// A cardinality one attribute has multiple assertions `[e a v1], [e a v2], ...`.
    CardinalityOneAddConflict {
        e: Entid,
        a: Entid,
        vs: BTreeSet<TypedValue>,
    },

    /// A datom has been both asserted and retracted, like `[:db/add e a v]` and `[:db/retract e a v]`.
    AddRetractConflict {
        e: Entid,
        a: Entid,
        vs: BTreeSet<TypedValue>,
    },
}

// TODO Error/ErrorKind pair
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SchemaConstraintViolation {
    /// A transaction tried to assert datoms where one tempid upserts to two (or more) distinct
    /// entids.
    ConflictingUpserts {
        /// A map from tempid to the entids it would upsert to.
        ///
        /// In the future, we might even be able to attribute the upserts to particular (reduced)
        /// datoms, i.e., to particular `[e a v]` triples that caused the constraint violation.
        /// Attributing constraint violations to input data is more difficult to the multiple
        /// rewriting passes the input undergoes.
        conflicting_upserts: BTreeMap<TempId, BTreeSet<KnownEntid>>,
    },

    /// A transaction tried to assert a datom or datoms with the wrong value `v` type(s).
    TypeDisagreements {
        /// The key (`[e a v]`) has an invalid value `v`: it is not of the expected value type.
        conflicting_datoms: BTreeMap<(Entid, Entid, TypedValue), ValueType>,
    },

    /// A transaction tried to assert datoms that don't observe the schema's cardinality constraints.
    CardinalityConflicts { conflicts: Vec<CardinalityConflict> },
}

impl ::std::fmt::Display for SchemaConstraintViolation {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        use self::SchemaConstraintViolation::*;
        match self {
            ConflictingUpserts {
                ref conflicting_upserts,
            } => {
                writeln!(f, "conflicting upserts:")?;
                for (tempid, entids) in conflicting_upserts {
                    writeln!(f, "  tempid {:?} upserts to {:?}", tempid, entids)?;
                }
                Ok(())
            }
            TypeDisagreements {
                ref conflicting_datoms,
            } => {
                writeln!(f, "type disagreements:")?;
                for (ref datom, expected_type) in conflicting_datoms {
                    writeln!(
                        f,
                        "  expected value of type {} but got datom [{} {} {:?}]",
                        expected_type, datom.0, datom.1, datom.2
                    )?;
                }
                Ok(())
            }
            CardinalityConflicts { ref conflicts } => {
                writeln!(f, "cardinality conflicts:")?;
                for conflict in conflicts {
                    writeln!(f, "  {:?}", conflict)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, thiserror::Error)]
pub enum InputError {
    /// Map notation included a bad `:db/id` value.
    BadDbId,

    /// A value place cannot be interpreted as an entity place (for example, in nested map
    /// notation).
    BadEntityPlace,
}

impl ::std::fmt::Display for InputError {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        use self::InputError::*;
        match self {
            BadDbId => {
                writeln!(f, ":db/id in map notation must either not be present or be an entid, an ident, or a tempid")
            }
            BadEntityPlace => {
                writeln!(f, "cannot convert value place into entity place")
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct DbError(#[from] pub DbErrorKind);

impl DbError {
    pub fn kind(&self) -> &DbErrorKind {
        &self.0
    }
}

impl From<rusqlite::Error> for DbError {
    fn from(error: rusqlite::Error) -> Self {
        DbError(DbErrorKind::RusqliteError(error.to_string()))
    }
}

#[derive(Clone, PartialEq, Debug, thiserror::Error)]
pub enum DbErrorKind {
    /// We're just not done yet.  Recognized a feature that is not yet implemented.
    #[error("not yet implemented: {0}")]
    NotYetImplemented(String),

    /// We've been given a value that isn't the correct Mentat type.
    #[error("value '{0}' is not the expected Mentat value type {1:?}")]
    BadValuePair(String, ValueType),

    /// We've got corrupt data in the SQL store: a value and value_type_tag don't line up.
    /// TODO _1.data_type()
    #[error("bad SQL (value_type_tag, value) pair: ({:?}, {:?})", 0, 1)]
    BadSQLValuePair(rusqlite::types::Value, i32),

    /// The SQLite store user_version isn't recognized.  This could be an old version of Mentat
    /// trying to open a newer version SQLite store; or it could be a corrupt file; or ...
    /// #[error("bad SQL store user_version: {}", 0)]
    /// BadSQLiteStoreVersion(i32),
    /// A bootstrap definition couldn't be parsed or installed.  This is a programmer error, not
    /// a runtime error.
    #[error("bad bootstrap definition: {0}")]
    BadBootstrapDefinition(String),

    /// A schema assertion couldn't be parsed.
    #[error("bad schema assertion: {0}")]
    BadSchemaAssertion(String),

    /// An ident->entid mapping failed.
    #[error("no entid found for ident: {0}")]
    UnrecognizedIdent(String),

    /// An entid->ident mapping failed.
    #[error("no ident found for entid: {0}")]
    UnrecognizedEntid(Entid),

    /// Tried to transact an entid that isn't allocated.
    #[error("entid not allocated: {0}")]
    UnallocatedEntid(Entid),

    #[error("unknown attribute for entid: {0}")]
    UnknownAttribute(Entid),

    #[error("cannot reverse-cache non-unique attribute: {0}")]
    CannotCacheNonUniqueAttributeInReverse(Entid),

    #[error("schema alteration failed: {0}")]
    SchemaAlterationFailed(String),

    /// A transaction tried to violate a constraint of the schema of the Mentat store.
    #[error("schema constraint violation: {0}")]
    SchemaConstraintViolation(SchemaConstraintViolation),

    /// The transaction was malformed in some way (that was not recognized at parse time; for
    /// example, in a way that is schema-dependent).
    #[error("transaction input error: {0}")]
    InputError(InputError),

    #[error(
        "Cannot transact a fulltext assertion with a typed value that is not :db/valueType :db.type/string"
    )]
    WrongTypeValueForFtsAssertion,

    // SQL errors.
    #[error("could not update a cache")]
    CacheUpdateFailed,

    #[error("Could not set_user_version")]
    CouldNotSetVersionPragma,

    #[error("Could not get_user_version")]
    CouldNotGetVersionPragma,

    #[error("Could not search!")]
    CouldNotSearch,

    #[error("Could not insert transaction: failed to add datoms not already present")]
    TxInsertFailedToAddMissingDatoms,

    #[error("Could not insert transaction: failed to retract datoms already present")]
    TxInsertFailedToRetractDatoms,

    #[error("Could not update datoms: failed to retract datoms already present")]
    DatomsUpdateFailedToRetract,

    #[error("Could not update datoms: failed to add datoms not already present")]
    DatomsUpdateFailedToAdd,

    #[error("Failed to create temporary tables")]
    FailedToCreateTempTables,

    #[error("Could not insert non-fts one statements into temporary search table!")]
    NonFtsInsertionIntoTempSearchTableFailed,

    #[error("Could not insert fts values into fts table!")]
    FtsInsertionFailed,

    #[error("Could not insert FTS statements into temporary search table!")]
    FtsInsertionIntoTempSearchTableFailed,

    #[error("Could not drop FTS search ids!")]
    FtsFailedToDropSearchIds,

    #[error("Could not update partition map")]
    FailedToUpdatePartitionMap,

    #[error("Can't operate over mixed timelines")]
    TimelinesMixed,

    #[error("Can't move transactions to a non-empty timeline")]
    TimelinesMoveToNonEmpty,

    #[error("Supplied an invalid transaction range")]
    TimelinesInvalidRange,

    // It would be better to capture the underlying `rusqlite::Error`, but that type doesn't
    // implement many useful traits, including `Clone`, `Eq`, and `PartialEq`.
    #[error("SQL error: {0}")]
    RusqliteError(String),
}
