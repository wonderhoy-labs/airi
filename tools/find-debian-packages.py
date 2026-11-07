"""Prints each package's path and checksum from a Debian package index, for tools/build-agent.sh."""
import sys


def main(index, wanted):
    found = {}
    with open(index, encoding="utf-8", errors="replace") as file:
        for block in file.read().split("\n\n"):
            fields = dict(line.split(": ", 1) for line in block.split("\n") if ": " in line and line[0] != " ")
            if fields.get("Package") in wanted:
                found[fields["Package"]] = fields
    for name in wanted:
        print(name, found[name]["Filename"], found[name]["SHA256"])


main(sys.argv[1], sys.argv[2:])
