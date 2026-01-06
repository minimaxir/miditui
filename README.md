# miditui

[Crates.io](https://crates.io/crates/miditui)

An interactive terminal app/UI for MIDI composing, mixing, and playback—written in Rust.

`miditui` allows for a DAW-like experience in the terminal and has many features that you wouldn't expect a terminal app to have:

- Full terminal mouse support: click, drag, scroll, double-click, right-click all work, which allows you to pan views, select notes, click piano keys to play them
- A piano roll view for showing the notes as they are played in the song
  - An Insert mode to press keys on your keyboard (or simply click the piano roll) and create music in real time: Two-octave QWERTY layout (Z-M and Q-I rows) with live audio playback as you type, visual keyboard showing held notes
- A project timeline timeline view to see all the notes
- Low-latency 44.1kHz audio via [rustysynth](https://github.com/sinshu/rustysynth)
- Timeline seeking by clicking the time rulers to skip to any point of the track
- Unlimited MIDI tracks with per-track mute/solo, volume/pan (L/R) controls, and automatic MIDI channel assignment
- Autosave that periodically saves your project and automatically reloads it when restarting the app
- Undo/Redo support to avoid losing work
- Import/Export MIDI and JSON files, plus export the music as a WAV file.

Watch this video to see `miditui` in action:

_**Disclosure:** This crate was coded with the assistance of Claude Opus 4.5, mostly as a personal experiment just to see how well modern coding agents can handle TUIs and I figured a full-on MIDI mixer which has atypical UI requirements would be a more **interesting** test. Opus 4.5 did a good job and after a demo [went viral on X](https://x.com/minimaxir/status/2005779586676842646) people were asking for me to release it, so I decided to spend extra time polishing and comprehensively testing the app before then open-sourcing it. I have written a full analysis of the agentic coding workflow—including the prompts provided to Opus 4.5—in the [agent_notes folder](agent_notes/)._

## Installation

The app binaries can be downloaded from the Releases page of this repo, or by using the following terminal commands:

If Rust is installed, you can install the crate directly via `cargo`:

```bash
cargo install miditui
```

Additionally, a SoundFont file (`.sf2`) is required to run `miditui`. There are many free SoundFonts which are commercially friendly for music generation: a small one is `TimGM6mb.sf2` ([6 MB, direct download link](https://sourceforge.net/p/mscore/code/HEAD/tree/trunk/mscore/share/sound/TimGM6mb.sf2?format=raw)), while a more robust SoundFont is [GeneralUser GS](https://github.com/mrbumpy409/GeneralUser-GS/tree/main) ([32.3 MB, direct download link](https://github.com/mrbumpy409/GeneralUser-GS/raw/refs/heads/main/GeneralUser-GS.sf2)).

It is also strongly recommended to use a terminal that support horizontal mouse scrolling which not all do: I recommend [Ghostty](https://ghostty.org).

## Example Usage

To run `miditui`: if you downloaded the binary, run it in the terminal with `./miditui`. If you installed via Rust, run `cargo run`. On the first load, the app will prompt you to select the path to a SoundFont: the path to the SoundFont will be saved for future runs.

There are a _very_ large number of keyboard shortcuts that are too big to fit into the README: press `?` in the app for documentation.

If you want example MIDIs for testing, you can view the examples folder.

## Notes

- Due to variations in terminal support, key release events [cannot be reliably detected](https://stackoverflow.com/a/74422335), which means the piano-key-input intentionally does not support holding keys to extend notes, unfortunately.
- Music files are autosaved as a bespoke `.oxm` binary file, which essentially wraps the song metadata with a few added fields outside of the MIDI spec, such as the SoundFont path and the mute/solo status of each track.

## License

MIT
