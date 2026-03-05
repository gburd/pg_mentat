# Unreleased

## Dependency Updates

* Updated all Cargo dependencies to latest stable versions:
  * rusqlite: 0.37 → 0.38.0
  * tokio: 1.8.0 → 1.50.0
  * hyper: 1.7 → 1.8.1
  * hyper-util: 0.1 → 0.1.20
  * hyper-tls: 0.6 → 0.6.0
  * http: 1.3 → 1.4.0
  * http-body-util: 0.1 → 0.1.3
  * bytes: 1.0 → 1.11.1
  * futures: 0.3 → 0.3.32
  * uuid: 1.18 → 1.21.0
  * chrono: 0.4 (unchanged - still latest stable)
  * thiserror: 2.0 → 2.0.18
  * time: 0.3 → 0.3.47
  * indexmap: 2.11 → 2.13.0
  * itertools: 0.14 → 0.14.0
  * ordered-float: 5.0 → 5.1.0
  * petgraph: 0.8 → 0.8.3
  * serde: 1.0 → 1.0.228
  * serde_json: 1.0 → 1.0.149
  * serde_derive: 1.0 → 1.0.228
  * serde_cbor: 0.11 → 0.11.2
  * serde_test: 1.0 → 1.0.177
  * lazy_static: 1.5 → 1.5.0
  * log: 0.4 → 0.4.29
  * mime: 0.3 → 0.3.17
  * env_logger: 0.11 → 0.11.9
  * tabwriter: 1.4 → 1.4.1
  * combine: 4.6 → 4.6.7
  * dirs: 4.0 → 6.0.0
  * getopts: 0.2 → 0.2.24
  * linefeed: 0.6 → 0.6.0
  * tempfile: 3.23 → 3.26.0
  * termion: 1.5 → 4.0.6
  * hex: 0.4.3 (unchanged)
  * num: 0.4 → 0.4.3
  * pretty: 0.12 → 0.12.5
  * peg: 0.8 → 0.8.5
  * libc: 0.2 → 0.2.182
  * enum-set: 0.0.8 (unchanged)

* Updated MSRV to Rust 1.88
* Note: termion upgraded from 1.5 to 4.0.6 - this is a major version bump that may include breaking changes

# 0.11.1 (2018-08-09)

* sdks/android compiled against:
  * Kotlin standard library 1.2.41

* **API changes**: Changed wording of MentatError::ConflictingAttributeDefinitions, MentatError::ExistingVocabularyTooNew, MentatError::UnexpectedCoreSchema.

* [Commits](https://github.com/mozilla/mentat/compare/v0.11.0...v0.11.1)

# 0.11 (2018-07-31)

* sdks/android compiled against:
  * Kotlin standard library 1.2.41

* **sdks/android**: `Mentat()` constructor replaced with `open` factory method.

* [Commits](https://github.com/mozilla/mentat/compare/v0.10.0...v0.11.0)

# 0.10 (2018-07-26)

* sdks/android compiled against:
  * Kotlin standard library 1.2.41

* **API changes**:
  * `store_open{_encrypted}` now accepts an error parameter; corresponding constructors changed to be factory functions.

* [Commits](https://github.com/mozilla/mentat/compare/v0.9.0...v0.10.0)

# 0.9 (2018-07-25)

* sdks/android compiled against:
  * Kotlin standard library 1.2.41

* **API changes**:
  * Mentat partitions now enforce their integrity, denying entids that aren't already known.

* **sdks/android**: First version published to nalexander's personal bintray repository.
* Various bugfixes and refactorings (see commits below for details)
* [Commits](https://github.com/mozilla/mentat/compare/v0.8.1...v0.9.0)
