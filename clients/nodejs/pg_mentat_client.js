/**
 * pg_mentat Node.js client -- Direct PostgreSQL access.
 *
 * No mentatd daemon required. Connect directly to PostgreSQL and call
 * the pg_mentat extension functions via standard SQL.
 *
 * Requirements:
 *     npm install pg
 *
 * Usage:
 *     const { MentatClient } = require('./pg_mentat_client');
 *
 *     const client = new MentatClient({ connectionString: 'postgresql://localhost/postgres' });
 *     await client.connect();
 *     await client.transact('[{:db/ident :person/name ...}]');
 *     const results = await client.query('[:find ?name :where [?e :person/name ?name]]');
 *     await client.close();
 */

const { Pool } = require('pg');

class MentatError extends Error {
  constructor(message) {
    super(message);
    this.name = 'MentatError';
  }
}

class MentatClient {
  /**
   * Create a client.
   *
   * @param {object} config - pg.Pool configuration, e.g. { connectionString: '...' }
   * @param {Pool} [pool] - Optional existing pg.Pool to reuse.
   */
  constructor(config, pool) {
    if (pool) {
      this._pool = pool;
      this._ownsPool = false;
    } else {
      this._pool = new Pool(config);
      this._ownsPool = true;
    }
  }

  /**
   * Close the connection pool (only if we created it).
   */
  async close() {
    if (this._ownsPool) {
      await this._pool.end();
    }
  }

  // -- Core API --------------------------------------------------------

  /**
   * Execute an EDN transaction.
   *
   * @param {string} ednTx - EDN transaction string.
   * @returns {object} Transaction report.
   */
  async transact(ednTx) {
    const { rows } = await this._pool.query(
      'SELECT mentat_transact($1)',
      [ednTx]
    );
    if (!rows.length) throw new MentatError('mentat_transact returned no result');
    const result = rows[0].mentat_transact;
    return typeof result === 'string' ? JSON.parse(result) : result;
  }

  /**
   * Execute a Datalog query.
   *
   * @param {string} datalog - Datalog query string.
   * @param {object} [inputs={}] - Optional query inputs.
   * @returns {*} Query results.
   */
  async query(datalog, inputs = {}) {
    const { rows } = await this._pool.query(
      'SELECT mentat_query($1, $2::jsonb)',
      [datalog, JSON.stringify(inputs)]
    );
    if (!rows.length) throw new MentatError('mentat_query returned no result');
    const result = rows[0].mentat_query;
    return typeof result === 'string' ? JSON.parse(result) : result;
  }

  /**
   * Pull attributes for an entity.
   *
   * @param {string} pattern - Pull pattern, e.g. '[:person/name :person/email]'
   * @param {number} entityId - Entity ID.
   * @returns {object} Entity attributes.
   */
  async pull(pattern, entityId) {
    const { rows } = await this._pool.query(
      'SELECT mentat_pull($1, $2)',
      [pattern, entityId]
    );
    if (!rows.length) throw new MentatError('mentat_pull returned no result');
    const result = rows[0].mentat_pull;
    return typeof result === 'string' ? JSON.parse(result) : result;
  }

  /**
   * Pull attributes for multiple entities.
   *
   * @param {string} pattern - Pull pattern.
   * @param {number[]} entityIds - Array of entity IDs.
   * @returns {object[]} Array of entity attribute objects.
   */
  async pullMany(pattern, entityIds) {
    const { rows } = await this._pool.query(
      'SELECT mentat_pull_many($1, $2)',
      [pattern, entityIds]
    );
    if (!rows.length) throw new MentatError('mentat_pull_many returned no result');
    const result = rows[0].mentat_pull_many;
    return typeof result === 'string' ? JSON.parse(result) : result;
  }

  /**
   * Get all attributes of an entity.
   *
   * @param {number} entityId - Entity ID.
   * @returns {object} Entity as object.
   */
  async entity(entityId) {
    const { rows } = await this._pool.query(
      'SELECT mentat_entity($1)',
      [entityId]
    );
    if (!rows.length) throw new MentatError('mentat_entity returned no result');
    const result = rows[0].mentat_entity;
    return typeof result === 'string' ? JSON.parse(result) : result;
  }

  /**
   * Return the current schema.
   *
   * @returns {object} Schema.
   */
  async schema() {
    const { rows } = await this._pool.query('SELECT mentat_schema()');
    if (!rows.length) throw new MentatError('mentat_schema returned no result');
    const result = rows[0].mentat_schema;
    return typeof result === 'string' ? JSON.parse(result) : result;
  }

  /**
   * Return the query execution plan.
   *
   * @param {string} datalog - Datalog query string.
   * @param {object} [inputs={}] - Optional query inputs.
   * @returns {*} Execution plan.
   */
  async explain(datalog, inputs = {}) {
    const { rows } = await this._pool.query(
      'SELECT mentat_explain($1, $2::jsonb)',
      [datalog, JSON.stringify(inputs)]
    );
    if (!rows.length) throw new MentatError('mentat_explain returned no result');
    const result = rows[0].mentat_explain;
    return typeof result === 'string' ? JSON.parse(result) : result;
  }

  // -- Native SQL view access ------------------------------------------

  /**
   * Query the facts view for human-readable EAVT data.
   *
   * @param {object} [opts={}] - Filter options.
   * @param {number} [opts.entityId] - Entity ID filter.
   * @param {string} [opts.attribute] - Attribute ident filter.
   * @param {string} [opts.store='default'] - Store name.
   * @returns {object[]} Array of fact objects.
   */
  async facts({ entityId, attribute, store = 'default' } = {}) {
    const schema = MentatClient._schemaForStore(store);
    let sql = `SELECT entity_id, attribute, value, value_type, tx, tx_time FROM ${schema}.facts`;
    const params = [];
    const wheres = [];
    if (entityId != null) { wheres.push(`entity_id = $${params.push(entityId)}`); }
    if (attribute != null) { wheres.push(`attribute = $${params.push(attribute)}`); }
    if (wheres.length) sql += ' WHERE ' + wheres.join(' AND ');
    sql += ' ORDER BY entity_id, attribute';
    const { rows } = await this._pool.query(sql, params);
    return rows;
  }

  /**
   * Query text_values view.
   *
   * @param {string} [attribute] - Optional attribute ident filter.
   * @param {string} [store='default'] - Store name.
   * @returns {object[]} Array of text value objects.
   */
  async textValues(attribute, store = 'default') {
    const schema = MentatClient._schemaForStore(store);
    let sql = `SELECT entity_id, attribute, value, tx FROM ${schema}.text_values`;
    const params = [];
    if (attribute != null) { sql += ` WHERE attribute = $${params.push(attribute)}`; }
    const { rows } = await this._pool.query(sql, params);
    return rows;
  }

  /**
   * Query numeric_values view.
   *
   * @param {string} [attribute] - Optional attribute ident filter.
   * @param {string} [store='default'] - Store name.
   * @returns {object[]} Array of numeric value objects.
   */
  async numericValues(attribute, store = 'default') {
    const schema = MentatClient._schemaForStore(store);
    let sql = `SELECT entity_id, attribute, value, tx FROM ${schema}.numeric_values`;
    const params = [];
    if (attribute != null) { sql += ` WHERE attribute = $${params.push(attribute)}`; }
    const { rows } = await this._pool.query(sql, params);
    return rows;
  }

  /**
   * Query entity_references view for relationship navigation.
   *
   * @param {object} [opts={}] - Filter options.
   * @param {number} [opts.source] - Source entity ID filter.
   * @param {number} [opts.target] - Target entity ID filter.
   * @param {string} [opts.store='default'] - Store name.
   * @returns {object[]} Array of reference objects.
   */
  async entityReferences({ source, target, store = 'default' } = {}) {
    const schema = MentatClient._schemaForStore(store);
    let sql = `SELECT source_entity, attribute, target_entity, target_ident, tx FROM ${schema}.entity_references`;
    const params = [];
    const wheres = [];
    if (source != null) { wheres.push(`source_entity = $${params.push(source)}`); }
    if (target != null) { wheres.push(`target_entity = $${params.push(target)}`); }
    if (wheres.length) sql += ' WHERE ' + wheres.join(' AND ');
    const { rows } = await this._pool.query(sql, params);
    return rows;
  }

  /**
   * Query entity_history view for temporal data.
   *
   * @param {number} [entityId] - Optional entity ID filter.
   * @param {string} [store='default'] - Store name.
   * @returns {object[]} Array of history objects.
   */
  async entityHistory(entityId, store = 'default') {
    const schema = MentatClient._schemaForStore(store);
    let sql = `SELECT entity_id, attribute, value, value_type, tx, tx_time, operation FROM ${schema}.entity_history`;
    const params = [];
    if (entityId != null) { sql += ` WHERE entity_id = $${params.push(entityId)}`; }
    sql += ' ORDER BY tx DESC';
    const { rows } = await this._pool.query(sql, params);
    return rows;
  }

  /**
   * Query tx_log view for transaction history.
   *
   * @param {number} [limit=100] - Maximum number of transactions.
   * @param {string} [store='default'] - Store name.
   * @returns {object[]} Array of transaction log objects.
   */
  async txLog(limit = 100, store = 'default') {
    const schema = MentatClient._schemaForStore(store);
    const { rows } = await this._pool.query(
      `SELECT tx, tx_time, datom_count FROM ${schema}.tx_log ORDER BY tx DESC LIMIT $1`,
      [limit]
    );
    return rows;
  }

  /**
   * Query schema_summary view.
   *
   * @param {string} [store='default'] - Store name.
   * @returns {object[]} Array of schema summary objects.
   */
  async schemaSummary(store = 'default') {
    const schema = MentatClient._schemaForStore(store);
    const { rows } = await this._pool.query(`SELECT * FROM ${schema}.schema_summary`);
    return rows;
  }

  /**
   * Find entities by attribute value using lookup_entity.
   *
   * @param {string} attribute - Attribute ident.
   * @param {string} value - Value to search for (as string).
   * @param {string} [store='default'] - Store name.
   * @returns {object[]} Array of { entity_id, tx }.
   */
  async lookupEntity(attribute, value, store = 'default') {
    const schema = MentatClient._schemaForStore(store);
    const { rows } = await this._pool.query(
      `SELECT entity_id, tx FROM ${schema}.lookup_entity($1, $2)`,
      [attribute, value]
    );
    return rows;
  }

  /**
   * Get a single attribute value for an entity.
   *
   * @param {number} entityId - Entity ID.
   * @param {string} attribute - Attribute ident.
   * @param {string} [store='default'] - Store name.
   * @returns {string|null} The value as string, or null.
   */
  async entityValue(entityId, attribute, store = 'default') {
    const schema = MentatClient._schemaForStore(store);
    const { rows } = await this._pool.query(
      `SELECT ${schema}.entity_value($1, $2)`,
      [entityId, attribute]
    );
    return rows.length ? rows[0].entity_value : null;
  }

  /**
   * Full-text search across all text values.
   *
   * @param {string} searchQuery - Search query string.
   * @param {string} [store='default'] - Store name.
   * @returns {object[]} Array of { entity_id, attribute, value, rank }.
   */
  async findText(searchQuery, store = 'default') {
    const schema = MentatClient._schemaForStore(store);
    const { rows } = await this._pool.query(
      `SELECT entity_id, attribute, value, rank FROM ${schema}.find_text($1)`,
      [searchQuery]
    );
    return rows;
  }

  // -- Helpers ---------------------------------------------------------

  /**
   * Derive the PostgreSQL schema name for a store.
   */
  static _schemaForStore(storeName) {
    if (storeName === 'default') return 'mentat';
    return `mentat_${storeName}`;
  }
}

module.exports = { MentatClient, MentatError };

// ---------------------------------------------------------------------------
// Example usage (run with: node pg_mentat_client.js)
// ---------------------------------------------------------------------------
if (require.main === module) {
  (async () => {
    const connectionString = process.argv[2] || 'postgresql://localhost/postgres';
    const client = new MentatClient({ connectionString });

    try {
      // Define schema
      await client.transact(`[
        {:db/ident :person/name
         :db/valueType :db.type/string
         :db/cardinality :db.cardinality/one}
        {:db/ident :person/email
         :db/valueType :db.type/string
         :db/cardinality :db.cardinality/one
         :db/unique :db.unique/identity}
      ]`);

      // Transact data
      await client.transact(`[
        {:person/name "Alice"
         :person/email "alice@example.com"}
        {:person/name "Bob"
         :person/email "bob@example.com"}
      ]`);

      // Query
      const results = await client.query(`
        [:find ?name ?email
         :where
         [?e :person/name ?name]
         [?e :person/email ?email]]
      `);
      console.log('Query results:', JSON.stringify(results, null, 2));

      // Schema
      const schema = await client.schema();
      console.log('Schema:', JSON.stringify(schema, null, 2));
    } finally {
      await client.close();
    }
  })();
}
