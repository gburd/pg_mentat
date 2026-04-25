// Copyright 2016 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! Stratification checking for Datalog queries with negation.
//!
//! In Datalog, negation is only well-defined under stratified semantics: a rule
//! may not recursively depend on its own negation. This module checks that
//! constraint by building a dependency graph among rules, computing strongly
//! connected components (SCCs) via Tarjan's algorithm, and rejecting any SCC
//! that contains a negative edge.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use edn::query::{PlainSymbol, Rule, WhereClause};

use query_algebrizer_traits::errors::{AlgebrizerError, Result};

/// The polarity of a dependency between two rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DependencyKind {
    /// Rule A positively invokes rule B.
    Positive,
    /// Rule A invokes rule B inside a `not` / `not-join` clause.
    Negative,
}

/// A single directed edge in the rule dependency graph.
#[derive(Clone, Debug)]
pub struct Dependency {
    pub target: PlainSymbol,
    pub kind: DependencyKind,
}

/// The dependency graph for all rules in a query.
#[derive(Clone, Debug, Default)]
pub struct StratificationGraph {
    /// Adjacency list: rule name -> list of dependencies.
    pub edges: BTreeMap<PlainSymbol, Vec<Dependency>>,
    /// All rule names that appear in the graph (including those with no outgoing edges).
    pub nodes: BTreeSet<PlainSymbol>,
}

/// Result of stratification: an ordered list of strata (each stratum is a set
/// of rule names that can be evaluated together).
#[derive(Clone, Debug)]
pub struct Stratification {
    pub strata: Vec<BTreeSet<PlainSymbol>>,
}

// ---------------------------------------------------------------------------
// Graph construction
// ---------------------------------------------------------------------------

impl StratificationGraph {
    /// Build a dependency graph from a set of parsed rules.
    pub fn from_rules(rules: &[Rule]) -> Self {
        let mut graph = StratificationGraph::default();

        for rule in rules {
            graph.nodes.insert(rule.name.clone());

            for clause in &rule.clauses {
                for body_clause in &clause.body {
                    Self::collect_deps(&rule.name, body_clause, false, &mut graph);
                }
            }
        }

        graph
    }

    /// Build a dependency graph from top-level where clauses *and* rules.
    /// Top-level where clauses that reference rules via `RuleExpr` or negate
    /// rules via `NotJoin` containing `RuleExpr` also contribute edges.
    /// We treat the top-level query as a synthetic node named `__query__`.
    pub fn from_query(where_clauses: &[WhereClause], rules: &[Rule]) -> Self {
        let mut graph = Self::from_rules(rules);

        let query_node = PlainSymbol("__query__".to_string());
        graph.nodes.insert(query_node.clone());

        for clause in where_clauses {
            Self::collect_deps(&query_node, clause, false, &mut graph);
        }

        graph
    }

    /// Walk a single where clause and record any rule dependencies.
    fn collect_deps(
        source: &PlainSymbol,
        clause: &WhereClause,
        negated: bool,
        graph: &mut StratificationGraph,
    ) {
        match clause {
            WhereClause::RuleExpr(ref invocation) => {
                let kind = if negated {
                    DependencyKind::Negative
                } else {
                    DependencyKind::Positive
                };
                graph.nodes.insert(invocation.name.clone());
                graph
                    .edges
                    .entry(source.clone())
                    .or_default()
                    .push(Dependency {
                        target: invocation.name.clone(),
                        kind,
                    });
            }
            WhereClause::NotJoin(ref not_join) => {
                for inner in &not_join.clauses {
                    Self::collect_deps(source, inner, true, graph);
                }
            }
            WhereClause::OrJoin(ref or_join) => {
                for or_clause in &or_join.clauses {
                    match or_clause {
                        edn::query::OrWhereClause::Clause(ref wc) => {
                            Self::collect_deps(source, wc, negated, graph);
                        }
                        edn::query::OrWhereClause::And(ref wcs) => {
                            for wc in wcs {
                                Self::collect_deps(source, wc, negated, graph);
                            }
                        }
                    }
                }
            }
            // Pattern, Pred, WhereFn, TypeAnnotation -- no rule dependencies.
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Tarjan's SCC algorithm
// ---------------------------------------------------------------------------

struct TarjanState {
    index_counter: usize,
    stack: Vec<PlainSymbol>,
    on_stack: BTreeSet<PlainSymbol>,
    index: HashMap<PlainSymbol, usize>,
    lowlink: HashMap<PlainSymbol, usize>,
    sccs: Vec<Vec<PlainSymbol>>,
}

impl TarjanState {
    fn new() -> Self {
        TarjanState {
            index_counter: 0,
            stack: Vec::new(),
            on_stack: BTreeSet::new(),
            index: HashMap::new(),
            lowlink: HashMap::new(),
            sccs: Vec::new(),
        }
    }
}

fn tarjan_scc(graph: &StratificationGraph) -> Vec<Vec<PlainSymbol>> {
    let mut state = TarjanState::new();

    for node in &graph.nodes {
        if !state.index.contains_key(node) {
            strongconnect(node, graph, &mut state);
        }
    }

    state.sccs
}

fn strongconnect(v: &PlainSymbol, graph: &StratificationGraph, state: &mut TarjanState) {
    let v_index = state.index_counter;
    state.index.insert(v.clone(), v_index);
    state.lowlink.insert(v.clone(), v_index);
    state.index_counter += 1;
    state.stack.push(v.clone());
    state.on_stack.insert(v.clone());

    if let Some(deps) = graph.edges.get(v) {
        for dep in deps {
            let w = &dep.target;
            if !state.index.contains_key(w) {
                strongconnect(w, graph, state);
                let w_lowlink = state.lowlink[w];
                let v_lowlink = state.lowlink.get_mut(v).expect("v must be in lowlink");
                if w_lowlink < *v_lowlink {
                    *v_lowlink = w_lowlink;
                }
            } else if state.on_stack.contains(w) {
                let w_index = state.index[w];
                let v_lowlink = state.lowlink.get_mut(v).expect("v must be in lowlink");
                if w_index < *v_lowlink {
                    *v_lowlink = w_index;
                }
            }
        }
    }

    let v_lowlink = state.lowlink[v];
    let v_idx = state.index[v];
    if v_lowlink == v_idx {
        let mut scc = Vec::new();
        loop {
            let w = state.stack.pop().expect("stack should not be empty");
            state.on_stack.remove(&w);
            scc.push(w.clone());
            if w == *v {
                break;
            }
        }
        state.sccs.push(scc);
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Check whether an SCC contains a negative edge (an edge within the SCC
/// whose kind is `Negative`).
fn scc_has_negative_edge(scc: &[PlainSymbol], graph: &StratificationGraph) -> Option<(PlainSymbol, PlainSymbol)> {
    let members: BTreeSet<&PlainSymbol> = scc.iter().collect();

    for node in scc {
        if let Some(deps) = graph.edges.get(node) {
            for dep in deps {
                if dep.kind == DependencyKind::Negative && members.contains(&dep.target) {
                    return Some((node.clone(), dep.target.clone()));
                }
            }
        }
    }

    None
}

/// Validate that a set of rules and where clauses form a stratifiable program.
///
/// Returns `Ok(Stratification)` with the computed strata if the program is
/// stratifiable, or an error describing the problematic cycle if not.
pub fn validate_stratification(
    where_clauses: &[WhereClause],
    rules: &[Rule],
) -> Result<Stratification> {
    // No rules means no recursion possible -- trivially stratifiable.
    if rules.is_empty() {
        return Ok(Stratification {
            strata: vec![BTreeSet::new()],
        });
    }

    let graph = StratificationGraph::from_query(where_clauses, rules);
    let sccs = tarjan_scc(&graph);

    // Check each SCC for negative cycles.
    for scc in &sccs {
        if scc.len() == 1 {
            // A single-node SCC can still have a self-loop through negation.
            let node = &scc[0];
            if let Some(deps) = graph.edges.get(node) {
                for dep in deps {
                    if dep.kind == DependencyKind::Negative && dep.target == *node {
                        return Err(AlgebrizerError::RecursionThroughNegation(format!(
                            "Rule '{}' depends on negation of itself",
                            node.0,
                        )));
                    }
                }
            }
        } else if let Some((from, to)) = scc_has_negative_edge(&scc, &graph) {
            return Err(AlgebrizerError::RecursionThroughNegation(format!(
                "Rule '{}' depends on negation of '{}' within the same recursive component (rules in component: {})",
                from.0,
                to.0,
                scc.iter()
                    .map(|s| format!("'{}'", s.0))
                    .collect::<Vec<_>>()
                    .join(", "),
            )));
        }
    }

    // Compute strata via topological sort of the condensation graph.
    let strata = compute_strata(&sccs, &graph);

    Ok(Stratification { strata })
}

/// Validate that a set of rules alone (without query context) is stratifiable.
pub fn validate_rules_stratification(rules: &[Rule]) -> Result<Stratification> {
    validate_stratification(&[], rules)
}

// ---------------------------------------------------------------------------
// Strata computation
// ---------------------------------------------------------------------------

/// Given the SCCs (in reverse topological order from Tarjan) and the original
/// graph, compute strata. Rules with no negative dependencies between SCCs
/// can share a stratum; a negative edge between SCCs forces the target SCC
/// into a lower stratum.
fn compute_strata(
    sccs: &[Vec<PlainSymbol>],
    graph: &StratificationGraph,
) -> Vec<BTreeSet<PlainSymbol>> {
    // Map each node to its SCC index.
    let mut node_to_scc: HashMap<&PlainSymbol, usize> = HashMap::new();
    for (i, scc) in sccs.iter().enumerate() {
        for node in scc {
            node_to_scc.insert(node, i);
        }
    }

    let num_sccs = sccs.len();
    // stratum_level[i] = stratum number for SCC i.
    let mut stratum_level: Vec<usize> = vec![0; num_sccs];

    // Tarjan produces SCCs such that sinks come first (reverse topological
    // order of the condensation DAG). Process forward: leaves/sinks first,
    // then work up to roots, so that when we process an SCC its dependencies'
    // strata are already computed.
    for scc_idx in 0..num_sccs {
        for node in &sccs[scc_idx] {
            if let Some(deps) = graph.edges.get(node) {
                for dep in deps {
                    if let Some(&target_scc) = node_to_scc.get(&dep.target) {
                        if target_scc != scc_idx {
                            let base = stratum_level[target_scc];
                            let required = if dep.kind == DependencyKind::Negative {
                                base + 1
                            } else {
                                base
                            };
                            if required > stratum_level[scc_idx] {
                                stratum_level[scc_idx] = required;
                            }
                        }
                    }
                }
            }
        }
    }

    // Group SCCs by stratum level.
    let max_stratum = stratum_level.iter().copied().max().unwrap_or(0);
    let mut strata: Vec<BTreeSet<PlainSymbol>> = vec![BTreeSet::new(); max_stratum + 1];

    for (scc_idx, scc) in sccs.iter().enumerate() {
        let level = stratum_level[scc_idx];
        for node in scc {
            // Skip synthetic query node from output strata.
            if node.0 != "__query__" {
                strata[level].insert(node.clone());
            }
        }
    }

    // Remove empty strata.
    strata.retain(|s| !s.is_empty());
    if strata.is_empty() {
        strata.push(BTreeSet::new());
    }

    strata
}

#[cfg(test)]
mod tests {
    use super::*;
    use edn::query::{
        FnArg, NotJoin, Pattern, PatternNonValuePlace, PatternValuePlace, RuleClause,
        RuleInvocation, UnifyVars, Variable,
    };

    fn var(name: &str) -> Variable {
        Variable::from_valid_name(name)
    }

    fn rule_invocation(name: &str, args: Vec<&str>) -> RuleInvocation {
        RuleInvocation {
            name: PlainSymbol(name.to_string()),
            args: args.into_iter().map(|a| FnArg::Variable(var(a))).collect(),
        }
    }

    fn pattern_clause(entity: &str, attr_ns: &str, attr_name: &str, value: &str) -> WhereClause {
        WhereClause::Pattern(Pattern {
            source: None,
            entity: PatternNonValuePlace::Variable(var(entity)),
            attribute: PatternNonValuePlace::Ident(
                std::sync::Arc::new(edn::query::Keyword::namespaced(attr_ns, attr_name)),
            ),
            value: PatternValuePlace::Variable(var(value)),
            tx: PatternNonValuePlace::Placeholder,
            added: PatternNonValuePlace::Placeholder,
        })
    }

    fn make_rule(name: &str, clauses: Vec<RuleClause>) -> Rule {
        Rule {
            name: PlainSymbol(name.to_string()),
            clauses,
        }
    }

    fn make_clause(head_name: &str, head_args: Vec<&str>, body: Vec<WhereClause>) -> RuleClause {
        RuleClause {
            head: rule_invocation(head_name, head_args),
            body,
        }
    }

    // -----------------------------------------------------------------------
    // Safe: simple recursive rule (no negation)
    // -----------------------------------------------------------------------
    #[test]
    fn test_safe_recursive_rule() {
        // ancestor(?a, ?b) :- parent(?a, ?b).
        // ancestor(?a, ?b) :- parent(?a, ?c), ancestor(?c, ?b).
        let rules = vec![make_rule(
            "ancestor",
            vec![
                make_clause(
                    "ancestor",
                    vec!["?a", "?b"],
                    vec![pattern_clause("?a", "foo", "parent", "?b")],
                ),
                make_clause(
                    "ancestor",
                    vec!["?a", "?b"],
                    vec![
                        pattern_clause("?a", "foo", "parent", "?c"),
                        WhereClause::RuleExpr(rule_invocation("ancestor", vec!["?c", "?b"])),
                    ],
                ),
            ],
        )];

        let result = validate_rules_stratification(&rules);
        assert!(result.is_ok(), "Simple recursion without negation should be stratifiable");
        let strat = result.unwrap();
        assert!(!strat.strata.is_empty());
    }

    // -----------------------------------------------------------------------
    // Safe: negation of a different rule (no cycle through negation)
    // -----------------------------------------------------------------------
    #[test]
    fn test_safe_negation_of_different_rule() {
        // likes(?x, ?y) :- knows(?x, ?y), not dislikes(?x, ?y).
        // dislikes(?x, ?y) :- [?x :foo/dislikes ?y].
        let rules = vec![
            make_rule(
                "likes",
                vec![make_clause(
                    "likes",
                    vec!["?x", "?y"],
                    vec![
                        WhereClause::RuleExpr(rule_invocation("knows", vec!["?x", "?y"])),
                        WhereClause::NotJoin(NotJoin::new(
                            UnifyVars::Implicit,
                            vec![WhereClause::RuleExpr(rule_invocation(
                                "dislikes",
                                vec!["?x", "?y"],
                            ))],
                        )),
                    ],
                )],
            ),
            make_rule(
                "knows",
                vec![make_clause(
                    "knows",
                    vec!["?x", "?y"],
                    vec![pattern_clause("?x", "foo", "knows", "?y")],
                )],
            ),
            make_rule(
                "dislikes",
                vec![make_clause(
                    "dislikes",
                    vec!["?x", "?y"],
                    vec![pattern_clause("?x", "foo", "dislikes", "?y")],
                )],
            ),
        ];

        let result = validate_rules_stratification(&rules);
        assert!(result.is_ok(), "Negation of a non-recursive rule should be stratifiable");
        let strat = result.unwrap();
        // dislikes should be in a lower stratum than likes
        assert!(strat.strata.len() >= 2, "Expected at least 2 strata, got {}", strat.strata.len());
    }

    // -----------------------------------------------------------------------
    // Unsafe: self-recursion through negation
    // -----------------------------------------------------------------------
    #[test]
    fn test_unsafe_self_recursion_through_negation() {
        // p(?x) :- [?x :foo/bar ?y], not p(?y).
        let rules = vec![make_rule(
            "p",
            vec![make_clause(
                "p",
                vec!["?x"],
                vec![
                    pattern_clause("?x", "foo", "bar", "?y"),
                    WhereClause::NotJoin(NotJoin::new(
                        UnifyVars::Implicit,
                        vec![WhereClause::RuleExpr(rule_invocation("p", vec!["?y"]))],
                    )),
                ],
            )],
        )];

        let result = validate_rules_stratification(&rules);
        assert!(result.is_err(), "Self-recursion through negation should be rejected");
        match result.unwrap_err() {
            AlgebrizerError::RecursionThroughNegation(msg) => {
                assert!(msg.contains("p"), "Error should mention the rule name 'p', got: {}", msg);
            }
            other => panic!("Expected RecursionThroughNegation, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Unsafe: mutual recursion through negation
    // -----------------------------------------------------------------------
    #[test]
    fn test_unsafe_mutual_recursion_through_negation() {
        // a(?x) :- [?x :foo/bar ?y], not b(?y).
        // b(?x) :- [?x :foo/baz ?y], a(?y).
        // a depends negatively on b, b depends positively on a.
        // They form an SCC with a negative edge.
        let rules = vec![
            make_rule(
                "a",
                vec![make_clause(
                    "a",
                    vec!["?x"],
                    vec![
                        pattern_clause("?x", "foo", "bar", "?y"),
                        WhereClause::NotJoin(NotJoin::new(
                            UnifyVars::Implicit,
                            vec![WhereClause::RuleExpr(rule_invocation("b", vec!["?y"]))],
                        )),
                    ],
                )],
            ),
            make_rule(
                "b",
                vec![make_clause(
                    "b",
                    vec!["?x"],
                    vec![
                        pattern_clause("?x", "foo", "baz", "?y"),
                        WhereClause::RuleExpr(rule_invocation("a", vec!["?y"])),
                    ],
                )],
            ),
        ];

        let result = validate_rules_stratification(&rules);
        assert!(result.is_err(), "Mutual recursion through negation should be rejected");
        match result.unwrap_err() {
            AlgebrizerError::RecursionThroughNegation(msg) => {
                assert!(
                    msg.contains("a") || msg.contains("b"),
                    "Error should mention rule names, got: {}",
                    msg
                );
            }
            other => panic!("Expected RecursionThroughNegation, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Safe: mutual recursion without negation
    // -----------------------------------------------------------------------
    #[test]
    fn test_safe_mutual_recursion_without_negation() {
        // even(?x) :- [?x :foo/val 0].
        // even(?x) :- [?x :foo/succ ?y], odd(?y).
        // odd(?x) :- [?x :foo/succ ?y], even(?y).
        let rules = vec![
            make_rule(
                "even",
                vec![
                    make_clause(
                        "even",
                        vec!["?x"],
                        vec![pattern_clause("?x", "foo", "val", "?z")],
                    ),
                    make_clause(
                        "even",
                        vec!["?x"],
                        vec![
                            pattern_clause("?x", "foo", "succ", "?y"),
                            WhereClause::RuleExpr(rule_invocation("odd", vec!["?y"])),
                        ],
                    ),
                ],
            ),
            make_rule(
                "odd",
                vec![make_clause(
                    "odd",
                    vec!["?x"],
                    vec![
                        pattern_clause("?x", "foo", "succ", "?y"),
                        WhereClause::RuleExpr(rule_invocation("even", vec!["?y"])),
                    ],
                )],
            ),
        ];

        let result = validate_rules_stratification(&rules);
        assert!(result.is_ok(), "Mutual recursion without negation should be stratifiable");
    }

    // -----------------------------------------------------------------------
    // Multi-stratum: A -> not B, B -> not C (chain of negation, no cycle)
    // -----------------------------------------------------------------------
    #[test]
    fn test_multi_stratum_chain() {
        // c(?x) :- [?x :foo/base ?y].
        // b(?x) :- [?x :foo/mid ?y], not c(?y).
        // a(?x) :- [?x :foo/top ?y], not b(?y).
        let rules = vec![
            make_rule(
                "c",
                vec![make_clause(
                    "c",
                    vec!["?x"],
                    vec![pattern_clause("?x", "foo", "base", "?y")],
                )],
            ),
            make_rule(
                "b",
                vec![make_clause(
                    "b",
                    vec!["?x"],
                    vec![
                        pattern_clause("?x", "foo", "mid", "?y"),
                        WhereClause::NotJoin(NotJoin::new(
                            UnifyVars::Implicit,
                            vec![WhereClause::RuleExpr(rule_invocation("c", vec!["?y"]))],
                        )),
                    ],
                )],
            ),
            make_rule(
                "a",
                vec![make_clause(
                    "a",
                    vec!["?x"],
                    vec![
                        pattern_clause("?x", "foo", "top", "?y"),
                        WhereClause::NotJoin(NotJoin::new(
                            UnifyVars::Implicit,
                            vec![WhereClause::RuleExpr(rule_invocation("b", vec!["?y"]))],
                        )),
                    ],
                )],
            ),
        ];

        let result = validate_rules_stratification(&rules);
        assert!(result.is_ok(), "Chain of negation (no cycle) should be stratifiable");
        let strat = result.unwrap();
        // Should have 3 strata: c at level 0, b at level 1, a at level 2
        assert!(
            strat.strata.len() >= 3,
            "Expected at least 3 strata for chain a -> not b -> not c, got {}",
            strat.strata.len()
        );
    }

    // -----------------------------------------------------------------------
    // No rules: trivially stratifiable
    // -----------------------------------------------------------------------
    #[test]
    fn test_no_rules() {
        let result = validate_stratification(&[], &[]);
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // Top-level query negating a rule
    // -----------------------------------------------------------------------
    #[test]
    fn test_top_level_negation_of_rule() {
        // Query: [:find ?x :where (ancestor ?x ?y) (not (ancestor ?y ?x))]
        // Rules: ancestor is recursive but the negation is at the top level,
        // not inside the rule. This creates:
        //   __query__ --positive--> ancestor
        //   __query__ --negative--> ancestor
        // ancestor is NOT in the same SCC as __query__ unless __query__
        // also depends back on itself. Since __query__ is not a real rule,
        // this is fine -- the negation is of a lower stratum.
        let where_clauses = vec![
            WhereClause::RuleExpr(rule_invocation("ancestor", vec!["?x", "?y"])),
            WhereClause::NotJoin(NotJoin::new(
                UnifyVars::Implicit,
                vec![WhereClause::RuleExpr(rule_invocation(
                    "ancestor",
                    vec!["?y", "?x"],
                ))],
            )),
        ];

        let rules = vec![make_rule(
            "ancestor",
            vec![
                make_clause(
                    "ancestor",
                    vec!["?a", "?b"],
                    vec![pattern_clause("?a", "foo", "parent", "?b")],
                ),
                make_clause(
                    "ancestor",
                    vec!["?a", "?b"],
                    vec![
                        pattern_clause("?a", "foo", "parent", "?c"),
                        WhereClause::RuleExpr(rule_invocation("ancestor", vec!["?c", "?b"])),
                    ],
                ),
            ],
        )];

        let result = validate_stratification(&where_clauses, &rules);
        assert!(
            result.is_ok(),
            "Top-level negation of a recursive rule should be stratifiable (negation is outside the recursion)"
        );
    }

    // -----------------------------------------------------------------------
    // Graph construction
    // -----------------------------------------------------------------------
    #[test]
    fn test_graph_construction() {
        let rules = vec![
            make_rule(
                "a",
                vec![make_clause(
                    "a",
                    vec!["?x"],
                    vec![
                        WhereClause::RuleExpr(rule_invocation("b", vec!["?x"])),
                        WhereClause::NotJoin(NotJoin::new(
                            UnifyVars::Implicit,
                            vec![WhereClause::RuleExpr(rule_invocation("c", vec!["?x"]))],
                        )),
                    ],
                )],
            ),
            make_rule(
                "b",
                vec![make_clause(
                    "b",
                    vec!["?x"],
                    vec![pattern_clause("?x", "foo", "bar", "?y")],
                )],
            ),
            make_rule(
                "c",
                vec![make_clause(
                    "c",
                    vec!["?x"],
                    vec![pattern_clause("?x", "foo", "baz", "?y")],
                )],
            ),
        ];

        let graph = StratificationGraph::from_rules(&rules);

        assert_eq!(graph.nodes.len(), 3);
        assert!(graph.nodes.contains(&PlainSymbol("a".to_string())));
        assert!(graph.nodes.contains(&PlainSymbol("b".to_string())));
        assert!(graph.nodes.contains(&PlainSymbol("c".to_string())));

        let a_deps = graph.edges.get(&PlainSymbol("a".to_string())).unwrap();
        assert_eq!(a_deps.len(), 2);
        assert!(a_deps
            .iter()
            .any(|d| d.target == PlainSymbol("b".to_string())
                && d.kind == DependencyKind::Positive));
        assert!(a_deps
            .iter()
            .any(|d| d.target == PlainSymbol("c".to_string())
                && d.kind == DependencyKind::Negative));

        // b and c have no outgoing edges
        assert!(graph
            .edges
            .get(&PlainSymbol("b".to_string()))
            .map_or(true, |v| v.is_empty()));
        assert!(graph
            .edges
            .get(&PlainSymbol("c".to_string()))
            .map_or(true, |v| v.is_empty()));
    }

    // -----------------------------------------------------------------------
    // Tarjan SCC on a simple cycle
    // -----------------------------------------------------------------------
    #[test]
    fn test_tarjan_simple_cycle() {
        let mut graph = StratificationGraph::default();
        let a = PlainSymbol("a".to_string());
        let b = PlainSymbol("b".to_string());

        graph.nodes.insert(a.clone());
        graph.nodes.insert(b.clone());

        graph.edges.entry(a.clone()).or_default().push(Dependency {
            target: b.clone(),
            kind: DependencyKind::Positive,
        });
        graph.edges.entry(b.clone()).or_default().push(Dependency {
            target: a.clone(),
            kind: DependencyKind::Positive,
        });

        let sccs = tarjan_scc(&graph);

        // a and b should be in the same SCC
        let mut found = false;
        for scc in &sccs {
            if scc.contains(&a) && scc.contains(&b) {
                found = true;
                break;
            }
        }
        assert!(found, "a and b should be in the same SCC");
    }

    // -----------------------------------------------------------------------
    // Edge case: rule with negation of non-existent rule in body
    // (The graph should still include the node for the target)
    // -----------------------------------------------------------------------
    #[test]
    fn test_negation_of_undefined_rule_in_graph() {
        let rules = vec![make_rule(
            "p",
            vec![make_clause(
                "p",
                vec!["?x"],
                vec![
                    pattern_clause("?x", "foo", "bar", "?y"),
                    WhereClause::NotJoin(NotJoin::new(
                        UnifyVars::Implicit,
                        vec![WhereClause::RuleExpr(rule_invocation("q", vec!["?y"]))],
                    )),
                ],
            )],
        )];

        let graph = StratificationGraph::from_rules(&rules);
        // q should appear as a node even though it's not defined as a rule
        assert!(graph.nodes.contains(&PlainSymbol("q".to_string())));

        // This should still be stratifiable since there's no cycle
        let result = validate_rules_stratification(&rules);
        assert!(result.is_ok());
    }
}
