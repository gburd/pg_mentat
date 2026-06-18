use crate::error::MentatError;
use crate::functions::store_management::{get_schema_for_store, quote_ident, validate_store_name};
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::json;

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Maximum length for a subscription name.
const MAX_SUBSCRIPTION_NAME_LEN: usize = 58;

/// Validate a subscription name.
///
/// Rules:
/// - Must not be empty
/// - Must be <= 58 characters (channel name = "mentat_" prefix + name, max 63)
/// - Must start with a letter or underscore
/// - Must contain only letters, digits, and underscores
fn validate_subscription_name(name: &str) -> Result<(), MentatError> {
    if name.is_empty() {
        return Err(MentatError::InvalidQuery {
            message: "Subscription name cannot be empty.".to_string(),
            suggestion: None,
        });
    }

    if name.len() > MAX_SUBSCRIPTION_NAME_LEN {
        return Err(MentatError::InvalidQuery {
            message: format!(
                "Subscription name exceeds maximum length of {} characters.",
                MAX_SUBSCRIPTION_NAME_LEN
            ),
            suggestion: None,
        });
    }

    let first = name
        .chars()
        .next()
        .ok_or_else(|| MentatError::InvalidQuery {
            message: "Subscription name cannot be empty.".to_string(),
            suggestion: None,
        })?;

    if !first.is_ascii_alphabetic() && first != '_' {
        return Err(MentatError::InvalidQuery {
            message: "Subscription name must start with a letter or underscore.".to_string(),
            suggestion: None,
        });
    }

    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(MentatError::InvalidQuery {
            message: "Subscription name must contain only letters, digits, and underscores."
                .to_string(),
            suggestion: None,
        });
    }

    Ok(())
}

/// Derive the LISTEN/NOTIFY channel name for a subscription.
fn channel_name(subscription_name: &str) -> String {
    format!("mentat_{}", subscription_name)
}

// ---------------------------------------------------------------------------
// Helper: check if subscription exists
// ---------------------------------------------------------------------------

fn subscription_exists(
    store_name: &str,
    subscription_name: &str,
) -> Result<bool, pgrx::spi::SpiError> {
    Spi::connect(|client| {
        let exists = client
            .select(
                "SELECT 1 FROM mentat.subscriptions WHERE store_name = $1 AND name = $2",
                None,
                &[
                    DatumWithOid::from(store_name),
                    DatumWithOid::from(subscription_name),
                ],
            )?
            .next()
            .is_some();
        Ok(exists)
    })
}

// ---------------------------------------------------------------------------
// Trigger function SQL generators
// ---------------------------------------------------------------------------

/// Generate the SQL for the trigger function that re-evaluates a Datalog query
/// on datom changes and issues NOTIFY if results differ.
///
/// The trigger function:
/// 1. Runs the subscribed Datalog query via mentat_query
/// 2. Computes an MD5 hash of the result
/// 3. Compares with the last known hash (stored in a session variable)
/// 4. Sends NOTIFY on the subscription channel if the hash changed
fn trigger_function_sql(
    schema: &str,
    subscription_name: &str,
    channel: &str,
    query: &str,
) -> String {
    // Escape single quotes in the query for embedding in PL/pgSQL
    let escaped_query = query.replace('\'', "''");
    let func_name = format!(
        "{}.mentat_sub_{}",
        schema,
        subscription_name
    );

    // The session variable key stores the last known hash for this subscription.
    // We use a custom GUC-like approach with current_setting / set_config.
    let var_name = format!("mentat.sub_hash_{}", subscription_name);

    format!(
        r"
CREATE OR REPLACE FUNCTION {func_name}() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    new_hash TEXT;
    old_hash TEXT;
    result_json JSONB;
BEGIN
    -- Re-evaluate the subscribed Datalog query
    SELECT mentat_query('{escaped_query}') INTO result_json;

    -- Compute hash of current results
    new_hash := md5(result_json::TEXT);

    -- Get last known hash from session state (returns empty string if unset)
    BEGIN
        old_hash := current_setting('{var_name}', true);
    EXCEPTION WHEN OTHERS THEN
        old_hash := '';
    END;

    -- If results changed, notify and update the stored hash
    IF old_hash IS DISTINCT FROM new_hash THEN
        PERFORM set_config('{var_name}', new_hash, false);
        PERFORM pg_notify('{channel}', result_json::TEXT);
    END IF;

    RETURN NULL;
END;
$$;
",
        func_name = func_name,
        escaped_query = escaped_query,
        var_name = var_name,
        channel = channel,
    )
}

/// Generate the SQL for the trigger that fires the subscription function.
fn trigger_sql(schema: &str, subscription_name: &str) -> String {
    let trigger_name = format!("mentat_sub_trg_{}", subscription_name);
    let func_name = format!(
        "{}.mentat_sub_{}",
        schema,
        subscription_name
    );

    format!(
        r"
CREATE OR REPLACE TRIGGER {trigger_name}
    AFTER INSERT OR UPDATE OR DELETE ON {schema}.datoms
    FOR EACH STATEMENT
    EXECUTE FUNCTION {func_name}();
",
        trigger_name = trigger_name,
        schema = schema,
        func_name = func_name,
    )
}

/// Generate SQL to drop the trigger and function for a subscription.
fn drop_trigger_sql(schema: &str, subscription_name: &str) -> String {
    let trigger_name = format!("mentat_sub_trg_{}", subscription_name);
    let func_name = format!(
        "{}.mentat_sub_{}",
        schema,
        subscription_name
    );

    format!(
        r"
DROP TRIGGER IF EXISTS {trigger_name} ON {schema}.datoms;
DROP FUNCTION IF EXISTS {func_name}();
",
        trigger_name = trigger_name,
        schema = schema,
        func_name = func_name,
    )
}

// ---------------------------------------------------------------------------
// Public API functions
// ---------------------------------------------------------------------------

/// Create a subscription that monitors Datalog query results for changes.
///
/// When data in the store's datoms table changes (INSERT, UPDATE, DELETE),
/// the subscribed query is re-evaluated. If the results differ from the
/// previous evaluation, a NOTIFY is sent on channel `mentat_<subscription_name>`
/// with the new query results as the payload.
///
/// Clients can listen for changes using:
/// ```sql
/// LISTEN mentat_my_subscription;
/// ```
///
/// # Arguments
/// - `store_name`: Name of the store to monitor (use "default" for the default store)
/// - `subscription_name`: Unique name for this subscription
/// - `query`: Datalog query string to evaluate on changes
///
/// # Example
/// ```sql
/// SELECT mentat_subscribe('default', 'all_people',
///     '[:find ?name :where [?e :person/name ?name]]');
/// LISTEN mentat_all_people;
/// ```
#[pg_extern]
pub fn subscribe(
    store_name: &str,
    subscription_name: &str,
    query: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    validate_store_name(store_name)?;
    validate_subscription_name(subscription_name)?;

    if query.trim().is_empty() {
        return Err(Box::new(MentatError::InvalidQuery {
            message: "Subscription query cannot be empty.".to_string(),
            suggestion: None,
        }));
    }

    // Check if subscription already exists
    if subscription_exists(store_name, subscription_name)? {
        return Err(Box::new(MentatError::InvalidQuery {
            message: format!(
                "Subscription '{}' already exists for store '{}'.",
                subscription_name, store_name
            ),
            suggestion: Some(format!(
                "Use mentat_unsubscribe('{}', '{}') first to remove the existing subscription.",
                store_name, subscription_name
            )),
        }));
    }

    let schema = get_schema_for_store(store_name);
    let quoted_schema = quote_ident(&schema);
    let channel = channel_name(subscription_name);

    // Create the trigger function
    let func_sql = trigger_function_sql(&quoted_schema, subscription_name, &channel, query);
    Spi::run(&func_sql)?;

    // Create the trigger on the datoms table.
    // The datoms table may be partitioned; for the default store the trigger
    // goes on mentat.datoms (the parent). PostgreSQL propagates statement-level
    // triggers to partitions automatically when using AFTER ... FOR EACH STATEMENT.
    let trg_sql = trigger_sql(&quoted_schema, subscription_name);
    Spi::run(&trg_sql)?;

    // Register in metadata table
    Spi::run_with_args(
        "INSERT INTO mentat.subscriptions (store_name, name, query) VALUES ($1, $2, $3)",
        &[
            DatumWithOid::from(store_name),
            DatumWithOid::from(subscription_name),
            DatumWithOid::from(query),
        ],
    )?;

    Ok(format!(
        "Subscription '{}' created. LISTEN {} to receive notifications.",
        subscription_name, channel
    ))
}

/// Remove a subscription, dropping its trigger and function.
///
/// After unsubscribing, no further NOTIFY messages will be sent for this
/// subscription. Existing LISTEN connections on the channel are not affected
/// but will stop receiving new messages.
///
/// # Arguments
/// - `store_name`: Name of the store the subscription belongs to
/// - `subscription_name`: Name of the subscription to remove
///
/// # Example
/// ```sql
/// SELECT mentat_unsubscribe('default', 'all_people');
/// ```
#[pg_extern]
pub fn unsubscribe(
    store_name: &str,
    subscription_name: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    validate_store_name(store_name)?;
    validate_subscription_name(subscription_name)?;

    // Verify subscription exists
    if !subscription_exists(store_name, subscription_name)? {
        return Err(Box::new(MentatError::InvalidQuery {
            message: format!(
                "Subscription '{}' not found for store '{}'.",
                subscription_name, store_name
            ),
            suggestion: None,
        }));
    }

    let schema = get_schema_for_store(store_name);
    let quoted_schema = quote_ident(&schema);

    // Drop trigger and function
    let drop_sql = drop_trigger_sql(&quoted_schema, subscription_name);
    Spi::run(&drop_sql)?;

    // Remove from metadata table
    Spi::run_with_args(
        "DELETE FROM mentat.subscriptions WHERE store_name = $1 AND name = $2",
        &[
            DatumWithOid::from(store_name),
            DatumWithOid::from(subscription_name),
        ],
    )?;

    Ok(format!(
        "Subscription '{}' removed from store '{}'.",
        subscription_name, store_name
    ))
}

/// List all active subscriptions, optionally filtered by store.
///
/// Returns a JSON array of subscription objects with store name, subscription
/// name, query, and creation time.
///
/// # Example
/// ```sql
/// SELECT mentat_list_subscriptions();
/// SELECT mentat_list_subscriptions('default');
/// ```
#[pg_extern]
pub fn list_subscriptions(
    store_name: default!(Option<&str>, "NULL"),
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let subscriptions = Spi::connect(|client| {
        let mut result = Vec::new();

        let rows = match store_name {
            Some(name) => client.select(
                "SELECT store_name, name, query, created_at::TEXT \
                 FROM mentat.subscriptions WHERE store_name = $1 ORDER BY created_at",
                None,
                &[DatumWithOid::from(name)],
            )?,
            None => client.select(
                "SELECT store_name, name, query, created_at::TEXT \
                 FROM mentat.subscriptions ORDER BY created_at",
                None,
                &[],
            )?,
        };

        for row in rows {
            let sname: String = row.get::<String>(1)?.unwrap_or_default();
            let sub_name: String = row.get::<String>(2)?.unwrap_or_default();
            let query_str: String = row.get::<String>(3)?.unwrap_or_default();
            let created: String = row.get::<String>(4)?.unwrap_or_default();

            result.push(json!({
                "store_name": sname,
                "name": sub_name,
                "query": query_str,
                "channel": channel_name(&sub_name),
                "created_at": created,
            }));
        }

        Ok::<_, pgrx::spi::SpiError>(result)
    })?;

    Ok(JsonB(json!(subscriptions)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_subscription_name_valid() {
        assert!(validate_subscription_name("my_sub").is_ok());
        assert!(validate_subscription_name("sub1").is_ok());
        assert!(validate_subscription_name("_private").is_ok());
        assert!(validate_subscription_name("a").is_ok());
    }

    #[test]
    fn test_validate_subscription_name_empty() {
        assert!(validate_subscription_name("").is_err());
    }

    #[test]
    fn test_validate_subscription_name_too_long() {
        let long_name = "a".repeat(59);
        assert!(validate_subscription_name(&long_name).is_err());
    }

    #[test]
    fn test_validate_subscription_name_starts_with_digit() {
        assert!(validate_subscription_name("1sub").is_err());
    }

    #[test]
    fn test_validate_subscription_name_invalid_chars() {
        assert!(validate_subscription_name("my-sub").is_err());
        assert!(validate_subscription_name("my sub").is_err());
        assert!(validate_subscription_name("sub!").is_err());
    }

    #[test]
    fn test_channel_name() {
        assert_eq!(channel_name("all_people"), "mentat_all_people");
        assert_eq!(channel_name("my_sub"), "mentat_my_sub");
    }

    #[test]
    fn test_trigger_function_sql_contains_key_elements() {
        let sql = trigger_function_sql(
            "mentat",
            "test_sub",
            "mentat_test_sub",
            "[:find ?e :where [?e :person/name]]",
        );
        assert!(sql.contains("mentat.mentat_sub_test_sub"));
        assert!(sql.contains("mentat_query"));
        assert!(sql.contains("pg_notify"));
        assert!(sql.contains("mentat_test_sub"));
        assert!(sql.contains("[:find ?e :where [?e :person/name]]"));
    }

    #[test]
    fn test_trigger_function_sql_escapes_quotes() {
        let sql = trigger_function_sql(
            "mentat",
            "test_sub",
            "mentat_test_sub",
            "[:find ?name :where [?e :person/name ?name] [(= ?name \"O'Brien\")]]",
        );
        // Single quotes are doubled once for the mentat_query('...') string
        // literal embedded inside the $$-quoted function body. The function
        // body is dollar-quoted, so the quotes inside it are literal -- there
        // is no second nesting level, hence one level of doubling (O''Brien),
        // not two (O''''Brien).
        assert!(sql.contains("O''Brien"));
    }

    #[test]
    fn test_trigger_sql_format() {
        let sql = trigger_sql("mentat", "test_sub");
        assert!(sql.contains("mentat_sub_trg_test_sub"));
        assert!(sql.contains("AFTER INSERT OR UPDATE OR DELETE"));
        assert!(sql.contains("FOR EACH STATEMENT"));
        assert!(sql.contains("mentat.datoms"));
    }

    #[test]
    fn test_drop_trigger_sql_format() {
        let sql = drop_trigger_sql("mentat", "test_sub");
        assert!(sql.contains("DROP TRIGGER IF EXISTS mentat_sub_trg_test_sub"));
        assert!(sql.contains("DROP FUNCTION IF EXISTS mentat.mentat_sub_test_sub"));
    }
}
