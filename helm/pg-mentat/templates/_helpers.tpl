{{/*
Expand the name of the chart.
*/}}
{{- define "pg-mentat.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "pg-mentat.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "pg-mentat.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "pg-mentat.labels" -}}
helm.sh/chart: {{ include "pg-mentat.chart" . }}
{{ include "pg-mentat.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels for mentatd
*/}}
{{- define "pg-mentat.selectorLabels" -}}
app.kubernetes.io/name: {{ include "pg-mentat.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Service account name for mentatd
*/}}
{{- define "pg-mentat.serviceAccountName" -}}
{{- if .Values.mentatd.serviceAccount.create }}
{{- default (include "pg-mentat.fullname" .) .Values.mentatd.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.mentatd.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
PostgreSQL fullname
*/}}
{{- define "pg-mentat.postgresql.fullname" -}}
{{- printf "%s-postgres" (include "pg-mentat.fullname" .) }}
{{- end }}

{{/*
PostgreSQL connection string
*/}}
{{- define "pg-mentat.postgresql.connectionString" -}}
postgresql://{{ .Values.postgresql.auth.username }}:{{ .Values.postgresql.auth.password }}@{{ include "pg-mentat.postgresql.fullname" . }}:5432/{{ .Values.postgresql.auth.database }}
{{- end }}

{{/*
mentatd image
*/}}
{{- define "pg-mentat.mentatd.image" -}}
{{- printf "%s:%s" .Values.mentatd.image.repository (default .Chart.AppVersion .Values.mentatd.image.tag) }}
{{- end }}

{{/*
pg_mentat extension image
*/}}
{{- define "pg-mentat.extension.image" -}}
{{- printf "%s:%s" .Values.postgresql.extensionImage.repository (default .Chart.AppVersion .Values.postgresql.extensionImage.tag) }}
{{- end }}

{{/*
PostgreSQL secret name
*/}}
{{- define "pg-mentat.postgresql.secretName" -}}
{{- if .Values.postgresql.auth.existingSecret }}
{{- .Values.postgresql.auth.existingSecret }}
{{- else }}
{{- printf "%s-postgres-auth" (include "pg-mentat.fullname" .) }}
{{- end }}
{{- end }}
