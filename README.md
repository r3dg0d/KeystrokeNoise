# KeystrokeNoise

Subtle mechanical keyboard + mouse click sounds for Linux — **without logging key content**.

Key identities are mapped to a coarse category (`normal` / `space` / `enter` / `modifier` / `mouse-*`) and discarded immediately. Nothing is stored or transmitted.

## Install

### Cargo

```bash
cargo install --path .
# or from a clone:
cargo build --release
install -Dm755 target/release/keystroke-noise ~/.local/bin/keystroke-noise
```

### Nix

```bash
nix build
nix run . -- --help
# binary: ./result/bin/keystroke-noise
# user unit: ./result/lib/systemd/user/keystroke-noise.service
```

### User systemd unit

```bash
mkdir -p ~/.config/systemd/user
cp packaging/keystroke-noise.service ~/.config/systemd/user/
# If the binary is not at ~/.local/bin, edit ExecStart= in the unit.
systemctl --user daemon-reload
```

## Quick start

```bash
keystroke-noise init
keystroke-noise test normal
keystroke-noise test mouse-left
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

## CLI

```text
keystroke-noise --help
keystroke-noise status
keystroke-noise test <normal|space|enter|modifier|mouse-left|mouse-right|mouse-middle>
```

## Privacy

- Reads `/dev/input/event*` (needs `input` group or seat ACL)
- Never logs key codes, characters, or device key names after categorization
- No network; audio stays local (PipeWire / Pulse via rodio)

## License

MIT — see [LICENSE](LICENSE). Mouse click samples under `assets/third_party/` retain their upstream CC0 / attribution notes.
