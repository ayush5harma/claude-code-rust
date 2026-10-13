# PRES heading one

## PRES heading two

### PRES heading three

A paragraph with **bold text**, *italic text*, ~~struck text~~, `inline code` and a [link label](https://example.com/pres).

- PRES bullet one
  - PRES nested a
    - PRES nested deeper
  - PRES nested b
- PRES bullet two

1. PRES first
2. PRES second
   - PRES numbered child

> PRES quoted line one
> PRES quoted line two

| Name | Qty | Note |
| --- | ---: | --- |
| apple | 3 | fresh |
| kiwi | 12 | a longer cell that widens its column |

```rust
fn main() {
    let items: Vec<u32> = (1..=3).collect();
    println!("PRES rust {:?}", items);
}
```

```bash
#!/usr/bin/env bash
set -euo pipefail
for f in *.txt; do echo "PRES bash $f"; done
```

```json
{"pres": "json", "list": [1, true, null]}
```

```diff
--- a/pres.txt
+++ b/pres.txt
@@ -1,2 +1,2 @@
-PRES old line
+PRES new line
```

PRES long line: the quick brown fox jumps over the lazy dog again and again while the measured terminal viewport keeps on going well past one hundred and twenty columns so that wrapping behaviour is visible without any ambiguity at all.
