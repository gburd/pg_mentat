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
