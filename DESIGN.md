# ASLE — Arbitrary Symbol Length Encoding

Compresses a bit stream by building a dictionary of symbols of any length up
to n bits, then encoding the stream of placed symbols with a prefix code whose
bits are range coded.

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
| bit length / code length | bits of input a symbol stands for (from the prefix tree) / bits its Huffman code takes in the body (from the code length section) |
| multiset-permutation index | one number saying which code length each symbol has, given how many symbols have each code length |
| code tree node | a place in the canonical Huffman code tree: (depth, code bits on the way to it); each node is one binary choice in the body |
| order-3 context | the depth-first positions of the last 3 placed symbols |
| last choice | the side (0 or 1) last taken at a (context, code tree node); one bit of memory, the same in encoder and decoder |

---

## Current rules, with the reason for each

1. **Start state.** Every input bit is a placed `0` or `1`. Window sizes start at 2,
   because 1-bit symbols never get a Huffman code shorter than 1 bit.
2. **Candidates.** Exactly fill the window with 2+ placed symbols; any mix of raw bits
   and earlier symbols. A placed symbol is never split.
3. **Occurrences.** The most non-overlapping occurrences, taken left to right. All
   occurrences have the same bit length, so this gives the maximum count.
4. **A merge must repeat** (2+ occurrences): a single occurrence is not a pattern.
5. **Judged on the whole.** A change (merge or leaf removal) commits when it strictly
   lowers the total encoded symbol bits (sum over all leaves of frequency × Huffman code
   length). The header never enters a decision.
6. **Best point.** Every window size commits at least one merge: if a window size
   reaches a round where no merge lowers the total and it has not committed a merge yet,
   its best-ranked merge is committed anyway. *Why:* from the start state no single merge
   can lower the total on text-like data (the first new leaf pushes `0` or `1` a level
   deeper), so judging on the whole alone committed nothing on the Rust files.
7. **Leaf removal first.** Each round, before merges: leaves whose code is longer than
   their own bits (break-even leaves stay: they are building blocks) are checked, biggest
   loss first (frequency × (code length − bit length)); the first whose respelling with
   other live leaves (fewest code bits, exact) lowers the total is removed.
8. **Ranking.** score = frequency × (1 + (bits_before − bits_after) / bits_before), bits =
   encoded symbol bits; exact integer comparison. Ties: higher frequency, then smaller
   bit value. Scores are recomputed after every change, since the tree changes.
9. **Stopping.** A window size ends when a pass commits nothing, or when an exact state
   (all frequencies) repeats (a removal puts placements back, so merges and removals
   could undo each other).

## Container format

```
Elias gamma(placed symbol count + 1)              (nothing more for empty input)
symbol prefix tree, depth first:
    skipped full levels                            4 bits (0..15)
    child bits: 2 per node at or below them        (has 0-child, has 1-child)
    Elias gamma(k + 1)                             k = non-root nodes with children that are symbols
    combination index                              exactly enough bits for C(m, k) - 1,
                                                   m = non-root nodes with children (depth first)
    (nodes without children are always symbols; the root never is)
code length section (nothing for a lone symbol: its code is 0, 1 bit):
    for code length 1, 2, 3, ...: how many symbols have it, in exactly enough bits for
        [max(0, 2·slots − remaining), min(remaining, slots)], until no symbol remains
        (slots: free places at that depth of the full canonical code tree, 2 at length 1,
        then twice the places not taken by leaves; remaining: symbols not yet placed)
    multiset-permutation index                     the code lengths in depth-first symbol order,
                                                   ranked among all orderings with those counts;
                                                   exactly enough bits for (orderings − 1)
body: the canonical Huffman code of every placed symbol, range coded bit by bit
    down the code tree (below), to the end of the stream
```

No magic number and no input length: the decoder stops after the placed symbol count, and
the output length is the sum of the decoded symbols' lengths. ASLE takes an exact bit
count; a file's last byte is padded with zeros when written, outside the format.
Measured against the previous section (gamma(input bits + 1), gamma(longest code length),
gamma(count + 1) per length): −40 to −48 bits per test file.

Two different lengths: a symbol's **bit length** (bits of input it stands for) comes from
the prefix tree; its **code length** (bits its code takes in the body) depends on its
frequency and comes from the code length section.

Canonical codes are handed out by (code length, depth-first position), a rule rather than
stored bits. The code lengths are exactly the builder's Huffman lengths, so before range
coding the body is exactly the encoded symbol bits the builder measured. Symbols with equal code
lengths are interchangeable, so the multiset permutation is exactly the information needed.

Alternatives measured (early builder source / binary, bits to link symbols to codes):
counts + multiset permutation 1,322 / 1,224 (chosen); Huffman tree shape + multiset
permutation 2,386 / 2,293; tree shape + full permutation of symbols onto leaves 5,631 / 5,687.
A full permutation also records the order among same-length codes, which changes nothing.
*Replaced:* an order-preserving code tree (Garsia–Wachs), whose body was 0.6–7% longer than
Huffman's; the multiset permutation won on every test file (binary −3,182 bits).

### Range-coded body (the user's design, 2026-11-10)

Each placed symbol's canonical code is coded one bit at a time, each bit being the choice
at one code tree node. The prediction for a node comes from the **last choice** taken at
the same (order-3 context, code tree node): the remembered side gets 3/4, the other 1/4;
a pair never seen gets 1/2 (the Huffman tree's own split). After each bit the last choice
is overwritten. Encoder and decoder hold the same memory, so nothing is stored for it.
Before the first 3 symbols a start marker stands in for the missing positions (MINE).

The coder is an LZMA-style binary range coder, integers only: a 64-bit low end, a 32-bit
range, carries through a cached byte. A probability is a u32 fraction of 2^32; the split
is (range × probability) >> 32 in 64 bits. Its first output byte, always 0, is not
written (MINE); the flush writes 4 more bytes, so the body ends on a byte and tiny inputs
pay about 32 bits (5 input bits: a 7-byte stream). The decoder reads zeros past the end.

---

## Measured history (bytes)

### Range-coded body (n = 16; whole stream in **bits**, lower is better; random is a noise reference, not counted)

Every column is the whole stream (header + body), each decoded back exactly. The counter
rows were measured on the way and not kept: one counter per node, its value c in
0..2^w − 1 predicting a 1 with chance (c + 1) / (2^w + 1), starting at 2^(w−1) − 1;
"±1" steps one toward each bit, "+2/−1" steps 2 on a right prediction, 1 back on a wrong one.

| Body coding | binary | packed 5-bit | Rust source | early builder | **total (4)** | *random* |
|---|---|---|---|---|---|---|
| canonical Huffman codes | 51,055 | 42,477 | 97,260 | 44,200 | 234,992 | *79,367* |
| **last choice 3/4, order 3 (current)** | **46,055** | **43,199** | **79,117** | **39,125** | **207,496** | *83,311* |
| last choice 2/3, order 3 | 46,983 | 42,263 | 84,061 | 40,485 | 213,792 | *80,919* |
| 3-bit counters ±1, order 3, one per pair | 45,535 | 42,879 | 80,917 | 39,749 | 209,080 | *83,663* |
| 2-bit counters ±1 (same as +2/−1), order 3 | 46,327 | 43,559 | 80,957 | 39,901 | 210,744 | *84,447* |
| 3-bit counters +2/−1, order 3 | 45,959 | 43,823 | 78,677 | 39,061 | 207,520 | *86,215* |
| 4-bit counters +2/−1, order 3 | 46,951 | 44,031 | 81,237 | 39,797 | 212,016 | *88,127* |
| the same, hashed into 2^16 counters (splitmix64) | 47,447 | 44,535 | 84,485 | 40,309 | 216,776 | *89,127* |
| the same, 2^20 counters | 46,975 | 44,087 | 81,461 | 39,845 | 212,368 | *88,199* |
| the same, order 2 | 48,167 | 45,927 | 76,949 | 38,365 | 209,408 | *93,839* |
| order 2, 2^20 counters, ±1 | 46,727 | 42,967 | 80,277 | 39,365 | 209,336 | *86,295* |

Text (the two Rust files) wants predictions that commit fast; the binary, the packed fields
and noise want predictions that stay near 1/2 when the bits disagree. Most (context, node)
pairs are seen about twice (Rust source: 88,967 node visits over 50,624 pairs), which is
why a 2^16 table collides (30% of the pairs on the Rust source) and wide counters stay
near 1/2.

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

### Decision rules with the canonical container (n = 16; random is a noise reference, not counted)

| Merge rule / removal | packed 5-bit | binary | Rust source | early builder | **total (4)** | *random* |
|---|---|---|---|---|---|---|
| per-symbol, no removal | 6,356 | 6,569 | 12,124 | 5,523 | 30,572 | *10,091* |
| per-symbol, remove if code ≥ bits, always | 6,256 | 7,247 | 13,213 | 6,150 | 32,866 | *10,547* |
| per-symbol, remove if code > bits, always | 6,033 | 6,936 | 13,163 | 6,282 | 32,414 | *10,455* |
| whole total, whole-total removals | 5,015 | 6,387 | 16,394 (stuck) | 7,356 (stuck) | 35,152 | *8,202* |
| per-symbol merges, whole-total removals | 6,256 | 6,486 | 11,812 | 5,550 | 30,104 | *10,055* |
| whole total, explore + roll back to lowest total | 5,768 | 6,620 | 11,421 | 5,525 | 29,334 | *8,202* |
| **whole total + one forced merge per window size (current)** | **5,319** | **6,391** | **12,168** | **5,535** | **29,413** | *9,930* |

### Builder before the decision-rule changes, by n (previous containers)

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

At n = 16 the prefix-tree container saved 3–12% (text 6,063 → 5,571). Building takes
under 7 s per file at any n here. These rows predate the multiset-permutation code length
section, which then saved another 27–397 bytes per file at n = 16:

| n = 16 | text | binary | packed 5-bit | Rust source | random |
|---|---|---|---|---|---|
| order-preserving code tree | 5,571 | 6,966 | 6,383 | 12,241 | 10,139 |
| **canonical Huffman + multiset permutation (current)** | **5,523** | **6,569** | **6,356** | **12,124** | **10,091** |

**Test data:** "text" and "Rust source" are both Rust code (early versions of this crate's
own source); the files are now named for what they are. Real prose (the 643 KB vim user
manual) is in the test set for larger runs.

## Observations from measurement

- As n grows, the body shrinks steadily and the header grows: long symbols copy the
  input into the dictionary. Totals bottom out at n = 24–32 on structured data.
  The builder never sees the header, so nothing stops it from adding long, rarely
  used symbols.
- Random data: 1,283 leaves of pure chance repeats; the best total is at the smallest n.
- With deletions gated on the encoded symbol bits, no deletion ever lowered them.
  With deletions allowed to raise them, almost every deletion removed 2 leaves and
  added 1, while plain merges added far more leaves.
- Final symbol frequencies (n = 16): leaves used only 2–3 times are 40–60% of every
  dictionary but cover 3–12% of the input on structured data (31% on random). A handful
  of symbols cover most of the input (binary: 4 leaves cover 71%).
- A third to a half of the leaves have a code as long as or longer than their own bits,
  so they save nothing in the body; they are the rarely used ones.
- Larger n lowers the body because longer symbols mean fewer placements (early builder
  source: 9,115 placements at n = 8, 2,067 at n = 256) while the average code length
  rises only from 5.8 to 9.6 bits. Past n = 32 the leaf count barely changes; the header
  grows because leaves get longer.

## Open questions

- What should stop the builder from adding symbols the header has to pay for, without
  measuring the header?
- Building time grows quickly with input size: every round re-predicts every candidate
  with a full Huffman rebuild (the 643 KB vim manual took ~2.5 min per window size by 17).

---

## Code layout (dependencies point down the list)

| Module | Job |
|---|---|
| `main` | command line only |
| `json_lines_log` | one observer: writes every event as JSON Lines |
| `container_format` | the file layout; the only module that knows the header |
| `symbol_prefix_tree` | describes, measures, writes and reads the symbol set |
| `code_length_section` | writes and reads which code length each symbol has |
| `canonical_huffman_code` | canonical codes from code lengths, and which symbol a code is |
| `range_coded_body` | the body: code tree choices predicted by the last choice per (context, node) |
| `binary_range_coder` | integer binary range coder, u32 probabilities |
| `multiset_permutation` | exact ranking and unranking of multiset permutations |
| `big_unsigned_integer` | big integers for the combination and permutation indexes |
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
