# Benchmarks

How fast Nebula downloads and post-processes compared with SABnzbd and NZBGet, measured with `dev/bench.py` against a local NNTP server. On a local server the network isn't the limit, so this measures the client itself: how much CPU it needs per MB/s, how it writes to disk and how long verification, repair and extraction take.

## The test

- **Case**: 1 GB of random data (a stand-in for a video that doesn't compress) packed with 7-Zip into 50 MB volumes, with 5 % PAR2 recovery files: 25 files and 1051 MB in 1386 articles of 750 KB.
- **Damage**: 6 data articles return `430 No such article`, so every client has to fetch extra PAR2 blocks, repair and then extract.
- **Server**: `dev/nntp-bench`, which serves the articles already yEnc-encoded from memory with no speed limit, on `127.0.0.1`.
- **Each client** downloads the case three times with 20 connections, one client at a time, starting empty. The table shows the run with the median total time.
- **Machine**: a 4-core cloud VM (Intel Xeon @ 2.10 GHz, 16 GB RAM, ext4 on a virtual disk), Ubuntu 24.04.

| Column | What it is |
|---|---|
| Average | MB the server sent divided by the seconds it was sending (pauses longer than half a second don't count, such as the verification before asking for the missing PAR2 blocks). |
| Peak | The best half-second or more. |
| Download | Seconds the server was sending. |
| Total | From adding the NZB to the extracted file being in place. |
| CPU | CPU seconds of the client and everything it launched (par2, 7z), and the average in % of one core. |
| RAM | The most memory the client and its children used at the same time. |

## Results

| Client | Average | Peak | Download | Total | Post-processing | CPU | RAM | Result |
|---|---|---|---|---|---|---|---|---|
| **Nebula 1.10.1** (before this work) | 255 MB/s | 314 MB/s | 4.1 s | 21.7 s | 14.9 s | 45.0 s (208 %) | 49 MB | correct |
| **Nebula** (now) | 1025 MB/s | 2014 MB/s | 1.0 s | 15.0 s | 11.3 s | 30.3 s (202 %) | 87 MB | correct |
| SABnzbd 4.2.2 | 1338 MB/s | 1563 MB/s | 0.8 s | 12.9 s | 11.5 s | 22.6 s (176 %) | 153 MB | correct |
| NZBGet 21.0 | 1186 MB/s | 1749 MB/s | 0.9 s | — | — | 1.5 s (97 %) | 147 MB | download only* |

\* NZBGet 21.0 from Ubuntu 24.04 crashes (`buffer overflow detected`) as soon as it starts post-processing, so the script only measures its download (`ParCheck=manual`, `Unpack=no`). The current NZBGet (v25, nzbget.com) doesn't have that package's problem; `dev/bench.py` uses whatever `nzbget` is in the `PATH`.

What it shows:

- At 1 GB/s on a local server, all three are far above what a home connection or a Usenet provider delivers (1 Gbit/s is 125 MB/s). On a real connection the download time is the same for all three; what changes is how much CPU it costs.
- Nebula uses the least memory of the three.
- In total time, Nebula is 2 seconds behind SABnzbd. Both spend most of it in par2 (verify, then repair) and 7-Zip. SABnzbd skips part of the first verification because it calculates the checksums while downloading; that's left for later (below).
- Times on a local disk vary between runs by ±20 % (the page cache and when the kernel decides to write it out), so take small differences as noise.

## What was optimized

Profiling Nebula with `perf` during the download (flame graph of `nebula get` against `nntp-bench`) showed two clear problems:

1. **Disk writes** (about 35 % of the CPU): each connection wrote its own article with a blocking task and waited. With 20 connections writing to the same file, the kernel serializes them on the file's lock and the CPU spun waiting for it. Now a single thread writes the articles in the order they arrive and marks them as done, and the connection goes on with the next article using a recycled buffer (`crates/nebula-core/src/disk.rs`). The queue is limited to 32 articles (about 25 MB), so a slow disk slows the download instead of filling memory. That's where the extra 40 MB of RAM comes from.
2. **yEnc decoding** (about 32 %): the loop went byte by byte. It now looks for the escape character with `memchr` and decodes the stretches in between in one go, which the compiler vectorizes: from 758 to 1036 MB/s on one core (`cargo test -p nebula-core --release yenc_speed -- --ignored --nocapture`).

The download went from 255 MB/s to over 1 GB/s on the same machine.

A third change shortens the repair: after fetching the extra PAR2 blocks, Nebula used to run `par2 v` (read and check everything) and then `par2 r` (which checks everything again before repairing). It now goes straight to `par2 r`. In this case that saves about 3 seconds per GB.

How connections are shared out didn't show up in the profile: with 20 connections all of them are busy until the end of the download.

## Running it

On Linux, from the root of the repository:

```bash
sudo apt install par2 7zip            # needed
sudo apt install sabnzbdplus nzbget   # optional, to compare
python3 dev/bench.py                  # 1 GB, 20 connections, Nebula + SABnzbd + NZBGet
python3 dev/bench.py --size-mb 4000 --conns 50 --runs 3 --json results.json
python3 dev/bench.py --clients nebula --missing 0
```

The script builds `nebula-cli` and `nntp-bench` in release, creates the case in `--work` (by default `/tmp/nebula-bench`, reused between runs of the same size), starts the server, runs each client with its own clean configuration and checks that the extracted file has the original MD5. Clients that aren't installed are skipped. It needs about three times `--size-mb` of free disk space and the same amount of RAM for the server.

`dev/nntp-bench` also works on its own as a fast server for testing by hand:

```bash
cargo run --release -p nntp-bench -- --src folder --nzb test.nzb --port 1119 --max-conns 100 --missing 3
nebula get test.nzb -s "nntp://test:test@127.0.0.1:1119?conns=30"
```

## Pending

- Calculating the PAR2 checksums while downloading, as SABnzbd does, so that a complete download doesn't need `par2 v` to read everything again. In this case that's another 3 seconds per GB, and much more on a slow NAS disk.
- A SIMD yEnc decoder (SSE2/AVX2/NEON) for low-power ARM NAS boxes, where decoding weighs more than on a desktop CPU.
