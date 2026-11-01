#!/usr/bin/env python3
"""Report maximum preorder BVH depth in built stage blobs."""
import argparse
import json
from pathlib import Path
import struct
import sys


def inspect(path: Path):
    data = path.read_bytes()
    if len(data) < 44:
        raise ValueError(f"{path}: shorter than StageHeader")
    chunk_count = struct.unpack_from("<I", data, 12)[0]
    chunk_offset = struct.unpack_from("<I", data, 40)[0]
    end = chunk_offset + chunk_count * 32
    if not chunk_count or chunk_offset < 44 or end > len(data):
        raise ValueError(f"{path}: invalid chunk array {chunk_offset=} {chunk_count=}")
    chunks = [
        struct.unpack_from("<HH", data, chunk_offset + index * 32 + 12)
        for index in range(chunk_count)
    ]
    memo = {}

    def depth(index):
        if index in memo:
            return memo[index]
        skip, leaf = chunks[index]
        if not index < skip <= chunk_count:
            raise ValueError(f"{path}: invalid subtree interval at chunk {index}")
        if leaf != 0xFFFF:
            if skip != index + 1:
                raise ValueError(f"{path}: leaf {index} has a subtree")
            value = 0
        else:
            left = index + 1
            if left >= skip:
                raise ValueError(f"{path}: empty inner node {index}")
            right = chunks[left][0]
            if not left < right < skip:
                raise ValueError(f"{path}: malformed child intervals at {index}")
            value = 1 + max(depth(left), depth(right))
        memo[index] = value
        return value

    maximum = 0
    roots = 0
    root = 0
    while root < chunk_count:
        skip = chunks[root][0]
        if not root < skip <= chunk_count:
            raise ValueError(f"{path}: invalid root interval at chunk {root}")
        maximum = max(maximum, depth(root))
        roots += 1
        root = skip
    return {"path": str(path), "chunks": chunk_count, "roots": roots,
            "max_depth": maximum}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", nargs="?", type=Path, default=Path("build/assets"))
    parser.add_argument("--max-depth", type=int,
                        help="fail if any stage tree exceeds this structural depth")
    args = parser.parse_args()
    paths = sorted(path for path in args.directory.glob("stage_*.bin")
                   if "_shared_" not in path.name and "_sky_" not in path.name)
    if not paths:
        parser.error(f"no stage blobs found in {args.directory}")
    try:
        rows = [inspect(path) for path in paths]
    except (OSError, ValueError, struct.error) as error:
        parser.exit(1, f"{error}\n")
    print(json.dumps(rows, indent=2))
    if args.max_depth is not None and any(row["max_depth"] > args.max_depth for row in rows):
        sys.exit(1)


if __name__ == "__main__":
    main()
