# KeystrokeNoise

Subtle mechanical keyboard sounds for Linux — **without logging key content**.

Key identities are mapped to a coarse category (`normal` / `space` / `enter` / `modifier`) and discarded immediately. Nothing is stored or transmitted.

## Quick start

```bash
keystroke-noise init
keystroke-noise test normal
systemctl --user enable --now keystroke-noise.service
```

Toggle (also bound to `SUPER+SHIFT+PERIOD` on zionsec):

```bash
keystroke-noise toggle
```

## Config

`~/.config/keystroke-noise/config.toml`

Sounds live in `~/.config/keystroke-noise/sounds/`. Regenerate packaged defaults:

```bash
python3 scripts/gen-sounds.py
cp assets/*.wav ~/.config/keystroke-noise/sounds/
```

## Privacy

- Reads `/dev/input/event*` (needs `input` group or seat ACL)
- Never logs key codes, characters, or device key names after categorization
