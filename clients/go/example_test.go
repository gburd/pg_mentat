package pgmentat_test

import (
	"context"
	"fmt"
	"log"
	"os"

	pgmentat "example.com/pgmentat" // replace with actual module path
)

func Example() {
	ctx := context.Background()
	connString := os.Getenv("PG_MENTAT_DSN")
	if connString == "" {
		connString = "postgresql://localhost/postgres"
	}

	client, err := pgmentat.New(ctx, connString)
	if err != nil {
		log.Fatal(err)
	}
	defer client.Close()

	// Define schema
	_, err = client.Transact(ctx, `[
	  {:db/ident :person/name
	   :db/valueType :db.type/string
	   :db/cardinality :db.cardinality/one}
	  {:db/ident :person/email
	   :db/valueType :db.type/string
	   :db/cardinality :db.cardinality/one
	   :db/unique :db.unique/identity}
	]`)
	if err != nil {
		log.Fatal(err)
	}

	// Transact data
	_, err = client.Transact(ctx, `[
	  {:person/name "Alice" :person/email "alice@example.com"}
	  {:person/name "Bob"   :person/email "bob@example.com"}
	]`)
	if err != nil {
		log.Fatal(err)
	}

	// Query
	results, err := client.Query(ctx, `
	  [:find ?name ?email
	   :where
	   [?e :person/name ?name]
	   [?e :person/email ?email]]
	`, nil)
	if err != nil {
		log.Fatal(err)
	}

	fmt.Println("Results:", results)
}
