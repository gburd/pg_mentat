# Kubernetes Deployment Guide

This guide covers deploying pg_mentat on Kubernetes using either raw manifests or the Helm chart.

## Architecture

The Kubernetes deployment consists of two components:

1. **mentatd** -- A stateless HTTP server (Deployment) that provides the Datomic-compatible API on port 8080. Scales horizontally behind a Service.
2. **PostgreSQL with pg_mentat** -- A StatefulSet running PostgreSQL 16 with the pg_mentat extension installed via init container. Uses persistent volume claims for data durability.

```
                    +-----------+
   Ingress ------> |  Service  |
                    | (mentatd) |
                    +-----+-----+
                          |
              +-----------+-----------+
              |           |           |
         +----+----+ +----+----+ +----+----+
         | mentatd | | mentatd | | mentatd |
         | pod (1) | | pod (2) | | pod (N) |
         +----+----+ +----+----+ +----+----+
              |           |           |
              +-----------+-----------+
                          |
                    +-----+------+
                    | PostgreSQL |
                    | StatefulSet|
                    |  (pg_mentat)|
                    +-----+------+
                          |
                    +-----+------+
                    |    PVC     |
                    | (10Gi)    |
                    +------------+
```

## Prerequisites

- Kubernetes cluster (1.25+)
- `kubectl` configured to access the cluster
- Container images built and available:
  - `pg-mentat/mentatd:latest` (the HTTP server)
  - `pg-mentat/pg-mentat:latest` (PostgreSQL with extension, from the root Dockerfile)
- For Helm deployment: Helm 3.x

## Quick Start with Raw Manifests

Apply all manifests in order:

```bash
# Create namespace
kubectl apply -f k8s/namespace.yaml

# Create secrets (edit values first for production!)
kubectl apply -f k8s/secret.yaml

# Deploy PostgreSQL
kubectl apply -f k8s/configmap.yaml
kubectl apply -f k8s/statefulset.yaml
kubectl apply -f k8s/service.yaml

# Wait for PostgreSQL to be ready
kubectl -n pg-mentat wait --for=condition=ready pod/pg-mentat-postgres-0 --timeout=120s

# Deploy mentatd
kubectl apply -f k8s/deployment.yaml
kubectl apply -f k8s/hpa.yaml
kubectl apply -f k8s/pdb.yaml

# Optional: ingress and network policies
kubectl apply -f k8s/ingress.yaml
kubectl apply -f k8s/networkpolicy.yaml
```

Verify the deployment:

```bash
kubectl -n pg-mentat get pods
kubectl -n pg-mentat get svc
kubectl -n pg-mentat logs deployment/mentatd
```

## Helm Chart Deployment

### Install

```bash
helm install pg-mentat ./helm/pg-mentat \
  --namespace pg-mentat \
  --create-namespace
```

### Install with custom values

```bash
helm install pg-mentat ./helm/pg-mentat \
  --namespace pg-mentat \
  --create-namespace \
  --set mentatd.replicaCount=3 \
  --set postgresql.auth.password=secure-password \
  --set postgresql.persistence.size=50Gi
```

### Using a values file

Create a `values-production.yaml`:

```yaml
mentatd:
  replicaCount: 3
  resources:
    requests:
      cpu: 500m
      memory: 256Mi
    limits:
      cpu: "2"
      memory: 1Gi
  cache:
    capacity: 10000
    ttlSecs: 600
  pool:
    size: 30

autoscaling:
  enabled: true
  minReplicas: 3
  maxReplicas: 20

ingress:
  enabled: true
  className: nginx
  hosts:
    - host: mentatd.yourdomain.com
      paths:
        - path: /
          pathType: Prefix
  tls:
    - hosts:
        - mentatd.yourdomain.com
      secretName: mentatd-tls

postgresql:
  auth:
    existingSecret: my-pg-secret
  resources:
    requests:
      cpu: "1"
      memory: 2Gi
    limits:
      cpu: "4"
      memory: 8Gi
  persistence:
    size: 100Gi
    storageClass: fast-ssd

networkPolicy:
  enabled: true

podDisruptionBudget:
  enabled: true
  minAvailable: 2
```

Then install:

```bash
helm install pg-mentat ./helm/pg-mentat \
  --namespace pg-mentat \
  --create-namespace \
  -f values-production.yaml
```

### Upgrade

```bash
helm upgrade pg-mentat ./helm/pg-mentat \
  --namespace pg-mentat \
  -f values-production.yaml
```

### Uninstall

```bash
helm uninstall pg-mentat --namespace pg-mentat
```

Note: Persistent volume claims are not deleted on uninstall. Delete them manually if needed:

```bash
kubectl -n pg-mentat delete pvc -l app.kubernetes.io/component=database
```

## Configuration Reference

### mentatd

| Parameter | Description | Default |
|---|---|---|
| `mentatd.replicaCount` | Number of mentatd replicas | `2` |
| `mentatd.image.repository` | Container image | `pg-mentat/mentatd` |
| `mentatd.image.tag` | Image tag | Chart appVersion |
| `mentatd.resources.requests.cpu` | CPU request | `100m` |
| `mentatd.resources.requests.memory` | Memory request | `128Mi` |
| `mentatd.resources.limits.cpu` | CPU limit | `1` |
| `mentatd.resources.limits.memory` | Memory limit | `512Mi` |
| `mentatd.config.port` | HTTP listen port | `8080` |
| `mentatd.config.logLevel` | Log level | `info` |
| `mentatd.config.logFormat` | Log format (json/compact/pretty) | `json` |
| `mentatd.cache.enabled` | Enable query cache | `true` |
| `mentatd.cache.capacity` | Cache max entries | `5000` |
| `mentatd.cache.ttlSecs` | Cache TTL in seconds | `300` |
| `mentatd.pool.size` | Connection pool size | `20` |

### PostgreSQL

| Parameter | Description | Default |
|---|---|---|
| `postgresql.enabled` | Deploy bundled PostgreSQL | `true` |
| `postgresql.auth.username` | Database username | `mentat` |
| `postgresql.auth.password` | Database password | `mentat` |
| `postgresql.auth.database` | Database name | `mentat` |
| `postgresql.auth.existingSecret` | Use existing secret | `""` |
| `postgresql.persistence.enabled` | Enable persistent storage | `true` |
| `postgresql.persistence.size` | PVC size | `10Gi` |
| `postgresql.persistence.storageClass` | Storage class | `""` (default) |
| `postgresql.resources.requests.cpu` | CPU request | `250m` |
| `postgresql.resources.limits.memory` | Memory limit | `2Gi` |

### Scaling and HA

| Parameter | Description | Default |
|---|---|---|
| `autoscaling.enabled` | Enable HPA | `true` |
| `autoscaling.minReplicas` | Minimum replicas | `2` |
| `autoscaling.maxReplicas` | Maximum replicas | `10` |
| `autoscaling.targetCPUUtilizationPercentage` | CPU target | `70` |
| `podDisruptionBudget.enabled` | Enable PDB | `true` |
| `podDisruptionBudget.minAvailable` | Min available pods | `1` |

## Production Considerations

### Secrets Management

Do not store passwords in plain text in values files. Use one of:

- **Kubernetes Secrets** with an existing secret: set `postgresql.auth.existingSecret`
- **External Secrets Operator** to sync from AWS Secrets Manager, Vault, etc.
- **Sealed Secrets** for GitOps workflows

### Using an External PostgreSQL

To connect to an existing PostgreSQL instance instead of deploying one:

```yaml
postgresql:
  enabled: false

mentatd:
  extraEnv:
    - name: DATABASE_URL
      valueFrom:
        secretKeyRef:
          name: my-external-db-secret
          key: connection-string
```

You will also need to update the ConfigMap connection string or override it via environment variables.

### Monitoring

mentatd exposes Prometheus metrics at `/metrics`. The pod annotations are pre-configured for Prometheus scraping:

```yaml
prometheus.io/scrape: "true"
prometheus.io/port: "8080"
prometheus.io/path: "/metrics"
```

Available metrics include:
- `mentatd_requests_total` -- total HTTP requests
- `mentatd_query_total` -- total queries
- `mentatd_query_duration_seconds` -- query latency histogram
- `mentatd_transactions_total` -- total transactions
- `mentatd_cache_hits_total` / `mentatd_cache_misses_total` -- cache hit rate
- `mentatd_errors_total` -- error count
- `mentatd_connection_pool_size` -- current pool size

### Resource Sizing

Guidelines for production:

| Workload | mentatd CPU | mentatd Memory | PostgreSQL CPU | PostgreSQL Memory | PVC |
|---|---|---|---|---|---|
| Small (< 100 req/s) | 200m-500m | 256Mi | 500m-1 | 1Gi | 10Gi |
| Medium (100-1000 req/s) | 500m-2 | 512Mi-1Gi | 1-2 | 2Gi-4Gi | 50Gi |
| Large (> 1000 req/s) | 1-4 | 1Gi-2Gi | 2-8 | 4Gi-16Gi | 100Gi+ |

### Network Policies

Enable network policies in production to restrict traffic:

```yaml
networkPolicy:
  enabled: true
```

This ensures:
- Only mentatd pods can reach PostgreSQL
- PostgreSQL has no outbound access except DNS
- mentatd accepts inbound HTTP and can reach PostgreSQL and DNS

### Backup

For PostgreSQL data backup, consider:
- **VolumeSnapshot** if your storage class supports CSI snapshots
- **pg_dump** via a CronJob
- **pgBackRest** or **Barman** for continuous archiving

Example CronJob for pg_dump:

```yaml
apiVersion: batch/v1
kind: CronJob
metadata:
  name: pg-mentat-backup
  namespace: pg-mentat
spec:
  schedule: "0 2 * * *"
  jobTemplate:
    spec:
      template:
        spec:
          containers:
            - name: backup
              image: postgres:16-bookworm
              command:
                - sh
                - -c
                - |
                  pg_dump -h pg-mentat-postgres -U mentat mentat | \
                    gzip > /backups/mentat-$(date +%Y%m%d-%H%M%S).sql.gz
              env:
                - name: PGPASSWORD
                  valueFrom:
                    secretKeyRef:
                      name: pg-mentat-secrets
                      key: postgres-password
              volumeMounts:
                - name: backups
                  mountPath: /backups
          restartPolicy: OnFailure
          volumes:
            - name: backups
              persistentVolumeClaim:
                claimName: pg-mentat-backups
```

## Troubleshooting

### mentatd pods not starting

Check if PostgreSQL is ready:

```bash
kubectl -n pg-mentat get pods -l app.kubernetes.io/component=database
kubectl -n pg-mentat logs statefulset/pg-mentat-postgres
```

Check mentatd logs:

```bash
kubectl -n pg-mentat logs deployment/mentatd
```

### Extension not loading

Verify the init container completed:

```bash
kubectl -n pg-mentat describe pod pg-mentat-postgres-0
```

Check that the extension files were copied:

```bash
kubectl -n pg-mentat exec pg-mentat-postgres-0 -- ls -la /usr/lib/postgresql/16/lib/pg_mentat.so
kubectl -n pg-mentat exec pg-mentat-postgres-0 -- ls -la /usr/share/postgresql/16/extension/pg_mentat*
```

### Connection issues

Test connectivity from a mentatd pod to PostgreSQL:

```bash
kubectl -n pg-mentat exec deployment/mentatd -- \
  sh -c 'nc -z pg-mentat-postgres 5432 && echo OK || echo FAIL'
```

### Health check

```bash
kubectl -n pg-mentat port-forward svc/mentatd 8080:8080
curl http://localhost:8080/health
curl http://localhost:8080/metrics
```
