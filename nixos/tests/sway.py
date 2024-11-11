# Code below borrowed from $nixpkgs/nixos/tests/sway.nix
# with minor modification (vm name+username changed)
import shlex
import json

q = shlex.quote
NODE_GROUPS = ["nodes", "floating_nodes"]


def swaymsg(command: str = "", succeed=True, type="command", machine = guivm):
    assert command != "" or type != "command", "Must specify command or type"
    shell = q(f"swaymsg -t {q(type)} -- {q(command)}")
    with machine.nested(
        f"sending swaymsg {shell!r}" + " (allowed to fail)" * (not succeed)
    ):
        run = machine.succeed if succeed else machine.execute
        ret = run(
            f"su - ghaf -c {shell}"
        )

    # execute also returns a status code, but disregard.
    if not succeed:
        _, ret = ret

    if not succeed and not ret:
        return None

    parsed = json.loads(ret)
    return parsed


def walk(tree):
    yield tree
    for group in NODE_GROUPS:
        for node in tree.get(group, []):
            yield from walk(node)


def wait_for_window(pattern):
    def func(last_chance):
        nodes = (node["name"] for node in walk(swaymsg(type="get_tree")))

        if last_chance:
            nodes = list(nodes)
            guivm.log(f"Last call! Current list of windows: {nodes}")
        return any(pattern in name for name in nodes)
    retry(func, timeout=30)
