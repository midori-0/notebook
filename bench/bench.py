#!/usr/bin/env python3
"""Benchmarks nb on generated notebooks and writes bench/results.json.

    python3 bench/bench.py [path/to/nb]     (default: target/release/nb)

For each size it measures the median wall time and peak memory of common commands, and how
many bytes an agent reads to answer "what do my notes say about <topic>?" two ways: with nb
(dictionary, matching headers, three notes) and by reading the same notes kept as one markdown
file. Needs Python 3.8+ and nothing else. Wall times include starting the process, as an agent
would see them.
"""
import json, os, platform, random, shutil, statistics, subprocess, sys, tempfile, time

SIZES = [100, 1_000, 10_000, 100_000]
TOPICS = 40
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORDS = ("cache index token latency retry schema migration request handler parser queue lock "
         "session worker config build release test fixture deploy rollback metric trace span "
         "budget limit timeout socket buffer stream batch shard replica leader vote commit "
         "branch merge review module crate service client server route header payload").split()


def sentence(rng, lo, hi):
    return " ".join(rng.choice(WORDS) for _ in range(rng.randint(lo, hi))).capitalize()


def generate(n, seed=7):
    """`n` notes as (number, timestamp, ids, title, body), plus the topic dictionary."""
    rng = random.Random(seed)
    topics = [(f"topic{i:02d}", sentence(rng, 2, 5)) for i in range(TOPICS)]
    notes = []
    for k in range(1, n + 1):
        ids = sorted(rng.sample([t for t, _ in topics], rng.choice([1, 1, 2, 2, 3])))
        day, sec = divmod(k * 86400 * 300 // n, 86400)
        ts = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(1767225600 + day * 86400 + sec))
        lines = []
        for _ in range(rng.randint(2, 8)):
            line = sentence(rng, 6, 14) + "."
            lines.append(f"- {line}" if rng.random() < 0.3 else line)
        notes.append((k, ts, ids, sentence(rng, 3, 7), "\n".join(lines)))
    return topics, notes


def write_notebook(path, topics, notes):
    with open(path, "w") as f:
        f.write("notebook v1\nuid bench.nb-0123456789abcdef\n")
        f.writelines(f"id {t}: {m}\n" for t, m in topics)
        f.write("\n")
        for k, ts, ids, title, body in notes:
            f.write(f"@@ {k} {ts} [{','.join(ids)}] {title}\n{body}\n\n")


def write_markdown(path, notes):
    """The same notes as the markdown file an agent would otherwise keep."""
    with open(path, "w") as f:
        f.write("# Notes\n\n")
        for k, ts, ids, title, body in notes:
            f.write(f"## {title}\n\n*{', '.join(ids)} · {ts}*\n\n{body}\n\n")


def run(cmd, env, capture=False):
    """Runs `cmd`; returns (milliseconds, peak RSS in MB, stdout)."""
    start = time.perf_counter()
    p = subprocess.Popen(cmd, env=env, stdin=subprocess.DEVNULL, stderr=subprocess.PIPE,
                         stdout=subprocess.PIPE if capture else subprocess.DEVNULL)
    out = p.stdout.read() if capture else b""
    _, status, usage = os.wait4(p.pid, 0)
    ms = (time.perf_counter() - start) * 1000
    if status != 0:
        sys.exit(f"{' '.join(cmd)} failed: {p.stderr.read().decode()}")
    rss = usage.ru_maxrss / (1 << 20 if sys.platform == "darwin" else 1 << 10)
    return ms, rss, out


def bench_size(nb, n, work):
    topics, notes = generate(n)
    fresh, path, md = (os.path.join(work, f) for f in ("fresh.nb", "b.nb", "NOTES.md"))
    write_notebook(fresh, topics, notes)
    write_markdown(md, notes)
    env = dict(os.environ, XDG_CACHE_HOME=os.path.join(work, "cache"))
    mid = str(n // 2)
    commands = {
        "ids": (["ids", path], False),
        "list": (["list", path], False),
        "list -i": (["list", path, "-i", "topic03"], False),
        "show -n": (["show", path, "-n", mid], False),
        "add": (["add", path, "-i", "topic01", "-t", "New note", "--text", "body"], True),
        "edit": (["edit", path, mid, "-t", "Edited title"], True),
        "rm": (["rm", path, mid], True),
    }
    reps = 15 if n >= 100_000 else 30
    results = {}
    for name, (args, writes) in commands.items():
        times, peak = [], 0.0
        shutil.copy(fresh, path)
        for _ in range(reps):
            if writes:
                shutil.copy(fresh, path)
                shutil.rmtree(env["XDG_CACHE_HOME"], ignore_errors=True)
            ms, rss, _ = run([nb] + args, env)
            times.append(ms)
            peak = max(peak, rss)
        results[name] = {"ms": round(statistics.median(times), 2), "rss_mb": round(peak, 2)}

    # One lookup per topic: the dictionary, the topic's headers, then three of its notes.
    shutil.copy(fresh, path)
    ids_bytes = len(run([nb, "ids", path], env, capture=True)[2])
    per_topic = []
    for topic, _ in topics:
        headers = run([nb, "list", path, "-i", topic], env, capture=True)[2]
        picks = [h.split(b" ", 1)[0].decode() for h in headers.splitlines()[:3]]
        shown = run([nb, "show", path] + [a for k in picks for a in ("-n", k)], env, capture=True)[2]
        per_topic.append(ids_bytes + len(headers) + len(shown))
    lookup = {"nb_bytes": round(statistics.mean(per_topic)), "markdown_bytes": os.path.getsize(md)}
    print(f"{n:>7} notes  {os.path.getsize(fresh) / 1e6:6.2f} MB  "
          + "  ".join(f"{k} {v['ms']:.1f}ms" for k, v in results.items()), flush=True)
    return {"notes": n, "bytes": os.path.getsize(fresh), "commands": results, "lookup": lookup}


def main():
    nb = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "target/release/nb"))
    version = subprocess.run([nb, "--version"], capture_output=True, text=True).stdout.strip()
    work = tempfile.mkdtemp(prefix="nb-bench-")
    try:
        sizes = [bench_size(nb, n, work) for n in SIZES]
    finally:
        shutil.rmtree(work, ignore_errors=True)
    cpu = platform.processor() or platform.machine()
    if sys.platform == "darwin":
        cpu = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True).stdout.strip() or cpu
    result = {
        "nb": version,
        "machine": f"{cpu}, " + (f"macOS {platform.mac_ver()[0]}" if sys.platform == "darwin" else f"{platform.system()} {platform.release()}"),
        "date": time.strftime("%Y-%m-%d"),
        "sizes": sizes,
    }
    out = os.path.join(ROOT, "bench", "results.json")
    with open(out, "w") as f:
        json.dump(result, f, indent=2)
        f.write("\n")
    print(f"wrote {os.path.relpath(out, ROOT)}")


if __name__ == "__main__":
    main()
