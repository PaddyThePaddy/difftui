# difftui

BeyondCompare/meld alternative in tui

![demo](assets/demo.gif)

## Installation

```bash
cargo install https://github.com/PaddyThePaddy/difftui --locked
```

## Keymaps

| Key               | Action                       |
| ----------------- | ---------------------------- |
| Arrow keys / hjkl | Navigation                   |
| `q`               | Closet tab                   |
| `c`               | Compare selected file/folder |
| `o`               | Open selected filde / folder |
| `g`               | Move to top                  |
| `G`               | Move to bottom               |
| `f`               | Filter files                 |
| `z`               | Tab specific actions         |
| `R`               | Reload tab                   |
| `x`               | Swap sides                   |
| `/`               | Search                       |
| `n` / `N`         | Search next/previous         |
| `]`/`[`           | Next/Previous difference     |
| `=`               | Decouple side-by-side view   |
| Enter             | Expand/Collapse              |
| Ctrl + `c`        | Exit app                     |
| `?` / `F1`        | Show help                    |

## To do

- Diff binary files with `similar` crate
- Search highlight in text compare view
- Config file and configurable keymap
- Proper cli option, especially disabling git ignore
