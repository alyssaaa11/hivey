#!/usr/bin/env python3
"""hivey cloud's stand-in for EnvSave inside a Claude Code cloud session.

There is no vault here: the secrets hivey copied (decrypted by .hivey-cloud/bootstrap.sh into
~/.hivey-cloud/secrets.json) answer `envsave get es_...` and, as the `envsave` Python module,
`envsave.get("es_...")`. A NAME works too. Network secrets return a placeholder: the cloud
proxy adds the real key to requests for their host.
"""

import json
import os
import sys

_PATH = os.path.join(os.path.expanduser("~"), ".hivey-cloud", "secrets.json")


def _load():
    try:
        with open(_PATH) as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def get(key, default=None):
    data = _load()
    value = (data.get("opaque") or {}).get(key)
    if value is None:
        value = (data.get("vars") or {}).get(key)
    if value is None and key in (data.get("network") or {}):
        value = "hivey-network-secret"
    if value is None:
        value = os.environ.get(key, default)
    if value is None:
        raise KeyError(f"hivey cloud: no secret {key} was copied to this session")
    return value


def _main(argv):
    if len(argv) == 2 and argv[0] == "get":
        try:
            print(get(argv[1]))
            return 0
        except KeyError as err:
            print(err.args[0], file=sys.stderr)
            return 1
    print("hivey cloud envsave shim: only `envsave get <es_key|NAME>` works here", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(_main(sys.argv[1:]))
