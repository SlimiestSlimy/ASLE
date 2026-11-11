# ASLE — Arbitrary Symbol Length Encoding

Compresses a bit stream by building a dictionary of symbols of any length up
to n bits, then Huffman-coding the stream of placed symbols.

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
| n, maximum symbol bit length | the largest window size, so the longest possible symbol; the only parameter |
| window size, L | the exact bit length being merged into at the current stage, 2..=n |
| symbol definition | a specific bit string of a specific length |
| placed symbol | one position in the stream's current parse; the parse always spells the input exactly |
| frequency | how many times a symbol is placed |
| leaf | a symbol with frequency > 0, i.e. a leaf of the Huffman tree |
| dead symbol | a symbol defined earlier whose frequency fell to 0 |
| encoded symbol bits | sum over leaves of frequency × code length (depth) = exact bits of the encoded symbols |
| header | the tree and symbol definitions in the container; **measured, never used to decide** |
| merge candidate | a bit string of exactly L bits filled by 2+ adjacent placed symbols, nothing poking out |
| occurrence | one place a candidate's window is filled exactly |
| parts | the distinct placed symbols a merge consumes |
| merge | replacing a candidate's occurrences with one symbol |
| deletion | removing some parts entirely after a merge, respelling their remaining placements |
| respelling | spelling a deleted symbol's bits with other known symbols |
| revival | a respelling using a dead symbol, which becomes a leaf again |
| option | a merge alone, or a merge plus a deletion of a subset of its parts |
| pass | find candidates once, then run rounds until nothing commits or a deletion happens |
| round | analyze every remaining candidate, rank options, commit the best passing one |

---

## Current rules, with the reason for each

1. **Start state.** Every input bit is a placed `0` or `1`. Window sizes start at 2,
   because 1-bit symbols never get a Huffman code shorter than 1 bit.
2. **Candidates.** Exactly fill the window with 2+ placed symbols; any mix of raw bits
   and earlier symbols. A placed symbol is never split.
3. **Occurrences.** The most non-overlapping occurrences, taken left to right. All
   occurrences have the same bit length, so this gives the maximum count.
4. **A merge must repeat** (2+ occurrences): a single occurrence is not a pattern.
   *Measured:* fewer leaves and smaller output on every test file.
5. **Shorter-code rule.** A merge commits only if its code bits (tree after the merge)
   are fewer than the code bits of the placements it replaces (tree before).
   *Why:* the stricter "total encoded symbol bits must drop" rule committed nothing at all on text,
   because the first new leaf pushes `0` or `1` a level deeper.
6. **The header never enters a decision.** It is measured and logged only.
7. **Ranking.** score = frequency × (1 + (bits_before − bits_final) / bits_before), where bits are
   the encoded symbol bits,
   × (leaves removed / leaves added) when the leaf count drops. Exact integer
   fractions; x/0 ranks above any finite ratio. Ties: lower bits_final, higher frequency,
   smaller bit value.
8. **Deletion options.** For every merge, every subset of its remaining parts is tried
   as a deletion. Respelling uses the fewest pieces, then the fewest revivals; dead
   symbols may be revived. A deletion option exists only if it lowers the leaf count,
   and it is committed even if the encoded symbol bits rise: a smaller dictionary is the goal.
9. **Re-score after every commit**, since the tree changes. A deletion moves positions,
   so the pass ends and the next pass rescans.
10. **Stopping.** A window size ends when a pass commits nothing, or when an exact
    state (all frequencies) repeats, which catches loops of any length.

---

## Container format

```
"ASLE" magic (32 bits)
Elias gamma(input bit count + 1)
Elias gamma(n)                                   (omitted for empty input)
size table: n bits, bit i set if some leaf is i+1 bits long
Huffman tree, depth first: 1 = internal; 0 = leaf, then
    index into the size table (exactly enough bits) + the symbol's bits
body: the Huffman code of every placed symbol
```

---

## Measured history (bytes; n = 16)

| Version | text 7,346 | binary 16,384 | packed 5-bit 10,000 | Rust source 16,384 | random 8,192 |
|---|---|---|---|---|---|
| total size (header included) must drop | 7,357 | — | 5,054 | — | — |
| shorter-code rule | 6,656 | 8,104 | 6,816 | 13,683 | 15,811 |
| + size-table header | 6,557 | 7,980 | 6,775 | 13,548 | 15,420 |
| + must repeat | 6,074 | 7,187 | 6,650 | 12,998 | 11,564 |
| + encoded-symbol-bits modifier on every option | 6,063 | 7,168 | 6,649 | 12,993 | 11,547 |
| + deletion of all parts, encoded symbol bits must drop | 6,063 | 7,168 | 6,649 | 12,993 | 11,547 |
| + leaf-removing options ×2, deletion even if encoded symbol bits rise | 7,035 | 8,544 | 8,478 | 15,582 | 13,524 |
| + removed/added ratio instead of ×2 | 7,043 | 8,947 | 8,479 | 14,895 | 13,626 |
| + every subset of parts (**current**) | 7,280 | 8,502 | 9,274 | 15,796 | 12,373 |
| gzip -9 | 2,611 | 3,520 | 6,350 | 5,038 | 8,222 |
| xz -9 | 2,644 | 2,636 | 6,340 | 4,924 | 8,252 |

The first row was not run on every file.

## Observations from measurement

- With deletions gated on the encoded symbol bits, no deletion ever lowered them
  (text: 0 of 562 respellable plans; on average they rose about 1,400 bits).
- With deletions allowed to raise the encoded symbol bits, almost every deletion removes 2 leaves and adds 1.
  Plain merges add most leaves; on text, 79% of them consumed respelled pieces.
- Text, commit 34 (window 6): one merge-with-deletion respelled 1,181 `001` and 150
  `011` back into single bits: 1 leaf fewer, encoded symbol bits from 58,420 to 68,770, placed
  symbols from 15,098 to 17,495.

## Open questions

- Why the leaf count still grows overall when deletions are allowed.
- Whether deletions cause more merges or only different ones (compare runs per window size).

---

## Code layout (dependencies point down the list)

| Module | Job |
|---|---|
| `main` | command line only |
| `json_lines_log` | one observer: writes every event as JSON Lines |
| `container_format` | the file layout; the only module that knows the header |
| `symbol_builder` | decision loop: window sizes, passes, rounds, commit rules |
| `builder_events` | event records and the observer interface |
| `option_ranking` | scores and ordering |
| `change_prediction`, `merge_candidates` | analysis: read-only predictions |
| `symbol_stream` | the parse state; the only code that changes it |
| `predicted_changes` | plain data for predictions |
| `symbol_definition`, `huffman_tree`, `bit_io`, `fast_hash` | building blocks |

## Running

```
asle compress   <input> <output> --max-symbol-bits <n> [--verbose] [--log <file.jsonl>]
asle decompress <input> <output>
asle benchmark  --max-symbol-bits <n> [--verbose] <files...>
python3 tools/plot_encoder_log.py <file.jsonl> <chart.html>
```
