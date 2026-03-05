# Product Overview

Project Mentat is a persistent, embedded knowledge base inspired by DataScript and Datomic. It provides a flexible relational store that abstracts away storage schema complexity while exposing change listeners outside the database.

## Key Features

- **Persistent embedded database** built on SQLite with Datalog querying
- **Schema evolution** without complex migrations
- **Event sourcing** capabilities with transaction logs
- **Multi-platform support** with Rust core, Android/Java, Swift/iOS, and CLI interfaces
- **Sync capabilities** for distributed data scenarios

## Target Use Cases

- Applications needing flexible, evolvable data models
- Systems requiring both relational queries and graph-like data access
- Embedded databases where schema changes are frequent
- Applications benefiting from event sourcing patterns

## Architecture Philosophy

Mentat prioritizes persistence and performance over immutable databases-as-values, positioning itself as a practical approach to knowledge storage and access, similar to how SQLite serves as a practical RDBMS.
