#!/usr/bin/env python3
"""List main-namespace, non-redirect titles of a jawiki dump part, skipping
the first N such pages (the ones a corpus already took). Used to pick
`no_answer` queries: titles of real articles outside the corpus.

Usage: dump_titles.py --dump PART.bz2 --skip 1300 --count 3000 > titles.txt
"""

import argparse
import bz2
import re
import sys

PAGE = re.compile(rb"<page>.*?</page>", re.S)
TITLE = re.compile(rb"<title>(.*?)</title>")
NS = re.compile(rb"<ns>(\d+)</ns>")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dump", required=True)
    parser.add_argument("--skip", type=int, default=1300)
    parser.add_argument("--count", type=int, default=3000)
    args = parser.parse_args()
    seen = emitted = 0
    buffer = b""
    with bz2.open(args.dump, "rb") as stream:
        while emitted < args.count:
            chunk = stream.read(1 << 20)
            if not chunk:
                break
            buffer += chunk
            last = 0
            for match in PAGE.finditer(buffer):
                last = match.end()
                page = match.group(0)
                ns = NS.search(page)
                if not ns or ns.group(1) != b"0" or b"<redirect" in page:
                    continue
                seen += 1
                if seen <= args.skip:
                    continue
                title = TITLE.search(page).group(1).decode("utf-8")
                sys.stdout.write(title + "\n")
                emitted += 1
                if emitted >= args.count:
                    break
            buffer = buffer[last:]
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
