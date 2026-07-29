# Perpetum state

Cycle: 3 · Stage: b12 · Updated: 2026-07-29

Generated from the journal by `perp state`. The journal is the truth; if this file disagrees with it, this file is wrong.

## Position

- Last step: `c3/b12/s22` — closed
- Nothing in flight.
- Steps: 45 done, 1 blocked

## Blocked

- `c1/b5/s07` — gate build — exit 101

  ```
  gate: build
  sha: ffe5618513a6bb1b3bc4a8e2444276684ee7611a
  $ cargo build --workspace
  cwd: ..\crates
  env: declared: kept 16, set 0
  exit 101 in 699ms
  --- stderr ---
     Compiling perp v0.1.0 (D:\repos\justcode\crates\perp)
  error: failed to remove file `D:\repos\justcode\crates\target\debug\perp.exe`
  
  Caused by:
    Access is denied. (os error 5)
  ```


## Requirements touched

`L-21` · `L-22` · `L-3` · `L-4` · `L-8` · `N-8` · `N-9` · `N-10` · `O-1` · `L-7` · `L-5` · `V-2`

## Steps

| Step | Outcome | Summary |
|---|---|---|
| `c1/b2/s06` | done | gate lint — green |
| `c1/b2/s06` | done | gate build — green |
| `c1/b2/s06` | done | gate test — green |
| `c1/b5/s07` | done | gate lint — green |
| `c1/b5/s07` | blocked | gate build — exit 101 |
| `c1/b3/s08` | done | gate lint — green |
| `c1/b3/s08` | done | gate build — green |
| `c1/b3/s08` | done | gate test — green |
| `c1/b5/s08` | done | gate lint — green |
| `c1/b5/s08` | done | gate build — green |
| `c1/b5/s08` | done | gate test — green |
| `c1/b3/s09` | done | drill closed by the operator — the park was correct, and this is how a parked step is resolved |
| `c1/b4/s09` | done | gate lint — green |
| `c1/b4/s09` | done | gate build — green |
| `c1/b4/s09` | done | gate test — green |
| `c1/b1/s13` | done | lint, build and test all green |
| `c1/b1/s14` | done | all four mutants went red; restored tree green |
| `c2/b6/s05` | done | gate lint — green |
| `c2/b6/s05` | done | gate build — green |
| `c2/b6/s05` | done | gate test — green |
| `c2/b7/s06` | done | gate lint — green |
| `c2/b7/s06` | done | gate build — green |
| `c2/b7/s06` | done | gate test — green |
| `c2/E/s06` | done | gate lint — green |
| `c2/E/s06` | done | gate build — green |
| `c2/E/s06` | done | gate test — green |
| `c2/b9/s07` | done | gate lint — green |
| `c2/b9/s07` | done | gate build — green |
| `c2/b9/s07` | done | gate test — green |
| `c2/b10/s07` | done | gate lint — green |
| `c2/b10/s07` | done | gate build — green |
| `c2/b10/s07` | done | gate test — green |
| `c2/b8/s08` | done | gate lint — green |
| `c2/b8/s08` | done | gate build — green |
| `c2/b8/s08` | done | gate test — green |
| `c3/b11/s06` | done | gate lint — green |
| `c3/b11/s06` | done | gate build — green |
| `c3/b11/s06` | done | gate test — green |
| `c3/b12/s15` | done | gate lint is green |
| `c3/b12/s16` | done | gate build is green |
| `c3/b12/s17` | done | gate test is green |
| `c3/b12/s18` | done | stopped: the backlog is exhausted |
| `c3/b12/s19` | done | gate lint is green |
| `c3/b12/s20` | done | gate build is green |
| `c3/b12/s21` | done | gate test is green |
| `c3/b12/s22` | done | stopped: the backlog is exhausted |
