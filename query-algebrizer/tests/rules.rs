// Copyright 2016 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

use edn::query::{FnArg, PlainSymbol, Rule, RuleClause, RuleInvocation, Variable, WhereClause};
use mentat_query_algebrizer::RuleEnvironment;

#[test]
fn test_rule_environment_creation() {
    let env = RuleEnvironment::new();
    assert!(env.get_rule_clauses(&PlainSymbol::plain("test")).is_none());
}

#[test]
fn test_add_simple_rule() {
    let mut env = RuleEnvironment::new();

    let rule = Rule {
        name: PlainSymbol::plain("parent"),
        clauses: vec![RuleClause {
            head: RuleInvocation {
                name: PlainSymbol::plain("parent"),
                args: vec![
                    FnArg::Variable(Variable::from_valid_name("?p")),
                    FnArg::Variable(Variable::from_valid_name("?c")),
                ],
            },
            body: vec![],
        }],
    };

    assert!(env.add_rule(rule).is_ok());
    assert!(env
        .get_rule_clauses(&PlainSymbol::plain("parent"))
        .is_some());
}

#[test]
fn test_add_rule_inconsistent_arity() {
    let mut env = RuleEnvironment::new();

    let rule = Rule {
        name: PlainSymbol::plain("parent"),
        clauses: vec![
            RuleClause {
                head: RuleInvocation {
                    name: PlainSymbol::plain("parent"),
                    args: vec![
                        FnArg::Variable(Variable::from_valid_name("?p")),
                        FnArg::Variable(Variable::from_valid_name("?c")),
                    ],
                },
                body: vec![],
            },
            RuleClause {
                head: RuleInvocation {
                    name: PlainSymbol::plain("parent"),
                    args: vec![FnArg::Variable(Variable::from_valid_name("?p"))],
                },
                body: vec![],
            },
        ],
    };

    assert!(env.add_rule(rule).is_err());
}

#[test]
fn test_detect_non_recursive_rule() {
    let mut env = RuleEnvironment::new();

    // Simple non-recursive rule
    let rule = Rule {
        name: PlainSymbol::plain("sibling"),
        clauses: vec![RuleClause {
            head: RuleInvocation {
                name: PlainSymbol::plain("sibling"),
                args: vec![
                    FnArg::Variable(Variable::from_valid_name("?x")),
                    FnArg::Variable(Variable::from_valid_name("?y")),
                ],
            },
            body: vec![],
        }],
    };

    env.add_rule(rule).unwrap();
    assert!(!env.is_recursive(&PlainSymbol::plain("sibling")));
}

#[test]
fn test_detect_recursive_rule() {
    let mut env = RuleEnvironment::new();

    // Recursive ancestor rule
    let rule = Rule {
        name: PlainSymbol::plain("ancestor"),
        clauses: vec![RuleClause {
            head: RuleInvocation {
                name: PlainSymbol::plain("ancestor"),
                args: vec![
                    FnArg::Variable(Variable::from_valid_name("?p")),
                    FnArg::Variable(Variable::from_valid_name("?a")),
                ],
            },
            body: vec![WhereClause::RuleExpr(RuleInvocation {
                name: PlainSymbol::plain("ancestor"),
                args: vec![
                    FnArg::Variable(Variable::from_valid_name("?x")),
                    FnArg::Variable(Variable::from_valid_name("?a")),
                ],
            })],
        }],
    };

    env.add_rule(rule).unwrap();
    assert!(env.is_recursive(&PlainSymbol::plain("ancestor")));
}

#[test]
fn test_rule_with_multiple_clauses() {
    let mut env = RuleEnvironment::new();

    // Rule with multiple clauses (OR semantics)
    let rule = Rule {
        name: PlainSymbol::plain("family"),
        clauses: vec![
            RuleClause {
                head: RuleInvocation {
                    name: PlainSymbol::plain("family"),
                    args: vec![
                        FnArg::Variable(Variable::from_valid_name("?p")),
                        FnArg::Variable(Variable::from_valid_name("?c")),
                    ],
                },
                body: vec![],
            },
            RuleClause {
                head: RuleInvocation {
                    name: PlainSymbol::plain("family"),
                    args: vec![
                        FnArg::Variable(Variable::from_valid_name("?p")),
                        FnArg::Variable(Variable::from_valid_name("?c")),
                    ],
                },
                body: vec![],
            },
        ],
    };

    assert!(env.add_rule(rule).is_ok());
    let clauses = env.get_rule_clauses(&PlainSymbol::plain("family")).unwrap();
    assert_eq!(clauses.len(), 2);
}
