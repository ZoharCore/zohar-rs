import os
import subprocess
import sys

sys.tracebacklimit = 0

release_name = os.environ["RELEASE_NAME"]
namespace = os.environ.get("NAMESPACE", "")
namespace_args = ["--namespace", namespace] if namespace else []

status_cmd = ["helm", "status", release_name] + namespace_args
status_result = subprocess.call(
    status_cmd,
    stdout=subprocess.DEVNULL,
    stderr=subprocess.DEVNULL,
)
if status_result == 0:
    delete_cmd = ["helm", "uninstall", release_name] + namespace_args
    print("Running cmd: %s" % " ".join(delete_cmd), file=sys.stderr)
    subprocess.call(delete_cmd)
