#!/usr/bin/env python3
"""Local-only import from a FORScan SQLite dump into canforge's private data directory.

    python3 -I scripts/forscan_import.py ~/Downloads/forscan.sqlite            # dry run: prints what it would write
    python3 -I scripts/forscan_import.py ~/Downloads/forscan.sqlite --write    # writes, under <data dir>:
        modules.d/forscan.tsv     module names by request ID
        forscan/functions.tsv     what each module can do (shown, never run)
        forscan/params.tsv        readable data identifiers with scaling (read on request)
        dtc.d/forscan.tsv         trouble code descriptions (confidently decoded generic codes only)

The dump is proprietary. This writes only to your own data directory (never the repo, never vehicle-db) and none of these
files is picked up by Help improve's export, so they are never uploaded. vehicle-db/LEGAL.md excludes FORScan data from the
public database: do not copy these files there. Check FORScan's terms for your own local use.

How it works (worked out by inspection, not documented upstream; see ~/Documents/forscan-sqlite-report.md):
- data12 lists modules (id, acronym, name text id). data8 holds definitions with the module id in c1 and the CAN request
  ID in c4. For each 0x7xx ID the module with the most definitions names it; an ID is kept only with at least 3 definitions
  and 1.5x the runner-up, so ambiguous IDs are skipped rather than guessed.
- data21 lists functions (title text id in NAME_ID, UDS routine in ROUTINE_ID, parameters in PIDS_RD/PIDS_WR). c4 is the
  module's request ID, or 0 and then c3 is the module id.
- data3 lists parameters: c1 key, c5 display name text id, c6 the DID, c7 response length, c10 bit offset, c13 the data9
  conversion (value = raw * c2 / c3 + c1) whose c5 selects the unit in data0. Its module comes from data3.c0 ->
  data16.c2 -> data16.c1 -> data8.c6 -> data8.c4 and is kept for IDs with at least 3 hits.
- data23 lists trouble codes: c0 = (SAE 2-byte code << 16) | detail. Only rows with no scope (c1 = 0) and no detail
  are used; the letter is the top two bits (0 P, 1 C, 2 B, 3 U).
"""
import collections
import os
import re
import sqlite3
import struct
import sys

MIN_HITS = 3
MIN_RATIO = 1.5
MAKE = "FORD"


class Dump:
    def __init__(self, path):
        self.db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        self._texts = {}

    def text(self, tid):
        if tid not in self._texts:
            row = self.db.execute("select text from texts where lang='en' and id=?", (tid,)).fetchone()
            self._texts[tid] = clean(row[0], self.text) if row else None
        return self._texts[tid]


def clean(s, lookup=None):
    """One line of plain text: markup expanded, UTF-8 that was decoded as Latin-1 repaired, no tabs or newlines."""
    s = re.sub(r"%%h\(([^)]*)\)\([0-9A-Fa-f]+\)", r"\1", s)
    s = s.replace("||", " ")
    try:  # "360Â°" is UTF-8 that went through cp1252 once too often
        s = s.encode("cp1252").decode("utf-8")
    except UnicodeError:
        pass
    return " ".join(s.split())


def votes(dump):
    """{module id: {request ID: definitions}} for 0x7xx request IDs."""
    out = collections.defaultdict(collections.Counter)
    for mid, rid in dump.db.execute("select c1, c4 from data8"):
        if 0x700 <= rid <= 0x7FF:
            out[mid][rid] += 1
    return out


def pick(counter):
    """The winning key of a Counter if it is clear enough, else None."""
    ranked = counter.most_common(2)
    top_key, top = ranked[0]
    runner = ranked[1][1] if len(ranked) > 1 else 0
    return top_key if top >= MIN_HITS and top >= MIN_RATIO * runner else None


def modules(dump):
    """({request ID: name}, [skipped request IDs], {module id: request ID})."""
    names = {}
    for mid, acronym, tid in dump.db.execute("select c0, c1, c2 from data12"):
        name = dump.text(tid)
        if name:
            names[mid] = (acronym, name)
    by_rid = collections.defaultdict(collections.Counter)
    for mid, counter in votes(dump).items():
        if mid in names:
            for rid, n in counter.items():
                by_rid[rid][mid] += n
    picked, skipped = {}, []
    for rid, counter in by_rid.items():
        mid = pick(counter)
        if mid is None:
            skipped.append(rid)
        else:
            picked[rid] = names[mid][1]
    module_rid = {}
    for mid, counter in votes(dump).items():
        rid = pick(counter)
        if rid is not None:
            module_rid[mid] = rid
    return picked, skipped, module_rid


# Keys of a function whose routine can be started with no input of our own. Anything else (OPTION selectors, input
# fields, scripts, PIDS_WR writes...) needs data FORScan builds in its own code, which is not in the dump.
SIMPLE_KEYS = {
    "CHECKSUM", "LABEL_ID", "NAME_ID", "ROUTINE_ID", "ROUTINE_RESULT", "SECDATA", "SESSION_DATA", "ENGINE_MODE", "PRESDATA",
    "TIME_MAX", "PROGRESS_STEP_DIV", "PROGRESS_STEP_MUL", "PIDS_RD", "PIDS_MIN", "PIDS_MAX", "ENGINE_RUN_TIME",
    "DELAY_TURNOFF", "DELAY_ROUTINE", "DELAY_AFTER", "DELAY_BEFORE", "DELAY_RESET", "DTC_RESET", "MODULE_RESET", "PCM_RESET",
}


def option_bytes(kv):
    """The routine's option record as hex: OPTION as big-endian bytes, OPTION_SIZE of them (2 when absent). Every one of the
    346 OPTION values in the dump fits this: without OPTION_SIZE they are all three hex digits (0x100-0xFFF), with it they
    are 1, 2 or 4 bytes wide. Empty if the function has none."""
    if "OPTION" not in kv:
        return ""
    size = int(kv.get("OPTION_SIZE", "2"), 16)
    value = int(kv["OPTION"], 16)
    return value.to_bytes(size, "big").hex().upper() if 0 < size <= 4 and value < 1 << (8 * size) else None


def runnable(kv):
    """True for a UDS routine whose only input is its option record, in the default or extended session."""
    keys = set(kv) - {"OPTION", "OPTION_SIZE"}
    if not keys <= SIMPLE_KEYS or not re.fullmatch(r"[0-9a-fA-F]{1,4}", kv.get("ROUTINE_ID", "")):
        return False
    return kv.get("SESSION_DATA", "0") in ("0", "3") and option_bytes(kv) is not None


def functions(dump, module_rid):
    """(request ID, title, routine, reads, writes, session, security, engine mode, runnable, option), deduplicated."""
    seen, out = set(), []
    for c3, c4, fields in dump.db.execute("select c3, c4, c6 from data21"):
        rid = c4 if 0x700 <= c4 <= 0x7FF else (module_rid.get(c3) if c4 == 0 else None)
        if rid is None:
            continue
        kv = dict(p.split("=", 1) for p in fields.split("|") if "=" in p)
        try:
            title = dump.text(int(kv["NAME_ID"], 16))
        except (KeyError, ValueError):
            continue
        if not title:
            continue
        item = (rid, title, kv.get("ROUTINE_ID", ""), kv.get("PIDS_RD", ""), kv.get("PIDS_WR", ""), kv.get("SESSION_DATA", ""),
                kv.get("SECDATA", ""), kv.get("ENGINE_MODE", ""), int(runnable(kv)), option_bytes(kv) or "")
        if item not in seen:
            seen.add(item)
            out.append(item)
    return sorted(out)


def signed16(v):
    return v - 0x10000 if v >= 0x8000 else v


def params(dump):
    """(request ID, DID, name, length, mult, div, offset, unit, bit offset, hits, kind) per readable parameter and module."""
    db = dump.db
    by_hash = collections.defaultdict(list)
    for key, group, h in db.execute("select c0, c1, c2 from data16"):
        by_hash[h].append(group)
    by_group = collections.defaultdict(list)
    for group, rid in db.execute("select c6, c4 from data8 where c4 between 1792 and 2047"):
        by_group[group].append(rid)
    scaling = {r[0]: r for r in db.execute("select c0, c1, c2, c3, c5 from data9")}
    units = {r[0]: dump.text(r[1]) for r in db.execute("select c0, c6 from data0")}
    out = {}
    for h, did, length, bit, nameid, blob in db.execute("select c0, c6, c7, c10, c5, c13 from data3 where c3 = 0"):
        if not (0 < did <= 0xFFFF and 0 < length <= 8 and len(blob) >= 2):
            continue
        name = dump.text(nameid)
        sid = struct.unpack("<H", blob[:2])[0]
        if not name or sid not in scaling:
            continue
        hits = collections.Counter(rid for g in by_hash.get(h, []) for rid in by_group.get(g, []))
        _, offset, mult, div, utype = scaling[sid]
        if div == 0:
            continue
        for rid, n in hits.most_common(3):
            if n >= MIN_HITS:
                key = (rid, did, name)
                if key not in out or n > out[key][-1]:
                    out[key] = (rid, did, name, length, mult, div, signed16(offset), units.get(utype) or "", bit, n)
    rows = sorted(out.values(), key=lambda r: (r[0], r[2].lower(), r[1]))
    offsets = collections.defaultdict(set)
    for r in rows:
        offsets[(r[0], r[1])].add(r[8])
    # a DID whose items sit at different bit offsets is a bit field; the same name in other ECU variants is not
    return [r + ("bits" if len(offsets[(r[0], r[1])]) > 1 else "value",) for r in rows]


def dtcs(dump):
    """{code: description} for unscoped rows whose code has no detail bytes."""
    out = {}
    for c0, tid in dump.db.execute("select c0, c2 from data23 where c0 >= 65536 and c1 = 0 and (c0 & 65535) = 0 order by c0, c3"):
        number = c0 >> 16
        code = "PCBU"[number >> 14] + f"{number & 0x3FFF:04X}"
        text = dump.text(tid)
        if text and code not in out:
            out[code] = text
    return out


def data_dir():
    if os.environ.get("CANFORGE_DATA_DIR"):
        return os.environ["CANFORGE_DATA_DIR"]
    base = os.environ.get("XDG_DATA_HOME") or os.path.expanduser("~/.local/share")
    return os.path.join(base, "canforge")


def write(path, header, rows):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(header + "\n")
        for row in rows:
            f.write("\t".join(str(x) for x in row) + "\n")
    print(f"wrote {path} ({len(rows)} rows)")


def main(argv):
    args = [a for a in argv if not a.startswith("--")]
    if len(args) != 1:
        sys.exit(__doc__)
    dump = Dump(args[0])
    names, skipped, module_rid = modules(dump)
    funcs = functions(dump, module_rid)
    plist = params(dump)
    codes = dtcs(dump)
    print(f"{len(names)} request IDs named, {len(skipped)} skipped as too thin or ambiguous")
    print(f"{len(funcs)} functions on {len({f[0] for f in funcs})} modules")
    print(f"{len(plist)} readable parameters on {len({p[0] for p in plist})} modules")
    print(f"{len(codes)} trouble code descriptions")
    for rid in (0x7E0, 0x760, 0x720):
        print(f"  {rid:03X}  {names.get(rid, '(skipped)')}")
    if "--write" not in argv:
        print("dry run: nothing written (add --write)")
        return
    root = data_dir()
    note = "# local FORScan import, not for redistribution"
    write(os.path.join(root, "modules.d", "forscan.tsv"), note, [(f"{rid:03X}", MAKE, names[rid]) for rid in sorted(names)])
    write(os.path.join(root, "forscan", "functions.tsv"), note + "\n# REQUEST_ID\tTITLE\tROUTINE_ID\tREADS\tWRITES\tSESSION\tSECURITY\tENGINE_MODE\tRUNNABLE\tOPTION_BYTES",
          [(f"{f[0]:03X}",) + f[1:] for f in funcs])
    write(os.path.join(root, "forscan", "params.tsv"), note + "\n# REQUEST_ID\tDID\tNAME\tLENGTH\tMULT\tDIV\tOFFSET\tUNIT\tBIT_OFFSET\tHITS\tKIND(value|bits)",
          [(f"{r:03X}", f"{d:04X}", n, ln, m, dv, o, u, b, h, k) for r, d, n, ln, m, dv, o, u, b, h, k in plist])
    write(os.path.join(root, "dtc.d", "forscan.tsv"), note, [(code, MAKE, codes[code]) for code in sorted(codes)])


if __name__ == "__main__":
    main(sys.argv[1:])
