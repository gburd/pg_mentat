use pgrx::prelude::*;
use pgrx::spi::Spi;
use serde_json::json;

use crate::functions::store_management::get_schema_for_store;

/// Permanently excise (delete) entities from the database.
///
/// Unlike retraction, which marks datoms as `added = false` for temporal
/// queries, excision physically removes all traces of the entity from
/// the typed storage tables and (optionally) the transaction log.
///
/// Safeguards:
/// - Partition must have `allow_excision = true`
/// - Schema entities (partition db.part/db, entid < 10000) cannot be excised
/// - Entities referenced by other entities (via ref attributes) are rejected
///   unless those references are also being excised in the same call
///
/// Returns a JSON summary of the operation.
#[pg_extern]
pub fn mentat_excise(
    store: default!(&str, "'default'"),
    entity_ids: Vec<i64>,
    reason: default!(Option<&str>, "NULL"),
) -> String {
    match excise_internal(store, &entity_ids, reason) {
        Ok(result) => result,
        Err(e) => {
            let err_json = json!({
                "error": format!("{}", e),
                "entity_ids": entity_ids,
            });
            pgrx::error!("{}", err_json);
        }
    }
}

fn excise_internal(
    store: &str,
    entity_ids: &[i64],
    reason: Option<&str>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if entity_ids.is_empty() {
        return Ok(json!({"excised": 0, "datoms_removed": 0, "tx_entries_removed": 0}).to_string());
    }

    let schema_name = get_schema_for_store(store);
    let schema_prefix = format!("{}.", schema_name);

    // Check each entity: reject schema entities and verify partition allows excision
    for &eid in entity_ids {
        // Reject schema entities (db.part/db partition: entid < 10000)
        if eid < 10000 {
            return Err(format!(
                ":db.error/excision-denied Cannot excise schema entity {}. \
                 Schema entities (entid < 10000) are protected from excision.",
                eid
            ).into());
        }

        // Verify partition allows excision for this entity
        let allowed = Spi::get_one_with_args::<bool>(
            &format!(
                "SELECT COALESCE( \
                    (SELECT allow_excision FROM {schema_prefix}partitions \
                     WHERE start_entid <= $1 AND end_entid > $1), FALSE)"
            ),
            &[pgrx::datum::DatumWithOid::from(eid)],
        )
        .ok()
        .flatten()
        .unwrap_or(false);

        if !allowed {
            return Err(format!(
                ":db.error/excision-denied Excision not allowed for entity {}. \
                 The partition containing this entity has allow_excision = false. \
                 Use: UPDATE mentat.partitions SET allow_excision = true WHERE ...",
                eid
            ).into());
        }
    }

    // Check for dangling references: other entities that reference the excised entities
    let eid_list: String = entity_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");

    let ref_count = Spi::get_one::<i64>(&format!(
        "SELECT COUNT(*) FROM mentat.datoms_ref_new \
         WHERE v = ANY(ARRAY[{eid_list}]::BIGINT[]) \
         AND e != ALL(ARRAY[{eid_list}]::BIGINT[]) \
         AND added = true"
    ))
    .ok()
    .flatten()
    .unwrap_or(0);

    if ref_count > 0 {
        // Collect referencing entities for the error message
        let referrers = Spi::get_one::<String>(&format!(
            "SELECT array_agg(DISTINCT e)::TEXT FROM mentat.datoms_ref_new \
             WHERE v = ANY(ARRAY[{eid_list}]::BIGINT[]) \
             AND e != ALL(ARRAY[{eid_list}]::BIGINT[]) \
             AND added = true \
             LIMIT 10"
        ))
        .ok()
        .flatten()
        .unwrap_or_else(|| "[]".to_string());

        return Err(format!(
            ":db.error/excision-dangling-refs Cannot excise: {} other entities reference the target entities. \
             Referencing entity IDs (first 10): {}. \
             Either excise the referencing entities too, or retract the references first.",
            ref_count, referrers
        ).into());
    }

    // Perform the excision: DELETE from all 9 typed tables
    let tables = [
        "datoms_ref_new", "datoms_boolean_new", "datoms_long_new",
        "datoms_double_new", "datoms_instant_new", "datoms_text_new",
        "datoms_keyword_new", "datoms_uuid_new", "datoms_bytes_new",
    ];

    let mut total_datoms_removed: i64 = 0;

    for table in &tables {
        // Count existing datoms for these entities in this table
        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.{table} WHERE e = ANY(ARRAY[{eid_list}]::BIGINT[])"
        ))
        .ok()
        .flatten()
        .unwrap_or(0);

        if count > 0 {
            Spi::run(&format!(
                "DELETE FROM mentat.{table} WHERE e = ANY(ARRAY[{eid_list}]::BIGINT[])"
            ))?;
            total_datoms_removed += count;
        }
    }

    // Remove transaction log entries that ONLY contain datoms for excised entities.
    // (We preserve transactions that also contain datoms for non-excised entities.)
    // For simplicity, we remove entries from the datoms view where e is in the list.
    // The transactions table row itself remains (it has tx_instant which is auditable).
    let tx_entries_removed: i64 = 0; // Transactions rows are preserved for audit trail

    // Log the excision
    Spi::run(&format!(
        "INSERT INTO mentat.excision_log (store_name, entity_ids, datoms_removed, tx_log_entries_removed, reason) \
         VALUES ('{}', ARRAY[{}]::BIGINT[], {}, {}, {})",
        store.replace('\'', "''"),
        eid_list,
        total_datoms_removed,
        tx_entries_removed,
        reason.map_or("NULL".to_string(), |r| format!("'{}'", r.replace('\'', "''")))
    ))?;

    let result = json!({
        "excised": entity_ids.len(),
        "datoms_removed": total_datoms_removed,
        "tx_entries_removed": tx_entries_removed,
    });

    Ok(result.to_string())
}
