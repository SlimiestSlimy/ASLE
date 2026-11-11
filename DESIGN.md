# ASLE — Arbitrary Symbol Length Encoding

Compresses a bit stream by building a dictionary of symbols of any length up
to n bits, then encoding the stream of placed symbols with a prefix code.

**Goal:** the smallest dictionary (fewest leaves) whose symbols still encode
the whole stream; the encoded symbol bits break ties.

The six design statements (epistemology, change, irreversibility, tradeoffs,
boundaries, links) apply to every decision here, down to names and module
boundaries.

---

## Glossary

| Term | Meaning |
|---|---|
| bit stream | the input, read as bits, most significant bit of each byte first |
| n, maximum symbol bit length | the largest window size, so the longest possible symbol; the only parameter; no built-in limit |
| window size, L | the exact bit length being merged into at the current stage, 2..=n |
| shared bit sequence | one packed sequence of bits that every symbol points into: the input bits (plus one `0` and one `1`) in the encoder, the decoded symbol bits in the decoder |
| symbol definition | a slice of the shared bit sequence: (first bit offset, bit length); equal bits = same symbol |
| placed symbol | one position in the stream's current parse; the parse always spells the input exactly |
| frequency | how many times a symbol is placed |
| leaf | a symbol with frequency > 0 |
| encoded symbol bits | sum over leaves of frequency × Huffman code length = exact bits of a Huffman-encoded stream; what the builder decides with |
| header | everything in the container before the body; **measured, never used to decide** |
| merge candidate | a bit string of exactly L bits filled by 2+ adjacent placed symbols, nothing poking out |
| occurrence | one place a candidate's window is filled exactly |
| merge | replacing a candidate's occurrences with one symbol |
| pass | find candidates once, then run rounds until a round commits nothing |
| round | predict every remaining candidate's merge, rank them, commit the best passing one |
| prefix tree | the binary tree of all leaves' bits; a node is a symbol if its path is one |
| depth-first order | bit by bit, 0 before 1, a prefix before its extensions; every tree here is written and read in this order |
| skipped full levels | top levels of the prefix tree where every node has both children; their child bits are implied |
| combination index | one number in [0, C(m, k)) saying which k of m nodes are symbols |
| order-preserving code tree | the optimal binary code tree whose leaves, left to right, are the symbols in depth-first order (an optimal alphabetic code) |

---

## Current rules, with the reason for each

1. **Start state.** Every input bit is a placed `0` or `1`. Window sizes start at 2,
   because 1-bit symbols never get a Huffman code shorter than 1 bit.
2. **Candidates.** Exactly fill the window with 2+ placed symbols; any mix of raw bits
   and earlier symbols. A placed symbol is never split.
3. **Occurrences.** The most non-overlapping occurrences, taken left to right. All
   occurrences have the same bit length, so this gives the maximum count.
4. **A merge must repeat** (2+ occurrences): a single occurrence is not a pattern.
5. **Shorter-code rule.** A merge commits only if its code bits (Huffman tree after the
   merge) are fewer than the code bits of the placements it replaces (tree before).
   *Why:* "total encoded symbol bits must drop" committed nothing at all on text,
   because the first new leaf pushes `0` or `1` a level deeper.
6. **The header never enters a decision.** It is measured and logged only.
7. **Ranking.** score = frequency × (1 + (bits_before − bits_after) / bits_before), bits =
   encoded symbol bits; exact integer comparison. Ties: higher frequency, then smaller
   bit value. Scores are recomputed after every commit, since the tree changes.
8. **No deletions.** Deleting parts of a merge (with respelling and revival) was tried in
   several forms and always made the dictionary and the output larger; it is removed
   (see history; the code is in git).
9. **Stopping.** A window size ends when a pass commits nothing. Every merge strictly
   reduces the number of placed symbols, so building always terminates.

---

## Container format

```
"ASLE" magic                                       32 bits
Elias gamma(input bit count + 1)                   (nothing more for empty input)
symbol prefix tree, depth first:
    skipped full levels                            4 bits (0..15)
    child bits: 2 per node at or below them        (has 0-child, has 1-child)
    Elias gamma(k + 1)                             k = non-root nodes with children that are symbols
    combination index                              exactly enough bits for C(m, k) - 1,
                                                   m = non-root nodes with children (depth first)
    (nodes without children are always symbols; the root never is)
code tree shape, depth first                       1 = internal, 0 = leaf; 2·leaves − 1 bits
    leaves, left to right = symbols in depth-first order
body: the code of every placed symbol
```

The container uses the optimal order-preserving code tree (Garsia–Wachs). Its body is
never shorter than Huffman's, but it needs no table linking codes to symbols. The
builder still decides with Huffman code lengths. Verified: the Garsia–Wachs body equals
an independent O(L²) dynamic-programming optimum on every test file.

---

## Measured history (bytes)

### Builder versions (n = 16, previous container)

| Version | text 7,346 | binary 16,384 | packed 5-bit 10,000 | Rust source 16,384 | random 8,192 |
|---|---|---|---|---|---|
| total size (header included) must drop | 7,357 | — | 5,054 | — | — |
| shorter-code rule | 6,656 | 8,104 | 6,816 | 13,683 | 15,811 |
| + size-table header | 6,557 | 7,980 | 6,775 | 13,548 | 15,420 |
| + must repeat | 6,074 | 7,187 | 6,650 | 12,998 | 11,564 |
| + encoded-symbol-bits modifier on every merge | **6,063** | **7,168** | **6,649** | **12,993** | **11,547** |
| + deletion of all parts, encoded symbol bits must drop | 6,063 | 7,168 | 6,649 | 12,993 | 11,547 |
| + leaf-removing options ×2, deletion even if bits rise | 7,035 | 8,544 | 8,478 | 15,582 | 13,524 |
| + removed/added ratio instead of ×2 | 7,043 | 8,947 | 8,479 | 14,895 | 13,626 |
| + every subset of parts | 7,280 | 8,502 | 9,274 | 15,796 | 12,373 |

The bold row is the current builder. The first row was not run on every file.

### Current builder, new container, by n

| n | text | binary | packed 5-bit | Rust source | random |
|---|---|---|---|---|---|
| 8 | 6,725 | 8,278 | 8,220 | 15,284 | **9,910** |
| 12 | 6,111 | 8,139 | 6,998 | 13,634 | 10,014 |
| 16 | 5,571 | 6,966 | 6,383 | 12,241 | 10,139 |
| 24 | 5,389 | **6,919** | **6,376** | 11,301 | 10,161 |
| 32 | **5,316** | 7,005 | 6,651 | **11,025** | 10,161 |
| 64 | 5,544 | 7,325 | 6,909 | 11,416 | 10,161 |
| 128 | 5,928 | 8,008 | 6,909 | 12,521 | 10,161 |
| 256 | 6,121 | 8,266 | 6,909 | 13,016 | 10,161 |
| gzip -9 | 2,602 | 3,512 | 6,318 | 5,036 | 8,215 |
| xz -9 | 2,644 | 2,636 | 6,340 | 4,924 | 8,252 |

At n = 16 the new container alone saved 3–12% (text 6,063 → 5,571). Building takes
under 7 s per file at any n here.

**Test data caveat:** "text" and "Rust source" are both Rust code (early versions of
this crate's own source), not prose.

## Observations from measurement

- As n grows, the body shrinks steadily and the header grows: long symbols copy the
  input into the dictionary. Totals bottom out at n = 24–32 on structured data.
  The builder never sees the header, so nothing stops it from adding long, rarely
  used symbols.
- Random data: 1,283 leaves of pure chance repeats; the best total is at the smallest n.
- With deletions gated on the encoded symbol bits, no deletion ever lowered them.
  With deletions allowed to raise them, almost every deletion removed 2 leaves and
  added 1, while plain merges added far more leaves.
- Binary: the order-preserving body costs 3,241 bits more than Huffman's, more than
  the header saves against writing code lengths (+2,074 bits for order). On the other
  four files the order-preserving tree wins.

## Open questions

- What should stop the builder from adding symbols the header has to pay for, without
  measuring the header?
- Binary data prefers code lengths over order; a one-bit choice per file would be
  exact.
- Garsia–Wachs here is O(L²) time in the leaf count; fine so far, a concern for much
  larger dictionaries.

---

## Code layout (dependencies point down the list)

| Module | Job |
|---|---|
| `main` | command line only |
| `json_lines_log` | one observer: writes every event as JSON Lines |
| `container_format` | the file layout; the only module that knows the header |
| `symbol_prefix_tree` | describes, measures, writes and reads the symbol set |
| `order_preserving_code_tree` | optimal code tree keeping the symbols' order (Garsia–Wachs) |
| `code_tree` | binary code tree: codes, write/read depth first, decode |
| `big_unsigned_integer` | big integers for the combination index |
| `symbol_builder` | decision loop: window sizes, passes, rounds, commit rules |
| `builder_events` | event records and the observer interface |
| `option_ranking` | scores and ordering |
| `change_prediction`, `merge_candidates` | analysis: read-only predictions |
| `symbol_stream` | the parse state; the only code that changes it |
| `predicted_changes` | plain data for predictions |
| `huffman_tree` | Huffman code lengths and encoded symbol bits for decisions |
| `packed_bit_sequence`, `symbol_definition`, `bit_io`, `fast_hash` | building blocks |

## Running

```
asle compress   <input> <output> --max-symbol-bits <n> [--verbose] [--log <file.jsonl>]
asle decompress <input> <output>
asle benchmark  --max-symbol-bits <n> [--verbose] <files...>
python3 tools/plot_encoder_log.py <file.jsonl> <chart.html>
```
