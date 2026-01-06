_**Editor's note**: My style of prompting Claude Code is with long Markdown prompts that are comprehensive and ideally give the LLM no wiggle room to interpret intent incorrectly. They are invoked by tagging the file, and then giving the command `implement`. After each prompt, the code is manually reviewed, manually tested to ensure the implementation matches the input prompt, then manually committed to `git`._

_This list of prompts is not 100% comprehensive: quick in-line followups are not recorded._

# 01.md

Create a robust and fun terminal application using the Rust programming language and the `ratatui` crate to create a MIDI composer and player as an interactive TUI terminal app. This app has similar UI and functionality to something such as Final Cut Pro.

This app MUST support adding an unlimited amount of tracks, both visually and programmatically. It must also support a way to export the combined audio.

It must support loading soundfont files: `TimGM6mb.sf2` is present in `/assets` for loading as a default.

You MUST use the latest versions of `rustysynth` with `rodio` to implement the MIDI player.

You MUST also add a user interactive MIDI composer, where the user can play the MIDI notes.

---

# 02.md

Add comprehensive and intuitive mouse click and scroll controls to the app.

---

# 03.md

The clicks are at inaccurate locations if the note canvas is scrolled. Fix so the clicks are at accurate locations

Additionally, add a way to cycle instruments in the `TimGM6mb.sf2` soundfont.

---

# 04.md

You MUST add ALL the following features to the Rust application in `/src`:

- Complete the work on any code stubs present in the project
- Add the ability to name tracks
- Add the ability to add a track with a mouse button click
- Add the ability to export the resulting full project composition as a .wav file
- Add a full-project view which shows all tracks on the timeline, and if they are playing audio or not.

Ensure that this new code is big-O optimal and follows DRY principles (these MUST not be noted in code comments)

---

# 05.md

You MUST add ALL the following features to the Rust application in `/src`:

- Create a way to export and import the project as a .json file, using a very efficient output schema. You may use the `serde` crate for serialization/deserialization. Create a test JSON file with 4 different tracks and instruments to test this
- Sometimes, inputs are unclear when certain modes are active, such as `Insert` mode. Make the visible key bindings contextual to the given selection.
- Remove the "Add Track" test and add a "Remove Track" button where the text used to be.

Ensure that this new code is big-O optimal and follows DRY principles (these MUST not be noted in code comments)

---

# 06.md

You MUST add ALL the following features to the Rust application in `/src`:

- Add the ability to set the volume of each track, for better mixing
- Add the ability to set the L/R channels of a track on a granular scale
- Create an efficient binary encoding for the project using `serde` that leverages the mostly-numeric data types within the project, and the ability to save and load in this optimized binary data type. Give the data type the `.oxm` extension.
- Add UI to indicate which format should be exported (.wav, .json, or .oxm)
- Add an autosave functionality to the current project after 5 seconds since the last diff interaction, saving it in this optimized binary type.

Ensure that this new code is big-O optimal and follows DRY principles (these MUST not be noted in code comments)

---

# 07.md

You MUST add ALL the following features to the Rust application in `/src`:

- There is a bug where if a note is scheduled at 0:00:000, it does not play. Fix this.
- Allow the user to change the BPM from 120 and change the time signature from 4/4.

Ensure that this new code is big-O optimal and follows DRY principles (these MUST not be noted in code comments)

---

# 08.md

When `Save` is invoked, open up a UI that asks the user for the file name with a default, and
whether they want it saved as `.oxm` or `.json`. When loading a file, invoke a file browser UI
to allow the user to search for it

---

# 09.md

Add a visual indicator for Tracks in the `Piano Roll` view to allow the user to easily determine if there active notes on the track scrolled off-screen.

---

# 10.md

You MUST add ALL the following features to the Rust application in `/src`:

- Allow horizontal scrolling through the `Piano Roll` and `Project Timeline` views
- Add a new default view which shows the `Piano Roll` and `Project Timeline` at the same time by splitting them horizontally. This new view is a part of the cycle between views and is the new default view.
- Add a `Ctrl+Space` shortcut which always restarts the song and plays it from the beginning.

Ensure that this new code is big-O optimal and follows DRY principles (these MUST not be noted in code comments)

---

# 11.md

You MUST add ALL the following features to the Rust application in `/src`:

- Change the `CTRL+Space` shortcut to be `Shift+Space` instead.
- For the Combined View, the red indicator lines align between the `Piano Roll` and the `Project Timeline` are not aligned. Truncate the margin and track name of the `Project Timeline` in the Combined view ONLY so the indicator lines align. (see attached image)
- The `export` feature should ONLY allow exporting as a WAV. It should also be more intuitive as to what happens after pressing `e`.
- Add a secondary view for the `Tracks` view that renders each track and its metadata on two lines instead of one line, with a keyboard shortcut to toggle.
- Add an indicator to the bottom keyboard shortcut bar to the quit command.

Ensure that this new code is big-O optimal and follows DRY principles (these MUST not be noted in code comments)

---

# 12.md

Add a keyboard shortcut to toggle the behavior where the `Project Timeline` appears white as it's playing.

---

# 13.md

Polish the Rust codebase in `/src`, which includes

- Removing unused and redundant code
- Optimizing code to follow DRY if possible
- Removing tautological code comments
- Documenting ALL keyboard shortcuts in the Help if they are not already documented.

---

# 14.md

Add MIDI `.mid` file importing. A test file is present at `spy_thriller_theme.mid`.

---

# 15.md

_**Editor's note**: This prompt was removed and never ran because it was a silly idea._

---

# 16.md

Add autosave functionality: after a period of inactivity, the file is automatically saved as a `autosave.oxm` file in the current working directory. After relaunching the app, it automatically reloads the autosave file.

Add a CLI command to start the app with a new project instead.

Also, add a `Ctrl+N` new project shortcut, with a confirmation modal.

---

# 17.md

Change the following defaults:

- The two-line track view (`t`) is default ON.
- The flashing Project Timeline blocks (`Shift+W`) is default OFF.

---

# 18.md

Create a WebAssembly/WASM version of the app, that MOST IMPORTANTLY retains the terminal aestethics and usability. It should have similar styles to the TUI app (see attached image)

- The theme of the web page is to make it appear entirely as it was a dark mode terminal, through CSS. It MUST look like an extension of the `ratatui` terminal styling, e.g. thin lines for sections, with the titles being seamlessly embedded in the borders. Follow the style of the app in the provided image #1.
- Use `picocss` as the base CSS framework.
- Use the `theme.ini` file as a baseline for corresponding terminal colors.
- Use `Jetbrains Mono` from Google Fonts as the fontface.

The webpage itself:

- Has:
  - a header which contains a button to the GitHub repository
  - A small hero section to explain the app
  - a body section with a responsive width (up to a maximum) for the main CLI app,
  - and a section after it to contain other tools
- Hide/disable ALL keyboard shortcuts and features in the TUI app that do not make sense on the webpage (e.g. Save/Load/Autosave)

The tools section is a tabbed section with the following tabs:

- `Examples` (default): conains buttons to automatically loads an example `.oxm` to demo the app. Copy `tests_project_files/spy_thriller_theme.oxm` as one of these so far
- `Import`: Buttons which Invokes the loading function, users can import their own JSON/.mid files
- `Export`: Buttons which Invokes the export function, allowing users to save the project as a WAV/JSON/.oxm/MIDI
- `Keyboard Shortcuts`: Comprehensive list of keyboard shortcuts in the same style as the Help Menu: see image #2

The app should still use autosave functionality by using browser LocalStorage to store the `.oxm` file.

---

# 19.md

When the `Piano Roll` interface is scrolled, the time markers (beats and measure) disappear. Ensure the indicators persist and are at the correct spot while scrolling.

Ensure that this new code is big-O optimal and follows DRY principles (these MUST not be noted in code comments)

---

# 20.md

For the `/web` WASM app, make the following changes:

- There MUST be no modal blocking access to the page. Ensure the rest of the page is visible with the dialogue box should prompt for user access to enable Web Audio and then launch the app.
- ALL WASM components needed to run the web page MUST be present in `/web`, including the bundled WASM with a corresponding `package.json`. The user must able to run the web app fully self-contained.

After making these changes, run a web server and verify.

---

# 21.md

Implement ALL the following changes and bug fixes:

- When a solo is triggered, if a note in another track is already playing, the note continues to play. Fix it such that when a solo is triggered, it immediately silences all other non-solo tracks.
- When a note is selected, allow the following keyboard shortcuts:
  - Shift+Left/Right to reduce/expand the note length
  - Shift+Up/Down to move the selected note up/down a key
- Click behavior when the `Piano Roll` view is scrolled horizontally executes the click at the wrong spot. Fix it by using the scroll to offset the click.
- Click behavior when the `Tracks` view is scrolled down executes the click at the wrong spot. Fix it by using the scroll to offset the click.
- Add scrolling behavior to the Help menu to be able to see all Keyboard shortcuts.
- Sometimes, a track's instrument plays when unpausing even if there are no active notes on the `Piano Roll` for that track. (e.g. if doing a solo). Investigate and fix this behavior.
- When a new project is loaded, the play track progression does not reset. It should reset to the start.
- The project is now officially named `miditui`. Adjust the naming where appropriate in the code.
- The WebAssemply/WASM code is no longer necessary and should be removed. Remove all references that differentiate between `native` and `web` code (since there is no longer any web code) and remove all WASM-code (do NOT remove the `/web` folder)

Ensure that this new code is big-O optimal and follows DRY principles (these MUST not be noted in code comments)

---

# 22.md

Add white highlighting for a complete note to the `Piano Roll` view when it is being played. This should be default on. The Shift+W key then cycles:

- `Piano Roll` on
- `Piano Roll` on, `Project Timeline` on
- All off
- `Project Timeline` on

Additionally, there may be a display lag where the vertical indicator does not visible match when the note is being played. Adjust the display to eliminate this lag, perhaps trying to set the vertical line movement before the note is played to avoid a lag.

---

# 23.md

After the `Piano Roll` and `Project Timeline` are scrolled (either automatically during playback or manually), the indicator lines become desynced, potentially as a result of the last change. Ensure the indicator lines are synced.

---

# 24.md

Implement comprehensive and robust undo/redo functionality with Ctrl+Z/Ctrl+Y for ALL possible user-initiated changes, holding a history of up to 8 different changes. This may require a separate data structure to be able to manage changes.

Additionally, handle potential errors that may result if the assumptions behind an undo/redo option are no longer valid, and prevent the application from crashing. If there isn't a graceful way to handle this case, clear the memory instead.

---

# 25.md

With the current implementation of Undo/Redo, only one level of Redo can be done before the stack is cleared.

The user MUST be able to Undo then Redo the same amount of times they undo'ed, e.g. if they undoed 4 changes, they must be able to Redo those same 4 changes if no other state changes have occured.

Additionally, clear the undo/redo stack on new projects/loads.

---

# 26.md

When inserting a new note via clicking, the note is inserted at one key level lower than the mouse position. It is possible that the addition of the time code bar has created a click position discrepancy. Fix the insertion code so it uses the correct click position.

---

# 27.md

When running `cargo run`, the Rust CLI shows massive amounts of warnings of dead code and unused imports. Fix them all so no warnings are shown:

[REMOVED FOR BREVITY]

---

# 28.md

Make ALL the following fixes to the vertical red indicator line:

- The `Project Timeline` vertical indicator line is no longer synced with the `Piano Roll` timeline, potentially due to a change in the `Project Timeline` to make the line more responsice. Make them synced.
- Clicking on the time ruler should seek the track to that location (submeasure or measure) in the playback. Ensure this implementation works when clicking either the `Project Timeline` or `Piano Roll` rulers, and ensure it accounts for potential scrolling offsets.

---

# 29.md

While a song is playing, if the user attempts to double-click to insert a note, the note is inserted at the time indicator's position instead of the mouse position. Fix it so it appears at the mouse position.

---

# 30.md

Make ALL the following fixes to the note insertion logic in the `Piano Roll`:

- When adding a note, play the note's audio (using the same instrument of the track).
- When double-clicking a note, it does not delete the note although the Delete Note event still fires. Fix it so when a note is deleted, it is deleted.
- Allow user to click and drag a selected note around (up/down pitch, up/down the track)
- With a note selected, change the keyboard shortcuts of WASD to move the note in the appropriate direction. With a note selected, set Shift+A to shrink the note, and Shift+D to expand the note. Remove the Shift+Up/Down/Left/Right keyboard shortcuts because they do not work with all terminals.

---

# 31.md

Add logic for the user to load a specified soundfont (a `.sf2` or `.sf3` file) from the filesystem instead of loading `/assets/TimGM6mb.sf2` on launch.

- The user should be able to load a soundfont at any time with a documented Ctrl+L shortcut.
- On the first load of the app, the user should be prompted with a modal to load a soundfont that informs them why a soundfont is necessary, with a call-to-action along the lines of "Click this dialogue or press Ctrl+L to select a soundfont to load."
- Once a soundfont is loaded, the reference to tbe soundfont path is saved with exports to `.json` or `.oxm`, i.e. it will also be autosaved with the project. Therefore, the modal should not be displayed when reloading the app via autosave or import if a valid soundfont is found.
- Add a CLI option to specify a path to the soundfont to automatically load it.

---

# 32.md

Adjust instrument names such that they are derived from the loaded soundfile itself, instead of having a predefined list of instrument/ID mappings.

---

# 33.md

Make playing the piano via Keyboard keys in `Insert Mode` more intuitive and representative of a standard media app with a MIDI controller:

- Always display the red vertical indicator line in this mode.
- When the user plays a piano shortcut key, the line starts moving. During this state, notes should be added at the time measure specified by the indicator line. This line continues until 2 measures pass with zero added piano notes via the keyboard.
- Ensure that notes can be added simultaneously if two different piano keys are pressed simultaneously.
- There is currently a bug where after a period of time in insert mode, no new notes are added when pressing the keyboard keys. Double check to see if there is a bug.

Additionally:

- Add the instrument name of the current track to the `Piano Roll` title.
- Display the keyboard shortcut to increment/decrement the instrument/instrument ID of the current track
- The `Help` menu is incorrect: ensure the Footer text is visible at all times, and ensure the user can use mouse scrolling to navigate through the `Help` Menu.

---

# 34.md

Make the following additional changes to note insertion logic:

- Whenever a note is added (in either `Insert Mode` or `Normal Mode`), color that note blue to make it distinctive until a new note is added in a different beat
- Additionally color the corresponding key note in the bottom of the UI blue until a new note is added in a different beat
- Whenever a note is added, if the new note is not visible in the piano roll viewport, scroll the piano roll widget both horizontally and vertically to the insert location of the note

---

# 35.md

Add support for opening `.mid` and `.midi` MIDI files via the Ctrl+O browser, and add support for saving files as `.midi` via the CTRL+S workflow.

---

# 36.md

Do another pass of all the Rust code in the `/src` folder to fully and completely optimize the application.

You MUST obey ALL the FOLLOWING rules during your optimization:

- Rewrite unnecessairly verbose and unoptimized code
- Avoid DRY, and rewrite code to use common data structures if possible to avoid DRY
- Remove all tautological code comments

---

# 37.md

Make ALL the following bug fixes involving switching `Track` instruments:

- Allow the user to switch `Track` instruments in `Insert Mode`, immediately silencing all current playing notes on the `Track` when doing so.
- If the user switches `Track` instruments while an `Track` note is playing, it will continue to play indefinitely.

---

# 38.md

In the `Piano Roll` only view, the piano roll does not extend the full vertical distance if the terminal window is tall. Ensure all vertical space is utilized.

---

# 39.md

When creating a new project via CTRL+N:

- Reset the playback and `Insert Mode` seek position back to base 0:00:000.
- Maintain the current mode (default or `Insert Mode`) and octave settings

---

# 40.md

Test additional different compiliation optimization settings to reduce the size of the Rust `release` binary to be as small as possible. Keep track of the binary sizes across different settings.
