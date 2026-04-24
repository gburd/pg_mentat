# Migrating from Datomic to pg_mentat

## Overview

**pg_mentat** provides a Datomic-compatible Datalog database backed by PostgreSQL. This guide helps you migrate existing Datomic applications to pg_mentat with minimal code changes.

**Target audience**: Teams using Datomic (Free, Pro, or Cloud) who want to:
- Reduce costs by using PostgreSQL instead of Datomic infrastructure
- Gain operational simplicity with standard PostgreSQL tooling
- Keep their existing Datalog query code

**What to expect**: Most Datomic code works unchanged. Connection setup and transaction reports require minor adaptations.

