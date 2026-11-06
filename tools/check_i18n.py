"""Validate translation catalogs and literal UI message keys (standard library only)."""
import collections
import json
from pathlib import Path
import re
import string
import sys

ROOT = Path(__file__).resolve().parents[1]
LOCALES = ROOT / "crates/app/locales"


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate message: {key}")
        result[key] = value
    return result


def slots(message):
    result = collections.Counter()
    for _, name, spec, conversion in string.Formatter().parse(message):
        if name is not None:
            if not name.isidentifier() or spec or conversion:
                raise ValueError(f"use plain named placeholders: {message}")
            result[name] += 1
    return result


def main():
    # Ignore comments and test fixtures. Macro keys are Rust string literals;
    # JSON decoding also handles the escapes used by these source messages.
    tokens = re.compile(r'//[^\n]*|/\*[\s\S]*?\*/|(?<!\w)(?:\w+::)*tr!\(\s*("(?:\\.|[^"\\])*")')
    sources = set()
    for path in (ROOT / "crates/app/src").glob("*.rs"):
        source = re.split(r"#\[cfg\(test\)\]\s*mod tests\s*\{", path.read_text(encoding="utf-8"))[0]
        for match in tokens.finditer(source):
            if match[1]:
                sources.add(json.loads(match[1]))
    for path in sorted(LOCALES.glob("*.json")):
        catalog = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
        for key, value in catalog.items():
            if not isinstance(value, str) or not value.strip():
                raise ValueError(f"{path.name}: empty or invalid translation: {key}")
            if slots(key) != slots(value):
                raise ValueError(f"{path.name}: placeholders differ: {key}")
        missing = sources - catalog.keys()
        if missing:
            raise ValueError(f"{path.name}: missing UI messages: {sorted(missing)}")
        print(f"{path.name}: {len(catalog)} messages; {len(sources)} UI keys verified")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, TypeError) as error:
        sys.exit(str(error))
