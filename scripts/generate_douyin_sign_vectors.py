# SPDX-License-Identifier: Apache-2.0
"""Regenerate fixtures from F2 f6be8c0ffba9a127075bbeafe4838716650b6325.

Run with Python and gmssl installed, passing the upstream abogus.py path.
The reference implementation is Apache-2.0, copyright JohnserfSeed.
"""

import argparse
import importlib.util
import json
from pathlib import Path
from unittest.mock import patch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("upstream", type=Path)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("f2_abogus", args.upstream)
    reference = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(reference)
    fp = "1920|1080|1944|1160|0|0|0|0|1920|1080|1920|1040|1920|1080|24|24|Win32"
    cases = [
        ("empty", "", "", ""),
        ("get", "device_platform=webapp&aid=6383&aweme_id=1234567890", "", "Mozilla/5.0 test"),
        ("unicode-query", "keyword=中文 😀&empty=&repeat=1&repeat=2", "", "Mozilla/5.0 test"),
        ("encoded-query", "keyword=%E4%B8%AD%E6%96%87+%F0%9F%98%80&reserved=%26%3D%2B%25", "", "Mozilla/5.0 test"),
        ("unicode-body", "aid=6383", '{"text":"中文 😀","enabled":true}', "Mozilla/5.0 test"),
        ("form-body", "aid=6383&blank=", "item_id=1234567890&source=0&text=a%2Bb+c", "test-agent/1.0"),
        ("latin1-ua", "a=1", "b=2", "test-agent/é"),
        ("clock-rollover", "a=1", "", "test-agent/1.0"),
        ("fingerprint-one", "a=1", "", "test-agent/1.0"),
        ("fingerprint-two", "a=1", "", "test-agent/1.0"),
        ("fingerprint-long", "a=1", "", "test-agent/1.0"),
    ]
    vectors = []
    for index, (name, query, body, ua) in enumerate(cases):
        start = 1700000000000 + index * 1000
        end = start + index
        if name == "clock-rollover":
            start, end = (1 << 40) - 1, 1 << 40
        random_values = [0, 9999, 255] if index % 2 == 0 else [1234, 5678, 9012]
        clock = [start / 1000, end / 1000]
        random = [(value + 0.5) / 10000 for value in random_values]
        assert [int(value * 10000) for value in random] == random_values
        fingerprint = {"fingerprint-one": "1", "fingerprint-two": "12",
                       "fingerprint-long": "1234567890|" * 26}.get(name, fp)
        signer = reference.ABogus(fp=fingerprint, user_agent=ua)
        with patch.object(reference.time, "time", side_effect=clock), patch.object(
            reference.random, "random", side_effect=random
        ):
            signed_query, signature, _, _ = signer.generate_abogus(query, body)
        assert signed_query == f"{query}&a_bogus={signature}"
        vectors.append(dict(name=name, query=query, body=body, ua=ua, fingerprint=fingerprint,
                            start_ms=start, end_ms=end, random=random_values, signature=signature))
    output = Path(__file__).resolve().parents[1] / "crates/favkeep/src/douyin/fixtures/sign.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({"upstream_commit": "f6be8c0ffba9a127075bbeafe4838716650b6325",
                                  "vectors": vectors}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {len(vectors)} public synthetic vectors to {output}")


if __name__ == "__main__":
    main()
