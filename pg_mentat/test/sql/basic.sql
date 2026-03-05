-- Basic EDN type tests

-- Test nil
SELECT mentat.edn_out(mentat.edn_in('nil'));

-- Test boolean
SELECT mentat.edn_out(mentat.edn_in('true'));
SELECT mentat.edn_out(mentat.edn_in('false'));

-- Test integer
SELECT mentat.edn_out(mentat.edn_in('42'));
SELECT mentat.edn_out(mentat.edn_in('-123'));

-- Test float
SELECT mentat.edn_out(mentat.edn_in('3.14'));
SELECT mentat.edn_out(mentat.edn_in('-2.5'));

-- Test string
SELECT mentat.edn_out(mentat.edn_in('"hello world"'));
SELECT mentat.edn_out(mentat.edn_in('"special chars: \n\t\\"'));

-- Test keyword
SELECT mentat.edn_out(mentat.edn_in(':name'));
SELECT mentat.edn_out(mentat.edn_in(':user/email'));

-- Test vector
SELECT mentat.edn_out(mentat.edn_in('[1 2 3 4 5]'));
SELECT mentat.edn_out(mentat.edn_in('[]'));

-- Test map
SELECT mentat.edn_out(mentat.edn_in('{:name "Alice" :age 30}'));
SELECT mentat.edn_out(mentat.edn_in('{}'));

-- Test set
SELECT mentat.edn_out(mentat.edn_in('#{1 2 3}'));
SELECT mentat.edn_out(mentat.edn_in('#{}'));

-- Test nested structures
SELECT mentat.edn_out(mentat.edn_in('{:users [{:name "Alice"} {:name "Bob"}]}'));

-- Test UUID
SELECT mentat.edn_out(mentat.edn_in('#uuid "550e8400-e29b-41d4-a716-446655440000"'));

-- Test instant
SELECT mentat.edn_out(mentat.edn_in('#inst "2025-03-05T12:00:00Z"'));

-- Test table creation with EDN column
CREATE TABLE edn_test (
    id SERIAL PRIMARY KEY,
    data mentat.EdnValue
);

-- Test insertion
INSERT INTO edn_test (data) VALUES
    (mentat.edn_in('nil')),
    (mentat.edn_in('true')),
    (mentat.edn_in('42')),
    (mentat.edn_in('"test string"')),
    (mentat.edn_in('[1 2 3]')),
    (mentat.edn_in('{:name "Alice" :age 30}'));

-- Test selection
SELECT id, mentat.edn_out(data) FROM edn_test ORDER BY id;

-- Test binary send/recv
SELECT mentat.edn_out(mentat.edn_recv(mentat.edn_send(mentat.edn_in('42'))));

-- Cleanup
DROP TABLE edn_test;
