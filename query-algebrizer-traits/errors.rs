// Copyright 2016 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

use std; // To refer to std::result::Result.

use core_traits::{ValueType, ValueTypeSet};

use edn::{query::PlainSymbol, ParseError};

pub type Result<T> = std::result::Result<T, AlgebrizerError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindingError {
    NoBoundVariable,
    UnexpectedBinding,
    RepeatedBoundVariable, // TODO: include repeated variable(s).

    /// Expected `[[?x ?y]]` but got some other type of binding.  Mentat is deliberately more strict
    /// than Datomic: we won't try to make sense of non-obvious (and potentially erroneous) bindings.
    ExpectedBindRel,

    /// Expected `[[?x ?y]]` or `[?x ...]` but got some other type of binding.  Mentat is
    /// deliberately more strict than Datomic: we won't try to make sense of non-obvious (and
    /// potentially erroneous) bindings.
    ExpectedBindRelOrBindColl,

    /// Expected `[?x1 … ?xN]` or `[[?x1 … ?xN]]` but got some other number of bindings.  Mentat is
    /// deliberately more strict than Datomic: we prefer placeholders to omission.
    InvalidNumberOfBindings {
        number: usize,
        expected: usize,
    },
}

#[derive(Clone, Debug, Eq, thiserror::Error, PartialEq)]
pub enum AlgebrizerError {
    #[error("{0} var {1} is duplicated")]
    DuplicateVariableError(PlainSymbol, &'static str),

    #[error("unexpected FnArg")]
    UnsupportedArgument,

    #[error(
        "value of type {} provided for var {}, expected {}",
        _0, 1, 2
    )]
    InputTypeDisagreement(PlainSymbol, ValueType, ValueType),

    #[error(
        "invalid number of arguments to {}: expected {}, got {}.",
        _0, 1, 2
    )]
    InvalidNumberOfArguments(PlainSymbol, usize, usize),

    #[error(
        "invalid argument to {}: expected {} in position {}.",
        _0, 1, 2
    )]
    InvalidArgument(PlainSymbol, &'static str, usize),

    #[error(
        "invalid argument to {}: expected one of {:?} in position {}.",
        _0, 1, 2
    )]
    InvalidArgumentType(PlainSymbol, ValueTypeSet, usize),

    // TODO: flesh this out.
    #[error("invalid expression in ground constant")]
    InvalidGroundConstant,

    #[error(
        "invalid limit {} of type {}: expected natural number.",
        _0, 1
    )]
    InvalidLimit(String, ValueType),

    #[error(
        "invalid offset {} of type {}: expected non-negative number.",
        _0, 1
    )]
    InvalidOffset(String, ValueType),

    #[error("mismatched bindings in ground")]
    GroundBindingsMismatch,

    #[error("no entid found for ident: {0}")]
    UnrecognizedIdent(String),

    #[error("no function named {0}")]
    UnknownFunction(PlainSymbol),

    #[error(":limit var {0} not present in :in")]
    UnknownLimitVar(PlainSymbol),

    #[error("unbound variable {0} in order clause or function call")]
    UnboundVariable(PlainSymbol),

    // TODO: flesh out.
    #[error("non-matching variables in 'or' clause")]
    NonMatchingVariablesInOrClause,

    #[error("non-matching variables in 'not' clause")]
    NonMatchingVariablesInNotClause,

    #[error("binding error in {0}: {1:?}")]
    InvalidBinding(PlainSymbol, BindingError),

    #[error("unknown rule: {0}")]
    UnknownRule(String),

    #[error("invalid rule head: {0}")]
    InvalidRuleHead(String),

    #[error("invalid rule invocation: {0}")]
    InvalidRuleInvocation(String),

    #[error("not yet implemented: {0}")]
    NotYetImplemented(String),

    #[error(transparent)]
    EdnParseError(#[from] ParseError),
}


