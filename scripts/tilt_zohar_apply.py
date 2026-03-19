import os
import subprocess
import sys
import time
from typing import Dict

from tilt_namespacing import add_default_namespace

sys.tracebacklimit = 0


def parse_image_string(image: str) -> Dict[str, str | None]:
    if "." in image or "localhost" in image or image.count(":") > 1:
        registry, repository = image.split("/", 1)
        repository, tag = repository.rsplit(":", 1)
        return {"registry": registry, "repository": repository, "tag": tag}

    repository, tag = image.rsplit(":", 1)
    return {"registry": None, "repository": repository, "tag": tag}


def maybe_reset_core_gameserver(app_namespace: str, release_name: str, desired_core_image: str) -> None:
    if not app_namespace or not desired_core_image:
        return

    selector = f"app.kubernetes.io/instance={release_name},app.kubernetes.io/component=core"
    pod_selector = f"agones.dev/role=gameserver,{selector}"
    pod_image_cmd = [
        "kubectl",
        "-n",
        app_namespace,
        "get",
        "pod",
        "-l",
        pod_selector,
        "-o",
        "jsonpath={.items[0].spec.containers[?(@.name=='zohar-core')].image}",
    ]
    current_pod_image = subprocess.run(
        pod_image_cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        check=False,
    ).stdout.strip()

    if current_pod_image == desired_core_image:
        print(
            f"core GameServer already running desired image {desired_core_image}; skipping reset",
            file=sys.stderr,
        )
        return

    delete_gs_cmd = [
        "kubectl",
        "-n",
        app_namespace,
        "delete",
        "gameserver",
        "-l",
        selector,
        "--ignore-not-found",
        "--wait=false",
    ]
    print(f"Running cmd: {delete_gs_cmd}", file=sys.stderr)
    subprocess.check_call(delete_gs_cmd, stdout=sys.stderr)

    delete_pod_cmd = [
        "kubectl",
        "-n",
        app_namespace,
        "delete",
        "pod",
        "-l",
        pod_selector,
        "--ignore-not-found",
        "--wait=false",
    ]
    print(f"Running cmd: {delete_pod_cmd}", file=sys.stderr)
    subprocess.check_call(delete_pod_cmd, stdout=sys.stderr)

    deadline = time.time() + 90
    while time.time() < deadline:
        gs_cmd = [
            "kubectl",
            "-n",
            app_namespace,
            "get",
            "gameserver",
            "-l",
            selector,
            "-o",
            "name",
        ]
        pod_cmd = [
            "kubectl",
            "-n",
            app_namespace,
            "get",
            "pod",
            "-l",
            pod_selector,
            "-o",
            "name",
        ]
        gs_exists = subprocess.run(
            gs_cmd, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, check=False
        ).stdout.strip()
        pod_exists = subprocess.run(
            pod_cmd, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, check=False
        ).stdout.strip()
        if not gs_exists and not pod_exists:
            return

        time.sleep(2)

    gs_debug_cmd = [
        "kubectl",
        "-n",
        app_namespace,
        "get",
        "gameserver",
        "-l",
        selector,
        "-o",
        "yaml",
    ]
    pod_debug_cmd = [
        "kubectl",
        "-n",
        app_namespace,
        "get",
        "pod",
        "-l",
        pod_selector,
        "-o",
        "yaml",
    ]
    print("Timed out waiting for core GameServer resources to disappear", file=sys.stderr)
    print(f"Running cmd: {gs_debug_cmd}", file=sys.stderr)
    subprocess.run(gs_debug_cmd, stdout=sys.stderr, stderr=sys.stderr, check=False)
    print(f"Running cmd: {pod_debug_cmd}", file=sys.stderr)
    subprocess.run(pod_debug_cmd, stdout=sys.stderr, stderr=sys.stderr, check=False)

    raise RuntimeError("core GameServer resources did not fully disappear in time")


flags = sys.argv[1:]

image_count = int(os.environ["TILT_IMAGE_COUNT"])
desired_core_image = os.environ.get("TILT_IMAGE_0", "")
for i in range(image_count):
    image = os.environ[f"TILT_IMAGE_{i}"]
    count = int(os.environ[f"TILT_IMAGE_KEY_COUNT_{i}"])
    for j in range(count):
        suffix = f"{i}_{j}"
        key = os.environ.get(f"TILT_IMAGE_KEY_{suffix}", "")
        if key:
            flags.extend(["--set", f"{key}={image}"])
            continue

        image_parts = parse_image_string(image)
        key0 = os.environ.get(f"TILT_IMAGE_KEY_REGISTRY_{suffix}", "")
        key1 = os.environ.get(f"TILT_IMAGE_KEY_REPO_{suffix}", "")
        key2 = os.environ.get(f"TILT_IMAGE_KEY_TAG_{suffix}", "")

        if image_parts["registry"]:
            if key0:
                flags.extend(
                    [
                        "--set",
                        f"{key0}={image_parts['registry']}",
                        "--set",
                        f"{key1}={image_parts['repository']}",
                    ]
                )
            else:
                flags.extend(["--set", f"{key1}={image_parts['registry']}/{image_parts['repository']}"])
        else:
            flags.extend(["--set", f"{key1}={image_parts['repository']}"])

        flags.extend(["--set", f"{key2}={image_parts['tag']}"])

install_cmd = ["helm", "upgrade", "--install"]
install_cmd.extend(flags)

get_cmd = ["helm", "get", "manifest"]
kubectl_cmd = ["kubectl", "get"]

release_name = os.environ["RELEASE_NAME"]
chart = os.environ["CHART"]
namespace = os.environ.get("NAMESPACE", "")
app_namespace = os.environ.get("APP_NAMESPACE", "")
if namespace:
    install_cmd.extend(["--namespace", namespace])
    get_cmd.extend(["--namespace", namespace])

install_cmd.extend([release_name, chart])
get_cmd.extend([release_name])
kubectl_cmd.extend(["-oyaml", "-f", "-"])

maybe_reset_core_gameserver(app_namespace, release_name, desired_core_image)

print(f"Running cmd: {install_cmd}", file=sys.stderr)
subprocess.check_call(install_cmd, stdout=sys.stderr)

print(f"Running cmd: {get_cmd}", file=sys.stderr)
out = subprocess.check_output(get_cmd).decode("utf-8")
input_yaml = add_default_namespace(out, namespace).encode("utf-8")

print(f"Running cmd: {kubectl_cmd}", file=sys.stderr)
completed = subprocess.run(kubectl_cmd, input=input_yaml)
completed.check_returncode()
