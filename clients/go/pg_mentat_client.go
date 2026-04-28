// Package pgmentat provides a direct PostgreSQL client for pg_mentat.
//
// No mentatd daemon required. Connect directly to PostgreSQL and call
// the pg_mentat extension functions via standard SQL.
//
// Requirements:
//
//	go get github.com/jackc/pgx/v5
//
// Usage:
//
//	client, _ := pgmentat.New(ctx, "postgresql://localhost/postgres")
//	defer client.Close()
//	client.Transact(ctx, `[{:db/ident :person/name ...}]`)
//	results, _ := client.Query(ctx, `[:find ?name :where [?e :person/name ?name]]`, nil)
package pgmentat

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/jackc/pgx/v5/pgxpool"
)

// MentatError is returned when a pg_mentat SQL function call fails.
type MentatError struct {
	Message string
}

func (e *MentatError) Error() string {
	return fmt.Sprintf("pg_mentat: %s", e.Message)
}

// Client is a direct PostgreSQL client for pg_mentat.
type Client struct {
	pool     *pgxpool.Pool
	ownsPool bool
}

// New creates a client with a new connection pool.
func New(ctx context.Context, connString string) (*Client, error) {
	pool, err := pgxpool.New(ctx, connString)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: connect: %w", err)
	}
	return &Client{pool: pool, ownsPool: true}, nil
}

// NewFromPool creates a client that reuses an existing connection pool.
func NewFromPool(pool *pgxpool.Pool) *Client {
	return &Client{pool: pool, ownsPool: false}
}

// Close releases the connection pool (only if we created it).
func (c *Client) Close() {
	if c.ownsPool {
		c.pool.Close()
	}
}

// Transact executes an EDN transaction and returns the transaction report.
func (c *Client) Transact(ctx context.Context, ednTx string) (map[string]interface{}, error) {
	var result string
	err := c.pool.QueryRow(ctx, "SELECT mentat_transact($1)", ednTx).Scan(&result)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: transact: %w", err)
	}
	var report map[string]interface{}
	if err := json.Unmarshal([]byte(result), &report); err != nil {
		return nil, fmt.Errorf("pg_mentat: parse transact result: %w", err)
	}
	return report, nil
}

// Query executes a Datalog query. Pass nil for inputs if none are needed.
func (c *Client) Query(ctx context.Context, datalog string, inputs map[string]interface{}) (interface{}, error) {
	if inputs == nil {
		inputs = map[string]interface{}{}
	}
	inputsJSON, err := json.Marshal(inputs)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: marshal inputs: %w", err)
	}

	var resultJSON json.RawMessage
	err = c.pool.QueryRow(ctx,
		"SELECT mentat_query($1, $2::jsonb)",
		datalog, string(inputsJSON),
	).Scan(&resultJSON)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: query: %w", err)
	}

	var result interface{}
	if err := json.Unmarshal(resultJSON, &result); err != nil {
		return nil, fmt.Errorf("pg_mentat: parse query result: %w", err)
	}
	return result, nil
}

// Pull retrieves attributes for an entity using a pull pattern.
func (c *Client) Pull(ctx context.Context, pattern string, entityID int64) (map[string]interface{}, error) {
	var resultJSON json.RawMessage
	err := c.pool.QueryRow(ctx,
		"SELECT mentat_pull($1, $2)",
		pattern, entityID,
	).Scan(&resultJSON)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: pull: %w", err)
	}

	var result map[string]interface{}
	if err := json.Unmarshal(resultJSON, &result); err != nil {
		return nil, fmt.Errorf("pg_mentat: parse pull result: %w", err)
	}
	return result, nil
}

// PullMany retrieves attributes for multiple entities.
func (c *Client) PullMany(ctx context.Context, pattern string, entityIDs []int64) ([]map[string]interface{}, error) {
	var resultJSON json.RawMessage
	err := c.pool.QueryRow(ctx,
		"SELECT mentat_pull_many($1, $2)",
		pattern, entityIDs,
	).Scan(&resultJSON)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: pull_many: %w", err)
	}

	var result []map[string]interface{}
	if err := json.Unmarshal(resultJSON, &result); err != nil {
		return nil, fmt.Errorf("pg_mentat: parse pull_many result: %w", err)
	}
	return result, nil
}

// Entity returns all attributes of an entity.
func (c *Client) Entity(ctx context.Context, entityID int64) (map[string]interface{}, error) {
	var resultJSON json.RawMessage
	err := c.pool.QueryRow(ctx, "SELECT mentat_entity($1)", entityID).Scan(&resultJSON)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: entity: %w", err)
	}

	var result map[string]interface{}
	if err := json.Unmarshal(resultJSON, &result); err != nil {
		return nil, fmt.Errorf("pg_mentat: parse entity result: %w", err)
	}
	return result, nil
}

// Schema returns the current schema.
func (c *Client) Schema(ctx context.Context) (map[string]interface{}, error) {
	var resultJSON json.RawMessage
	err := c.pool.QueryRow(ctx, "SELECT mentat_schema()").Scan(&resultJSON)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: schema: %w", err)
	}

	var result map[string]interface{}
	if err := json.Unmarshal(resultJSON, &result); err != nil {
		return nil, fmt.Errorf("pg_mentat: parse schema result: %w", err)
	}
	return result, nil
}

// Explain returns the execution plan for a Datalog query.
func (c *Client) Explain(ctx context.Context, datalog string, inputs map[string]interface{}) (interface{}, error) {
	if inputs == nil {
		inputs = map[string]interface{}{}
	}
	inputsJSON, err := json.Marshal(inputs)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: marshal inputs: %w", err)
	}

	var resultJSON json.RawMessage
	err = c.pool.QueryRow(ctx,
		"SELECT mentat_explain($1, $2::jsonb)",
		datalog, string(inputsJSON),
	).Scan(&resultJSON)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: explain: %w", err)
	}

	var result interface{}
	if err := json.Unmarshal(resultJSON, &result); err != nil {
		return nil, fmt.Errorf("pg_mentat: parse explain result: %w", err)
	}
	return result, nil
}

// -- Native SQL view access ------------------------------------------

// schemaForStore derives the PostgreSQL schema name for a store.
func schemaForStore(store string) string {
	if store == "default" || store == "" {
		return "mentat"
	}
	return "mentat_" + store
}

// Fact represents a row from the facts view.
type Fact struct {
	EntityID  int64   `json:"entity_id"`
	Attribute string  `json:"attribute"`
	Value     string  `json:"value"`
	ValueType string  `json:"value_type"`
	Tx        int64   `json:"tx"`
	TxTime    *string `json:"tx_time,omitempty"`
}

// Facts queries the facts view with optional filters.
func (c *Client) Facts(ctx context.Context, store string, entityID *int64, attribute *string) ([]Fact, error) {
	schema := schemaForStore(store)
	sql := fmt.Sprintf("SELECT entity_id, attribute, value, value_type, tx, tx_time::TEXT FROM %s.facts", schema)
	args := []interface{}{}
	wheres := []string{}
	if entityID != nil {
		args = append(args, *entityID)
		wheres = append(wheres, fmt.Sprintf("entity_id = $%d", len(args)))
	}
	if attribute != nil {
		args = append(args, *attribute)
		wheres = append(wheres, fmt.Sprintf("attribute = $%d", len(args)))
	}
	if len(wheres) > 0 {
		sql += " WHERE " + wheres[0]
		for _, w := range wheres[1:] {
			sql += " AND " + w
		}
	}
	sql += " ORDER BY entity_id, attribute"

	rows, err := c.pool.Query(ctx, sql, args...)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: facts: %w", err)
	}
	defer rows.Close()

	var facts []Fact
	for rows.Next() {
		var f Fact
		if err := rows.Scan(&f.EntityID, &f.Attribute, &f.Value, &f.ValueType, &f.Tx, &f.TxTime); err != nil {
			return nil, fmt.Errorf("pg_mentat: scan fact: %w", err)
		}
		facts = append(facts, f)
	}
	return facts, rows.Err()
}

// Reference represents a row from the entity_references view.
type Reference struct {
	SourceEntity int64   `json:"source_entity"`
	Attribute    string  `json:"attribute"`
	TargetEntity int64   `json:"target_entity"`
	TargetIdent  *string `json:"target_ident,omitempty"`
	Tx           int64   `json:"tx"`
}

// EntityReferences queries the entity_references view with optional filters.
func (c *Client) EntityReferences(ctx context.Context, store string, source *int64, target *int64) ([]Reference, error) {
	schema := schemaForStore(store)
	sql := fmt.Sprintf("SELECT source_entity, attribute, target_entity, target_ident, tx FROM %s.entity_references", schema)
	args := []interface{}{}
	wheres := []string{}
	if source != nil {
		args = append(args, *source)
		wheres = append(wheres, fmt.Sprintf("source_entity = $%d", len(args)))
	}
	if target != nil {
		args = append(args, *target)
		wheres = append(wheres, fmt.Sprintf("target_entity = $%d", len(args)))
	}
	if len(wheres) > 0 {
		sql += " WHERE " + wheres[0]
		for _, w := range wheres[1:] {
			sql += " AND " + w
		}
	}

	rows, err := c.pool.Query(ctx, sql, args...)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: entity_references: %w", err)
	}
	defer rows.Close()

	var refs []Reference
	for rows.Next() {
		var r Reference
		if err := rows.Scan(&r.SourceEntity, &r.Attribute, &r.TargetEntity, &r.TargetIdent, &r.Tx); err != nil {
			return nil, fmt.Errorf("pg_mentat: scan reference: %w", err)
		}
		refs = append(refs, r)
	}
	return refs, rows.Err()
}

// HistoryEntry represents a row from the entity_history view.
type HistoryEntry struct {
	EntityID  int64   `json:"entity_id"`
	Attribute string  `json:"attribute"`
	Value     string  `json:"value"`
	ValueType string  `json:"value_type"`
	Tx        int64   `json:"tx"`
	TxTime    *string `json:"tx_time,omitempty"`
	Operation string  `json:"operation"`
}

// EntityHistory queries the entity_history view.
func (c *Client) EntityHistory(ctx context.Context, store string, entityID *int64) ([]HistoryEntry, error) {
	schema := schemaForStore(store)
	sql := fmt.Sprintf("SELECT entity_id, attribute, value, value_type, tx, tx_time::TEXT, operation FROM %s.entity_history", schema)
	args := []interface{}{}
	if entityID != nil {
		args = append(args, *entityID)
		sql += fmt.Sprintf(" WHERE entity_id = $%d", len(args))
	}
	sql += " ORDER BY tx DESC"

	rows, err := c.pool.Query(ctx, sql, args...)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: entity_history: %w", err)
	}
	defer rows.Close()

	var entries []HistoryEntry
	for rows.Next() {
		var e HistoryEntry
		if err := rows.Scan(&e.EntityID, &e.Attribute, &e.Value, &e.ValueType, &e.Tx, &e.TxTime, &e.Operation); err != nil {
			return nil, fmt.Errorf("pg_mentat: scan history: %w", err)
		}
		entries = append(entries, e)
	}
	return entries, rows.Err()
}

// TxLogEntry represents a row from the tx_log view.
type TxLogEntry struct {
	Tx         int64   `json:"tx"`
	TxTime     *string `json:"tx_time,omitempty"`
	DatomCount int64   `json:"datom_count"`
}

// TxLog queries the tx_log view.
func (c *Client) TxLog(ctx context.Context, store string, limit int) ([]TxLogEntry, error) {
	if limit <= 0 {
		limit = 100
	}
	schema := schemaForStore(store)
	sql := fmt.Sprintf("SELECT tx, tx_time::TEXT, datom_count FROM %s.tx_log ORDER BY tx DESC LIMIT $1", schema)

	rows, err := c.pool.Query(ctx, sql, limit)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: tx_log: %w", err)
	}
	defer rows.Close()

	var entries []TxLogEntry
	for rows.Next() {
		var e TxLogEntry
		if err := rows.Scan(&e.Tx, &e.TxTime, &e.DatomCount); err != nil {
			return nil, fmt.Errorf("pg_mentat: scan tx_log: %w", err)
		}
		entries = append(entries, e)
	}
	return entries, rows.Err()
}

// LookupEntity finds entities by attribute value.
func (c *Client) LookupEntity(ctx context.Context, store string, attribute string, value string) ([]int64, error) {
	schema := schemaForStore(store)
	sql := fmt.Sprintf("SELECT entity_id FROM %s.lookup_entity($1, $2)", schema)

	rows, err := c.pool.Query(ctx, sql, attribute, value)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: lookup_entity: %w", err)
	}
	defer rows.Close()

	var ids []int64
	for rows.Next() {
		var id int64
		if err := rows.Scan(&id); err != nil {
			return nil, fmt.Errorf("pg_mentat: scan lookup: %w", err)
		}
		ids = append(ids, id)
	}
	return ids, rows.Err()
}

// EntityValue gets a single attribute value for an entity.
func (c *Client) EntityValue(ctx context.Context, store string, entityID int64, attribute string) (*string, error) {
	schema := schemaForStore(store)
	sql := fmt.Sprintf("SELECT %s.entity_value($1, $2)", schema)

	var result *string
	err := c.pool.QueryRow(ctx, sql, entityID, attribute).Scan(&result)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: entity_value: %w", err)
	}
	return result, nil
}

// TextSearchResult represents a full-text search match.
type TextSearchResult struct {
	EntityID  int64   `json:"entity_id"`
	Attribute string  `json:"attribute"`
	Value     string  `json:"value"`
	Rank      float32 `json:"rank"`
}

// FindText performs full-text search across all text values.
func (c *Client) FindText(ctx context.Context, store string, searchQuery string) ([]TextSearchResult, error) {
	schema := schemaForStore(store)
	sql := fmt.Sprintf("SELECT entity_id, attribute, value, rank FROM %s.find_text($1)", schema)

	rows, err := c.pool.Query(ctx, sql, searchQuery)
	if err != nil {
		return nil, fmt.Errorf("pg_mentat: find_text: %w", err)
	}
	defer rows.Close()

	var results []TextSearchResult
	for rows.Next() {
		var r TextSearchResult
		if err := rows.Scan(&r.EntityID, &r.Attribute, &r.Value, &r.Rank); err != nil {
			return nil, fmt.Errorf("pg_mentat: scan text search: %w", err)
		}
		results = append(results, r)
	}
	return results, rows.Err()
}
