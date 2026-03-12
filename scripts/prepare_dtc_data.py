#!/usr/bin/env python3
"""Extract DTC codes from Wal33D/dtc-database into TSV files.

Downloads the SQLite database (or uses a local copy) and produces:
  crates/zc-canbus-tools/data/dtc_generic.tsv   — generic OBD-II codes
  crates/zc-canbus-tools/data/dtc_manufacturer.tsv — manufacturer-specific codes

Usage:
  python3 scripts/prepare_dtc_data.py                    # download from GitHub
  python3 scripts/prepare_dtc_data.py --input db.sqlite  # use local file
"""

import argparse
import os
import sqlite3
import sys
import tempfile
import urllib.request
from datetime import datetime, timezone

# Pinned source
REPO = "Wal33D/dtc-database"
COMMIT_SHA = "04c43d72e7db7197658b6f72fe582c5076d9eee8"
DB_URL = f"https://raw.githubusercontent.com/{REPO}/{COMMIT_SHA}/data/dtc_codes.db"
LICENSE = "MIT"

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.dirname(SCRIPT_DIR)
DATA_DIR = os.path.join(PROJECT_ROOT, "crates", "zc-canbus-tools", "data")


def download_db(dest: str) -> None:
    print(f"Downloading {DB_URL} ...")
    urllib.request.urlretrieve(DB_URL, dest)
    print(f"Downloaded to {dest} ({os.path.getsize(dest)} bytes)")


def extract(db_path: str) -> None:
    conn = sqlite3.connect(db_path)
    cur = conn.cursor()

    # Verify schema
    cur.execute("SELECT COUNT(*) FROM dtc_definitions")
    total = cur.fetchone()[0]
    print(f"Total rows in dtc_definitions: {total}")

    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    metadata = (
        f"# Source: https://github.com/{REPO}\n"
        f"# Commit: {COMMIT_SHA}\n"
        f"# License: {LICENSE}\n"
        f"# Extracted: {timestamp}\n"
    )

    # Generic codes: is_generic=1 OR manufacturer='GENERIC'
    cur.execute(
        "SELECT code, description FROM dtc_definitions "
        "WHERE is_generic = 1 OR manufacturer = 'GENERIC' "
        "ORDER BY code"
    )
    generic_rows = cur.fetchall()

    generic_path = os.path.join(DATA_DIR, "dtc_generic.tsv")
    with open(generic_path, "w") as f:
        f.write(metadata)
        f.write(f"# Rows: {len(generic_rows)}\n")
        for code, desc in generic_rows:
            # Sanitize: strip whitespace, replace tabs/newlines
            code = code.strip()
            desc = desc.strip().replace("\t", " ").replace("\n", " ")
            if code and desc:
                f.write(f"{code}\t{desc}\n")
    print(f"Wrote {len(generic_rows)} generic codes to {generic_path}")

    # Manufacturer-specific codes: is_generic=0 AND manufacturer != 'GENERIC'
    cur.execute(
        "SELECT code, manufacturer, description FROM dtc_definitions "
        "WHERE is_generic = 0 AND manufacturer != 'GENERIC' "
        "ORDER BY code, manufacturer"
    )
    mfr_rows = cur.fetchall()

    mfr_path = os.path.join(DATA_DIR, "dtc_manufacturer.tsv")
    with open(mfr_path, "w") as f:
        f.write(metadata)
        f.write(f"# Rows: {len(mfr_rows)}\n")
        for code, mfr, desc in mfr_rows:
            code = code.strip()
            mfr = mfr.strip().upper()
            desc = desc.strip().replace("\t", " ").replace("\n", " ")
            if code and desc:
                f.write(f"{code}\t{mfr}\t{desc}\n")
    print(f"Wrote {len(mfr_rows)} manufacturer codes to {mfr_path}")

    # Summary stats
    cur.execute(
        "SELECT type, COUNT(*) FROM dtc_definitions GROUP BY type ORDER BY type"
    )
    for dtype, count in cur.fetchall():
        print(f"  {dtype}-codes: {count}")

    conn.close()


def main() -> None:
    parser = argparse.ArgumentParser(description="Extract DTC data from Wal33D/dtc-database")
    parser.add_argument("--input", help="Path to local dtc_codes.db SQLite file")
    args = parser.parse_args()

    os.makedirs(DATA_DIR, exist_ok=True)

    if args.input:
        if not os.path.exists(args.input):
            print(f"Error: {args.input} not found", file=sys.stderr)
            sys.exit(1)
        extract(args.input)
    else:
        with tempfile.NamedTemporaryFile(suffix=".db", delete=False) as tmp:
            tmp_path = tmp.name
        try:
            download_db(tmp_path)
            extract(tmp_path)
        finally:
            os.unlink(tmp_path)

    print("\nDone. Commit the TSV files in crates/zc-canbus-tools/data/")


if __name__ == "__main__":
    main()
