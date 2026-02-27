{{- define "zohar.tenant" -}}
{{- default "" .Values.tenant -}}
{{- end -}}

{{- define "zohar.tenantNamespace" -}}
{{- if .Values.tenant -}}
{{- printf "%s-%s" .Values.tenant (include "zohar.profile" .) -}}
{{- end -}}
{{- end -}}

{{- define "zohar.namespace" -}}
{{- if .Values.namespaceOverride -}}
{{- .Values.namespaceOverride -}}
{{- else if .Values.tenant -}}
{{- include "zohar.tenantNamespace" . -}}
{{- else -}}
{{- .Release.Namespace -}}
{{- end -}}
{{- end -}}

{{- define "zohar.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "zohar.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else if contains (include "zohar.name" .) .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name (include "zohar.name" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "zohar.labels" -}}
app.kubernetes.io/name: {{ include "zohar.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
zohar.io/profile: {{ include "zohar.profile" . | quote }}
zohar.io/tenant: {{ default "unspecified" (include "zohar.tenant" .) | quote }}
{{- end -}}

{{- define "zohar.profile" -}}
{{- required "values profile is required; deploy with -f values.yaml -f values-dev.yaml or -f values-prod.yaml" .Values.profile -}}
{{- end -}}

{{- define "zohar.authDbSecretName" -}}
{{- if .Values.authDatabase.secret.name -}}
{{- .Values.authDatabase.secret.name -}}
{{- else -}}
{{- printf "%s-auth-db-url" (include "zohar.fullname" .) -}}
{{- end -}}
{{- end -}}

{{- define "zohar.authDbSecretKey" -}}
{{- default "url" .Values.authDatabase.secret.key -}}
{{- end -}}

{{- define "zohar.postgresServiceName" -}}
{{- printf "%s-postgres" (include "zohar.fullname" .) -}}
{{- end -}}

{{- define "zohar.infraNamespace" -}}
{{- default "infra" .Values.infraNamespace -}}
{{- end -}}

{{- define "zohar.postgresNamespace" -}}
{{- coalesce .Values.postgres.namespaceOverride (include "zohar.infraNamespace" .) -}}
{{- end -}}

{{- define "zohar.postgresHost" -}}
{{- $clusterDomain := default "cluster.local" .Values.postgres.clusterDomain -}}
{{- printf "%s.%s.svc.%s" (include "zohar.postgresServiceName" .) (include "zohar.postgresNamespace" .) $clusterDomain -}}
{{- end -}}

{{- define "zohar.authDatabaseUrl" -}}
{{- if .Values.authDatabase.url -}}
{{- .Values.authDatabase.url -}}
{{- else -}}
{{- printf "postgres://%s:%s@%s:%v/%s" .Values.postgres.credentials.username .Values.postgres.credentials.password (include "zohar.postgresHost" .) .Values.postgres.service.port .Values.postgres.credentials.database -}}
{{- end -}}
{{- end -}}

{{- define "zohar.gameDbSecretName" -}}
{{- if .Values.gameDatabase.secret.name -}}
{{- .Values.gameDatabase.secret.name -}}
{{- else -}}
{{- printf "%s-game-db-url" (include "zohar.fullname" .) -}}
{{- end -}}
{{- end -}}

{{- define "zohar.gameDbSecretKey" -}}
{{- default "url" .Values.gameDatabase.secret.key -}}
{{- end -}}

{{- define "zohar.gameDatabaseUrl" -}}
{{- if .Values.gameDatabase.url -}}
{{- .Values.gameDatabase.url -}}
{{- else -}}
{{- printf "postgres://%s:%s@%s:%v/%s" .Values.postgres.credentials.username .Values.postgres.credentials.password (include "zohar.postgresHost" .) .Values.postgres.service.port .Values.postgres.credentials.database -}}
{{- end -}}
{{- end -}}

{{- define "zohar.authTokenSecretName" -}}
{{- default "zohar-authsrv-token" .Values.auth.token.secret.name -}}
{{- end -}}

{{- define "zohar.authTokenSecretKey" -}}
{{- default "secret" .Values.auth.token.secret.key -}}
{{- end -}}

{{- define "zohar.coreServiceAccountName" -}}
{{- printf "%s-core" (include "zohar.fullname" .) -}}
{{- end -}}

{{- define "zohar.channelServiceAccountName" -}}
{{- printf "%s-channel" (include "zohar.fullname" .) -}}
{{- end -}}

{{- define "zohar.coreGameServerName" -}}
{{- $root := index . "root" -}}
{{- $channelId := int (index . "channelId") -}}
{{- $map := index . "map" -}}
{{- printf "%s-ch%d-%s" (include "zohar.fullname" $root) $channelId ($map | replace "_" "-" | replace "." "-" | lower) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "zohar.channelEntryServiceName" -}}
{{- $root := index . "root" -}}
{{- $channelId := int (index . "channelId") -}}
{{- printf "%s-ch%d-entry" (include "zohar.fullname" $root) $channelId | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "zohar.channelGatewayDeploymentName" -}}
{{- $root := index . "root" -}}
{{- $channelId := int (index . "channelId") -}}
{{- printf "%s-ch%d-gateway" (include "zohar.fullname" $root) $channelId | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "zohar.mapEndpointServiceName" -}}
{{- $root := index . "root" -}}
{{- $channelId := int (index . "channelId") -}}
{{- $map := index . "map" -}}
{{- printf "%s-ch%d-%s-endpoint" (include "zohar.fullname" $root) $channelId ($map | replace "_" "-" | replace "." "-" | lower) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
