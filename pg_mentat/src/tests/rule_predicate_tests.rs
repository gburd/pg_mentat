// Tests for predicates in rule bodies

use pgrx::prelude::*;
use crate::functions::{mentat_transact, mentat_query};

#[pg_test]
fn test_rule_with_simple_predicate() {
    // Setup schema
    mentat_transact(r#"[
        {:db/id "schema1"
         :db/ident :person/age
         :db/valueType :db.type/long
         :db/cardinality :db.cardinality/one}
        {:db/id "schema2"
         :db/ident :person/name
         :db/valueType :db.type/string
         :db/cardinality :db.cardinality/one}
    ]"#).unwrap();

    // Setup data
    mentat_transact(r#"[
        {:db/id "t1" :person/name "Alice" :person/age 17}
        {:db/id "t2" :person/name "Bob" :person/age 25}
        {:db/id "t3" :person/name "Charlie" :person/age 35}
        {:db/id "t4" :person/name "David" :person/age 65}
    ]"#).unwrap();

    // Rule definition with predicate
    let rules = r#"%[
        [(adult ?person)
         [?person :person/age ?age]
         [(>= ?age 18)]]
    ]"#;

    // Query using rule
    let result = mentat_query(r#"
        [:find ?name
         :in $ %
         :where (adult ?person)
                [?person :person/name ?name]]
    "#, format!(r#"{{"rules": {}}}"#, rules).into()).unwrap();

    // Should return Bob, Charlie, and David (age >= 18), not Alice
    assert_eq!(result.len(), 3);

    let names: Vec<String> = result.iter()
        .map(|row| row[0].as_str().unwrap().to_string())
        .collect();

    assert!(names.contains(&"Bob".to_string()));
    assert!(names.contains(&"Charlie".to_string()));
    assert!(names.contains(&"David".to_string()));
    assert!(!names.contains(&"Alice".to_string()));
}

#[pg_test]
fn test_rule_with_multiple_predicates() {
    // Setup schema
    mentat_transact(r#"[
        {:db/id "schema1"
         :db/ident :person/age
         :db/valueType :db.type/long
         :db/cardinality :db.cardinality/one}
        {:db/id "schema2"
         :db/ident :person/name
         :db/valueType :db.type/string
         :db/cardinality :db.cardinality/one}
    ]"#).unwrap();

    // Setup data
    mentat_transact(r#"[
        {:db/id "t1" :person/name "Alice" :person/age 17}
        {:db/id "t2" :person/name "Bob" :person/age 25}
        {:db/id "t3" :person/name "Charlie" :person/age 35}
        {:db/id "t4" :person/name "David" :person/age 65}
    ]"#).unwrap();

    // Rule with multiple predicates (age range)
    let rules = r#"%[
        [(in-working-age ?person)
         [?person :person/age ?age]
         [(>= ?age 18)]
         [(<= ?age 65)]]
    ]"#;

    // Query using rule
    let result = mentat_query(r#"
        [:find ?name
         :in $ %
         :where (in-working-age ?person)
                [?person :person/name ?name]]
    "#, format!(r#"{{"rules": {}}}"#, rules).into()).unwrap();

    // Should return Bob and Charlie (18 <= age <= 65)
    assert_eq!(result.len(), 2);

    let names: Vec<String> = result.iter()
        .map(|row| row[0].as_str().unwrap().to_string())
        .collect();

    assert!(names.contains(&"Bob".to_string()));
    assert!(names.contains(&"Charlie".to_string()));
    assert!(!names.contains(&"Alice".to_string()));
    assert!(!names.contains(&"David".to_string()));
}

#[pg_test]
fn test_rule_with_arithmetic_function() {
    // Setup schema
    mentat_transact(r#"[
        {:db/id "schema1"
         :db/ident :product/price
         :db/valueType :db.type/double
         :db/cardinality :db.cardinality/one}
        {:db/id "schema2"
         :db/ident :product/name
         :db/valueType :db.type/string
         :db/cardinality :db.cardinality/one}
    ]"#).unwrap();

    // Setup data
    mentat_transact(r#"[
        {:db/id "p1" :product/name "Book" :product/price 10.0}
        {:db/id "p2" :product/name "Laptop" :product/price 1000.0}
        {:db/id "p3" :product/name "Pen" :product/price 2.0}
    ]"#).unwrap();

    // Rule with arithmetic function
    let rules = r#"%[
        [(discounted-price ?product ?final-price)
         [?product :product/price ?price]
         [(* ?price 0.9) ?final-price]]
    ]"#;

    // Query using rule
    let result = mentat_query(r#"
        [:find ?name ?final
         :in $ %
         :where (discounted-price ?product ?final)
                [?product :product/name ?name]]
    "#, format!(r#"{{"rules": {}}}"#, rules).into()).unwrap();

    assert_eq!(result.len(), 3);

    // Check discounted prices
    for row in result {
        let name = row[0].as_str().unwrap();
        let final_price = row[1].as_f64().unwrap();

        match name {
            "Book" => assert_eq!(final_price, 9.0),
            "Laptop" => assert_eq!(final_price, 900.0),
            "Pen" => assert_eq!(final_price, 1.8),
            _ => panic!("Unexpected product: {}", name),
        }
    }
}

#[pg_test]
fn test_recursive_rule_with_predicate() {
    // Setup schema
    mentat_transact(r#"[
        {:db/id "schema1"
         :db/ident :person/child
         :db/valueType :db.type/ref
         :db/cardinality :db.cardinality/many}
        {:db/id "schema2"
         :db/ident :person/age
         :db/valueType :db.type/long
         :db/cardinality :db.cardinality/one}
        {:db/id "schema3"
         :db/ident :person/name
         :db/valueType :db.type/string
         :db/cardinality :db.cardinality/one}
    ]"#).unwrap();

    // Setup data - family tree with ages
    mentat_transact(r#"[
        {:db/id "grandpa" :person/name "Grandpa" :person/age 70}
        {:db/id "parent" :person/name "Parent" :person/age 40}
        {:db/id "child" :person/name "Child" :person/age 10}
        {:db/id "grandpa" :person/child "parent"}
        {:db/id "parent" :person/child "child"}
    ]"#).unwrap();

    // Recursive rule with predicate: find ancestors older than a threshold
    let rules = r#"%[
        [(older-ancestor ?ancestor ?descendant)
         [?ancestor :person/child ?descendant]
         [?ancestor :person/age ?a-age]
         [?descendant :person/age ?d-age]
         [(> ?a-age ?d-age)]]

        [(older-ancestor ?ancestor ?descendant)
         (older-ancestor ?ancestor ?intermediate)
         [?intermediate :person/child ?descendant]
         [?ancestor :person/age ?a-age]
         [?descendant :person/age ?d-age]
         [(> ?a-age ?d-age)]]
    ]"#;

    // Query using recursive rule
    let result = mentat_query(r#"
        [:find ?a-name ?d-name
         :in $ %
         :where (older-ancestor ?a ?d)
                [?a :person/name ?a-name]
                [?d :person/name ?d-name]]
    "#, format!(r#"{{"rules": {}}}"#, rules).into()).unwrap();

    // Should find all ancestor-descendant pairs where ancestor is older
    assert!(result.len() > 0);

    let pairs: Vec<(String, String)> = result.iter()
        .map(|row| (row[0].as_str().unwrap().to_string(),
                    row[1].as_str().unwrap().to_string()))
        .collect();

    // Grandpa -> Parent (70 > 40)
    assert!(pairs.contains(&("Grandpa".to_string(), "Parent".to_string())));
    // Grandpa -> Child (70 > 10)
    assert!(pairs.contains(&("Grandpa".to_string(), "Child".to_string())));
    // Parent -> Child (40 > 10)
    assert!(pairs.contains(&("Parent".to_string(), "Child".to_string())));
}

#[pg_test]
fn test_rule_with_comparison_operators() {
    // Setup schema
    mentat_transact(r#"[
        {:db/id "schema1"
         :db/ident :score/value
         :db/valueType :db.type/long
         :db/cardinality :db.cardinality/one}
        {:db/id "schema2"
         :db/ident :score/name
         :db/valueType :db.type/string
         :db/cardinality :db.cardinality/one}
    ]"#).unwrap();

    // Setup data
    mentat_transact(r#"[
        {:db/id "s1" :score/name "Test1" :score/value 50}
        {:db/id "s2" :score/name "Test2" :score/value 75}
        {:db/id "s3" :score/name "Test3" :score/value 100}
        {:db/id "s4" :score/name "Test4" :score/value 25}
    ]"#).unwrap();

    // Test different comparison operators
    let rules = r#"%[
        [(passing-score ?test)
         [?test :score/value ?v]
         [(>= ?v 60)]]

        [(perfect-score ?test)
         [?test :score/value ?v]
         [(= ?v 100)]]

        [(needs-improvement ?test)
         [?test :score/value ?v]
         [(< ?v 50)]]
    ]"#;

    // Test passing scores
    let passing_result = mentat_query(r#"
        [:find ?name
         :in $ %
         :where (passing-score ?test)
                [?test :score/name ?name]]
    "#, format!(r#"{{"rules": {}}}"#, rules).into()).unwrap();

    assert_eq!(passing_result.len(), 2); // Test2 (75) and Test3 (100)

    // Test perfect scores
    let perfect_result = mentat_query(r#"
        [:find ?name
         :in $ %
         :where (perfect-score ?test)
                [?test :score/name ?name]]
    "#, format!(r#"{{"rules": {}}}"#, rules).into()).unwrap();

    assert_eq!(perfect_result.len(), 1); // Test3 (100)

    // Test needs improvement
    let improvement_result = mentat_query(r#"
        [:find ?name
         :in $ %
         :where (needs-improvement ?test)
                [?test :score/name ?name]]
    "#, format!(r#"{{"rules": {}}}"#, rules).into()).unwrap();

    assert_eq!(improvement_result.len(), 1); // Test4 (25)
}