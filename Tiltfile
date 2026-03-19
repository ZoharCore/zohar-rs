allow_k8s_contexts('orbstack')
update_settings(k8s_upsert_timeout_secs=300)

tenant = os.getenv('ZOHAR_TENANT', 'nightlysrv')
profile = os.getenv('ZOHAR_PROFILE', 'dev')
infra_namespace = os.getenv('ZOHAR_INFRA_NAMESPACE', 'infra')
app_namespace = os.getenv('ZOHAR_NAMESPACE_OVERRIDE', '%s-%s' % (tenant, profile))

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
if [ "${ZOHAR_REFRESH_AGONES:-}" != "1" ] && helm status agones -n agones-system >/dev/null 2>&1; then
  echo "agones already installed; skipping helm upgrade"
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
fi

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

k8s_custom_deploy(
    'zohar',
    apply_cmd=[
        'python3',
        'scripts/tilt_zohar_apply.py',
        '--server-side', 'false',
        '--force-replace',
        '-f', 'deploy/charts/zohar/values.yaml',
        '-f', profile_values,
        '--set-string', 'tenant=%s' % tenant,
        '--set-string', 'profile=%s' % profile,
        '--set-string', 'infraNamespace=%s' % infra_namespace,
    ],
    apply_env={
        'CHART': 'deploy/charts/zohar',
        'RELEASE_NAME': 'zohar',
        'NAMESPACE': 'default',
        'APP_NAMESPACE': app_namespace,
        'TILT_IMAGE_COUNT': '3',
        'TILT_IMAGE_KEY_COUNT_0': '1',
        'TILT_IMAGE_KEY_REPO_0_0': 'core.image.repository',
        'TILT_IMAGE_KEY_TAG_0_0': 'core.image.tag',
        'TILT_IMAGE_KEY_COUNT_1': '1',
        'TILT_IMAGE_KEY_REPO_1_0': 'auth.image.repository',
        'TILT_IMAGE_KEY_TAG_1_0': 'auth.image.tag',
        'TILT_IMAGE_KEY_COUNT_2': '1',
        'TILT_IMAGE_KEY_REPO_2_0': 'channelGateway.image.repository',
        'TILT_IMAGE_KEY_TAG_2_0': 'channelGateway.image.tag',
    },
    delete_cmd=['python3', 'scripts/tilt_zohar_delete.py'],
    delete_env={
        'RELEASE_NAME': 'zohar',
        'NAMESPACE': 'default',
    },
    deps=[
        'Tiltfile',
        'scripts/tilt_zohar_apply.py',
        'scripts/tilt_zohar_delete.py',
        'scripts/tilt_namespacing.py',
        'deploy/charts/zohar',
    ],
    image_deps=['zohar-core', 'zohar-auth', 'zohar-chgw'],
)
k8s_resource('zohar', resource_deps=['agones', 'prepare-content'])
