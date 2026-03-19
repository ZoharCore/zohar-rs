import re


def add_default_namespace_resource(resource, namespace, indent=""):
    meta = re.search(r"^%smetadata:\r?\n(\s+.*\r?\n)*" % indent, resource, re.MULTILINE)
    if not meta:
        return resource

    metadata = meta.group(0)
    metadata = re.sub(r"\r?\n\s+namespace:\s*\r?\n", "\n", metadata, flags=re.MULTILINE)

    has_namespace = re.search(r"\r?\n\s+namespace: *\S", metadata, re.MULTILINE)
    if has_namespace:
        return resource

    field_indent_match = re.search(r"^%smetadata:\r?\n(\s+)" % indent, metadata, re.MULTILINE)
    field_indent = field_indent_match.group(1) if field_indent_match else indent + "  "

    metadata = re.sub(
        r"^%smetadata:" % indent,
        "%smetadata:\n%snamespace: %s" % (indent, field_indent, namespace),
        metadata,
        count=1,
    )
    return resource[: meta.start()] + metadata + resource[meta.end() :]


def add_default_namespace_resource_list(resource, namespace):
    meta = re.search(r"^(\s+)metadata:\s*$", resource, flags=re.MULTILINE)
    if not meta:
        return resource

    indent = meta.group(1)
    items = re.split(r"^%smetadata:\s*$" % indent, resource, flags=re.MULTILINE)
    for i in range(1, len(items)):
        items[i] = add_default_namespace_resource("%smetadata:%s" % (indent, items[i]), namespace, indent=indent)

    return "".join(items)


def add_default_namespace(yaml, namespace):
    if not namespace:
        return yaml

    resources = re.split(r"^---$", yaml, flags=re.MULTILINE)
    for i, resource in enumerate(resources):
        kind = re.search(r"^kind:\s*(\w+)\s*$", resource, flags=re.MULTILINE)
        if kind and kind.group(1) == "List":
            resources[i] = add_default_namespace_resource_list(resource, namespace)
        else:
            resources[i] = add_default_namespace_resource(resource, namespace)

    return "---".join(resources)
