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
