# Runbook: Security Incident

## Severity: P0

## Trigger

Any of the following:
- Unauthorized access to the database detected
- API key compromise suspected
- Unusual query patterns or data exfiltration indicators
- Vulnerability disclosure affecting pg_mentat or its dependencies

## Symptoms

- Unexpected queries in `pg_stat_activity` from unknown sources
- mentatd logs showing unauthorized access attempts
- Unusual spike in `mentatd_requests_total` from unexpected sources
- Data modifications not attributable to known operations

## Investigation Steps

### 1. Assess the Scope

```bash
# Check mentatd access logs for unauthorized requests
journalctl -u mentatd --since "1 hour ago" | grep -i "unauthorized\|401\|forbidden"

# Check for unusual request patterns
journalctl -u mentatd --since "1 hour ago" | grep -c "POST /"
```

```sql
-- Check PostgreSQL connections from unexpected sources
SELECT pid, usename, client_addr, application_name, state, query
FROM pg_stat_activity
WHERE datname = 'mentat'
ORDER BY backend_start DESC;

-- Check for recently created roles
SELECT rolname, rolcreatedb, rolsuper, rolcreaterole
FROM pg_roles
WHERE rolname NOT IN ('postgres', 'mentat', 'mentat_app');
```

### 2. Check for Data Tampering

```sql
-- Recent transactions (look for unexpected writes)
SELECT tx, tx_instant
FROM mentat.transactions
ORDER BY tx DESC
LIMIT 50;

-- Check for schema modifications
SELECT * FROM mentat.schema
ORDER BY entid DESC
LIMIT 20;

-- Look for unusual datom patterns (large batches from unexpected sources)
SELECT tx, count(*) AS datom_count
FROM mentat.datoms
GROUP BY tx
ORDER BY tx DESC
LIMIT 20;
```

## Immediate Response

### 1. Contain the Incident

```bash
# Rotate the API key immediately
export MENTATD_API_KEY="new-strong-random-key-$(openssl rand -hex 32)"
systemctl restart mentatd

# Block suspicious IP addresses at the firewall
iptables -A INPUT -s <suspicious-ip> -j DROP
```

```sql
-- Revoke access for compromised credentials
ALTER ROLE compromised_user NOLOGIN;

-- Change database passwords
ALTER ROLE mentat_app PASSWORD 'new-strong-password';
```

### 2. Preserve Evidence

```bash
# Save current logs
cp /var/log/mentatd.log /var/log/mentatd.log.incident-$(date +%Y%m%d)

# Save PostgreSQL logs
cp /var/log/postgresql/*.log /secure/evidence/

# Capture current connection state
psql -c "SELECT * FROM pg_stat_activity;" > /secure/evidence/connections.txt
psql -c "SELECT * FROM pg_stat_replication;" > /secure/evidence/replication.txt
```

### 3. Restrict Access

```sql
-- Restrict pg_hba.conf to known sources only
-- Edit pg_hba.conf:
-- hostssl mentat mentat_app 10.0.0.0/24 scram-sha-256
-- Reject all others

SELECT pg_reload_conf();
```

### 4. If Data Has Been Tampered

```bash
# Take a backup of the current (potentially compromised) state for forensics
pg_dump -Fc -f /secure/evidence/mentat_post_incident.dump --schema=mentat mentat

# Restore from the last known good backup
# See BACKUP.md for full restore procedures
```

## Post-Incident

1. **Incident report**: Document what happened, timeline, and impact
2. **Root cause analysis**: How was access gained?
3. **Credential rotation**: Rotate all secrets (API keys, database passwords, TLS certs)
4. **Access review**: Audit all database roles and permissions
5. **Security hardening**:
   - Enable `log_connections` and `log_disconnections` in PostgreSQL
   - Enable `log_statement = 'ddl'` to log schema changes
   - Review network policies and firewall rules
   - Enable audit logging if not already active
6. **Monitoring improvement**: Add alerts for the attack vector used

## Prevention

- Use strong, randomly generated API keys
- Rotate credentials regularly
- Use TLS for all connections (mentatd <-> clients, mentatd <-> PostgreSQL)
- Restrict network access with firewalls and Kubernetes NetworkPolicies
- Enable PostgreSQL audit logging (`pgaudit` extension)
- Monitor for unusual access patterns
- Keep all components updated with security patches
- Use the principle of least privilege for database roles
- Never store credentials in plaintext in configuration files; use secret management

## Escalation

- Security incidents are P0 and require immediate escalation to the security team
- Engage legal/compliance if data breach is confirmed
- Notify affected users per regulatory requirements
