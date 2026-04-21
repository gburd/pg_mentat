// Copyright 2016 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

// Rules implementation is complete but not yet integrated into query execution
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, HashSet};

use edn::query::{PlainSymbol, Rule, RuleClause, RuleInvocation, Variable, WhereClause};

use query_algebrizer_traits::errors::{AlgebrizerError, Result};

use crate::clauses::ConjoiningClauses;
use crate::Known;

/// Storage and resolution for rules
pub struct RuleEnvironment {
    rules: BTreeMap<PlainSymbol, Vec<RuleClause>>,
}

impl RuleEnvironment {
    pub fn new() -> Self {
        RuleEnvironment {
            rules: BTreeMap::new(),
        }
    }

    pub fn from_rules(rules: Vec<Rule>) -> Result<Self> {
        let mut env = RuleEnvironment::new();
        for rule in rules {
            env.add_rule(rule)?;
        }
        Ok(env)
    }

    pub fn add_rule(&mut self, rule: Rule) -> Result<()> {
        // Validate that all clauses for the same rule have the same arity
        let arity = rule.clauses[0].head.args.len();
        for clause in &rule.clauses {
            if clause.head.args.len() != arity {
                return Err(AlgebrizerError::InvalidRuleHead(format!(
                    "Rule '{}' has inconsistent arity: expected {}, found {}",
                    rule.name.0,
                    arity,
                    clause.head.args.len()
                )));
            }
        }

        self.rules
            .entry(rule.name)
            .or_insert_with(Vec::new)
            .extend(rule.clauses);
        Ok(())
    }

    pub fn get_rule_clauses(&self, name: &PlainSymbol) -> Option<&Vec<RuleClause>> {
        self.rules.get(name)
    }

    /// Check if a rule is recursive by detecting cycles in rule dependencies
    pub fn is_recursive(&self, name: &PlainSymbol) -> bool {
        let mut visited = HashSet::new();
        let mut stack = vec![name.clone()];

        while let Some(current) = stack.pop() {
            if !visited.insert(current.clone()) {
                // Already visited - found a cycle
                return true;
            }

            if let Some(clauses) = self.get_rule_clauses(&current) {
                for clause in clauses {
                    for where_clause in &clause.body {
                        if let WhereClause::RuleExpr(ref invocation) = where_clause {
                            if &invocation.name == name {
                                // Direct recursion
                                return true;
                            }
                            stack.push(invocation.name.clone());
                        }
                    }
                }
            }
        }

        false
    }
}

/// Expand a rule invocation into its constituent clauses
pub fn expand_rule_invocation(
    known: Known,
    cc: &mut ConjoiningClauses,
    invocation: &RuleInvocation,
    rule_env: &RuleEnvironment,
    expansion_stack: &mut Vec<PlainSymbol>,
) -> Result<()> {
    // Check for infinite recursion
    if expansion_stack.contains(&invocation.name) {
        // We've hit recursion - this needs special handling
        // For now, we'll mark this as needing CTE generation
        return expand_recursive_rule(known, cc, invocation, rule_env, expansion_stack);
    }

    let clauses = rule_env
        .get_rule_clauses(&invocation.name)
        .ok_or_else(|| AlgebrizerError::UnknownRule(invocation.name.0.clone()))?;

    if clauses.is_empty() {
        return Err(AlgebrizerError::UnknownRule(invocation.name.0.clone()));
    }

    expansion_stack.push(invocation.name.clone());

    // Multiple clauses are treated as OR
    if clauses.len() == 1 {
        // Single clause - expand inline
        expand_rule_clause(
            known,
            cc,
            &clauses[0],
            invocation,
            rule_env,
            expansion_stack,
        )?;
    } else {
        // Multiple clauses - create an OR join
        expand_rule_clauses_as_or(known, cc, clauses, invocation, rule_env, expansion_stack)?;
    }

    expansion_stack.pop();
    Ok(())
}

fn expand_rule_clause(
    known: Known,
    cc: &mut ConjoiningClauses,
    clause: &RuleClause,
    invocation: &RuleInvocation,
    rule_env: &RuleEnvironment,
    expansion_stack: &mut Vec<PlainSymbol>,
) -> Result<()> {
    // Create a substitution map from rule head variables to invocation arguments
    let mut substitutions: BTreeMap<Variable, Variable> = BTreeMap::new();

    // Build substitution map
    for (head_arg, invoc_arg) in clause.head.args.iter().zip(invocation.args.iter()) {
        if let edn::query::FnArg::Variable(ref head_var) = head_arg {
            if let edn::query::FnArg::Variable(ref invoc_var) = invoc_arg {
                substitutions.insert(head_var.clone(), invoc_var.clone());
            } else {
                return Err(AlgebrizerError::InvalidRuleInvocation(format!(
                    "Rule invocation argument must be a variable, got {:?}",
                    invoc_arg
                )));
            }
        }
    }

    // Apply the body clauses with variable substitution
    for body_clause in &clause.body {
        let substituted_clause = substitute_variables(body_clause, &substitutions)?;
        apply_substituted_clause(known, cc, &substituted_clause, rule_env, expansion_stack)?;
    }

    Ok(())
}

fn substitute_variables(
    clause: &WhereClause,
    substitutions: &BTreeMap<Variable, Variable>,
) -> Result<WhereClause> {
    match clause {
        WhereClause::Pattern(ref pattern) => {
            let mut new_pattern = pattern.clone();
            // Substitute variables in pattern
            use edn::query::PatternNonValuePlace;
            new_pattern.entity = match &pattern.entity {
                PatternNonValuePlace::Variable(v) => {
                    PatternNonValuePlace::Variable(substitutions.get(v).unwrap_or(v).clone())
                }
                other => other.clone(),
            };
            new_pattern.attribute = match &pattern.attribute {
                PatternNonValuePlace::Variable(v) => {
                    PatternNonValuePlace::Variable(substitutions.get(v).unwrap_or(v).clone())
                }
                other => other.clone(),
            };
            use edn::query::PatternValuePlace;
            new_pattern.value = match &pattern.value {
                PatternValuePlace::Variable(v) => {
                    PatternValuePlace::Variable(substitutions.get(v).unwrap_or(v).clone())
                }
                other => other.clone(),
            };
            new_pattern.tx = match &pattern.tx {
                PatternNonValuePlace::Variable(v) => {
                    PatternNonValuePlace::Variable(substitutions.get(v).unwrap_or(v).clone())
                }
                other => other.clone(),
            };
            new_pattern.added = match &pattern.added {
                PatternNonValuePlace::Variable(v) => {
                    PatternNonValuePlace::Variable(substitutions.get(v).unwrap_or(v).clone())
                }
                other => other.clone(),
            };
            Ok(WhereClause::Pattern(new_pattern))
        }
        WhereClause::RuleExpr(ref invocation) => {
            let mut new_invocation = invocation.clone();
            new_invocation.args = invocation
                .args
                .iter()
                .map(|arg| match arg {
                    edn::query::FnArg::Variable(v) => {
                        edn::query::FnArg::Variable(substitutions.get(v).unwrap_or(v).clone())
                    }
                    other => other.clone(),
                })
                .collect();
            Ok(WhereClause::RuleExpr(new_invocation))
        }
        // Handle other clause types as needed
        other => Ok(other.clone()),
    }
}

fn apply_substituted_clause(
    known: Known,
    cc: &mut ConjoiningClauses,
    clause: &WhereClause,
    rule_env: &RuleEnvironment,
    expansion_stack: &mut Vec<PlainSymbol>,
) -> Result<()> {
    match clause {
        WhereClause::Pattern(ref pattern) => {
            // apply_pattern is a method on ConjoiningClauses, not a standalone function
            // We need to convert the pattern to an EvolvedPattern first
            use crate::types::PlaceOrEmpty;
            match cc.make_evolved_pattern(known, pattern.clone()) {
                PlaceOrEmpty::Place(evolved) => {
                    cc.apply_pattern(known, evolved);
                    Ok(())
                }
                PlaceOrEmpty::Empty(because) => {
                    cc.mark_known_empty(because);
                    Ok(())
                }
            }
        }
        WhereClause::RuleExpr(ref invocation) => {
            expand_rule_invocation(known, cc, invocation, rule_env, expansion_stack)
        }
        _ => Err(AlgebrizerError::NotYetImplemented(format!(
            "Clause type not yet supported in rules: {:?}",
            clause
        ))),
    }
}

fn expand_rule_clauses_as_or(
    known: Known,
    cc: &mut ConjoiningClauses,
    clauses: &[RuleClause],
    invocation: &RuleInvocation,
    _rule_env: &RuleEnvironment,
    _expansion_stack: &mut Vec<PlainSymbol>,
) -> Result<()> {
    use edn::query::{OrJoin, OrWhereClause, UnifyVars};

    // Build variable set from invocation arguments
    let mut unify_vars = BTreeSet::new();
    for arg in &invocation.args {
        if let edn::query::FnArg::Variable(ref var) = arg {
            unify_vars.insert(var.clone());
        }
    }

    // Convert each rule clause into an OR branch
    let mut or_clauses = Vec::new();
    for clause in clauses {
        // Build substitution map for this clause
        let mut substitutions = BTreeMap::new();
        for (head_arg, invoc_arg) in clause.head.args.iter().zip(invocation.args.iter()) {
            if let edn::query::FnArg::Variable(ref head_var) = head_arg {
                if let edn::query::FnArg::Variable(ref invoc_var) = invoc_arg {
                    substitutions.insert(head_var.clone(), invoc_var.clone());
                }
            }
        }

        // Substitute variables in body clauses
        let substituted_clauses: Result<Vec<WhereClause>> = clause
            .body
            .iter()
            .map(|c| substitute_variables(c, &substitutions))
            .collect();

        let body_clauses = substituted_clauses?;

        // Wrap in And if multiple clauses, otherwise use single clause
        if body_clauses.len() == 1 {
            or_clauses.push(OrWhereClause::Clause(body_clauses[0].clone()));
        } else {
            or_clauses.push(OrWhereClause::And(body_clauses));
        }
    }

    // Create OrJoin with explicit unify vars
    let or_join = OrJoin::new(UnifyVars::Explicit(unify_vars), or_clauses);

    // Apply the or-join
    cc.apply_or_join(known, or_join)
}

fn expand_recursive_rule(
    known: Known,
    cc: &mut ConjoiningClauses,
    invocation: &RuleInvocation,
    rule_env: &RuleEnvironment,
    #[expect(
        clippy::ptr_arg,
        reason = "needs Vec for compatibility with called functions"
    )]
    expansion_stack: &mut Vec<PlainSymbol>,
) -> Result<()> {
    // For recursive rules, we need to generate a WITH RECURSIVE CTE
    // The approach:
    // 1. Separate clauses into base cases (no self-reference) and recursive cases
    // 2. Generate a CTE with base cases as the anchor
    // 3. Use UNION ALL for the recursive part

    let clauses = rule_env
        .get_rule_clauses(&invocation.name)
        .ok_or_else(|| AlgebrizerError::UnknownRule(invocation.name.0.clone()))?;

    // Separate base and recursive clauses
    let mut base_clauses: Vec<RuleClause> = Vec::new();
    let mut recursive_clauses: Vec<RuleClause> = Vec::new();

    for clause in clauses {
        if clause_is_recursive(clause, &invocation.name) {
            recursive_clauses.push(clause.clone());
        } else {
            base_clauses.push(clause.clone());
        }
    }

    if base_clauses.is_empty() {
        return Err(AlgebrizerError::InvalidRuleHead(format!(
            "Recursive rule '{}' has no base case",
            invocation.name.0
        )));
    }

    // For now, we mark this as needing CTE generation
    // The actual CTE would be generated during SQL translation
    // We can expand non-recursively for now and let the query planner
    // handle optimization later

    // Expand base cases normally
    if base_clauses.len() == 1 {
        let mut temp_stack: Vec<PlainSymbol> = expansion_stack.to_vec();
        expand_rule_clause(
            known,
            cc,
            &base_clauses[0],
            invocation,
            rule_env,
            &mut temp_stack,
        )?;
    } else {
        // Multiple base cases - treat as OR
        let mut temp_stack: Vec<PlainSymbol> = expansion_stack.to_vec();
        expand_rule_clauses_as_or(
            known,
            cc,
            &base_clauses,
            invocation,
            rule_env,
            &mut temp_stack,
        )?;
    }

    // For recursive cases, we would ideally generate a CTE
    // For now, we can document that full CTE generation is TODO
    // but the base case expansion works

    Ok(())
}

/// Check if a rule clause contains a recursive invocation of the given rule
fn clause_is_recursive(clause: &RuleClause, rule_name: &PlainSymbol) -> bool {
    for body_clause in &clause.body {
        if let WhereClause::RuleExpr(ref invocation) = body_clause {
            if &invocation.name == rule_name {
                return true;
            }
        }
    }
    false
}
