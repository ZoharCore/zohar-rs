load('ext://helm_resource', 'helm_resource')

allow_k8s_contexts('orbstack')

tenant = os.getenv('ZOHAR_TENANT', 'nightlysrv')
profile = os.getenv('ZOHAR_PROFILE', 'dev')
infra_namespace = os.getenv('ZOHAR_INFRA_NAMESPACE', 'infra')

if profile not in ['dev', 'prod']:
    fail('ZOHAR_PROFILE must be one of: dev, prod')

profile_values = 'deploy/charts/zohar/values-%s.yaml' % profile

local_resource(
    'prepare-content',
    './scripts/prepare-content.sh',
    deps=['scripts/prepare-content.sh'],
)

docker_build(
    'zohar-core',
    '.',
    dockerfile='Dockerfile',
    target='core-runtime',
    ignore=['target', '.git'],
)

docker_build(
    'zohar-auth',
    '.',
    dockerfile='Dockerfile',
    target='auth-runtime',
    ignore=['target', '.git'],
)

docker_build(
    'zohar-chgw',
    '.',
    dockerfile='Dockerfile',
    target='channel-runtime',
    ignore=['target', '.git'],
)

local_resource(
    'agones',
    '''
set -e
helm repo add agones https://agones.dev/chart/stable >/dev/null 2>&1 || true
helm repo update
helm upgrade --install agones agones/agones --namespace agones-system --create-namespace \
  --set agones.controller.replicas=1 \
  --set agones.extensions.replicas=1 \
  --set agones.allocator.install=false \
  --set agones.ping.install=false \
  --set agones.metrics.prometheusEnabled=false \
  --set agones.metrics.prometheusServiceDiscovery=false
kubectl -n agones-system rollout status deployment/agones-controller --timeout=180s
kubectl -n agones-system rollout status deployment/agones-extensions --timeout=180s
for i in $(seq 1 90); do
  if kubectl -n agones-system get endpoints agones-controller-service -o jsonpath='{.subsets[0].addresses[0].ip}' 2>/dev/null | grep -q .; then
    exit 0
  fi
  sleep 2
done
echo "agones-controller-service endpoints not ready in time" >&2
exit 1
''',
)

helm_resource(
    'zohar',
    chart='deploy/charts/zohar',
    namespace='default',
    flags=[
        '--server-side', 'false',
        '--force-replace',
        '-f', 'deploy/charts/zohar/values.yaml',
        '-f', profile_values,
        '--set-string', 'tenant=%s' % tenant,
        '--set-string', 'profile=%s' % profile,
        '--set-string', 'infraNamespace=%s' % infra_namespace,
    ],
    resource_deps=['agones', 'prepare-content'],
    image_deps=['zohar-core', 'zohar-auth', 'zohar-chgw'],
    image_keys=[
        ('core.image.repository', 'core.image.tag'),
        ('auth.image.repository', 'auth.image.tag'),
        ('channelGateway.image.repository', 'channelGateway.image.tag'),
    ],
)
