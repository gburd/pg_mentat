.PHONY: outdated fix upgrades install-upgrade-scripts

# PostgreSQL extension directory (auto-detected via pg_config)
PG_SHAREDIR := $(shell pg_config --sharedir 2>/dev/null || echo "/usr/share/postgresql")
EXTENSION_DIR := $(PG_SHAREDIR)/extension

outdated:
	for p in $(dirname $(ls Cargo.toml */Cargo.toml */*/Cargo.toml)); do echo $p; (cd $p; cargo outdated -R); done

fix:
	$(for p in $(dirname $(ls Cargo.toml */Cargo.toml */*/Cargo.toml)); do echo $p; (cd $p; cargo fix --allow-dirty --broken-code --edition-idioms); done)

upgrades:
	cargo upgrades

# Install upgrade SQL scripts to PostgreSQL extension directory.
# Run after `cargo pgrx install` to enable ALTER EXTENSION ... UPDATE.
install-upgrade-scripts:
	@echo "Installing upgrade scripts to $(EXTENSION_DIR)"
	@for f in pg_mentat/sql/upgrade--*.sql; do \
		target="$(EXTENSION_DIR)/pg_mentat--$$(echo $$f | sed 's|.*upgrade--||')"; \
		echo "  $$f -> $$target"; \
		install -m 644 "$$f" "$$target"; \
	done
	@echo "Done. Available upgrade paths:"
	@ls -1 $(EXTENSION_DIR)/pg_mentat--*--*.sql 2>/dev/null || echo "  (none installed)"
