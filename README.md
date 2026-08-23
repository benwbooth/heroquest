# HeroQuest 3D

HeroQuest 3D is a native 3D board-game experience inspired by the original
1989 US Game System. It puts the board, miniatures, furniture, cards, dice,
quest book, and Zargon’s screen on a virtual table, with a computer game master
running the adventure.

The campaign includes the 14 original US quests, character setup, spells,
treasure, traps, searches, combat, physical dice, figure movement, and a
camera that follows the action. The game is designed for mouse, keyboard, and
trackpad play.

HeroQuest and its artwork are the property of their respective owners. This is
an unofficial fan project and is not affiliated with or endorsed by Hasbro,
Avalon Hill, or Games Workshop.

## Download

Use the latest release page to download the package for your computer:

[Download the latest HeroQuest 3D release](https://github.com/benwbooth/heroquest/releases/latest)

### Windows

Recommended: download **HeroQuest-windows-x86_64.msi**, open it, and follow the
installer. The game will appear in the Start menu.

For a no-install version, download **HeroQuest-windows-x86_64-portable.zip**,
extract it to a folder, and run `heroquest.exe`.

### macOS

Download **HeroQuest-macos-universal.dmg**, open it, and drag **HeroQuest** to
Applications. The universal app runs on both Apple Silicon and Intel Macs.

Homebrew users can install the cask included with each release:

```sh
curl -L -o heroquest.rb \
  https://github.com/benwbooth/heroquest/releases/latest/download/heroquest.rb
brew install --cask ./heroquest.rb
```

The first launch may require macOS approval in **System Settings → Privacy &
Security** because the app is distributed outside the Mac App Store.

If macOS still refuses to open the app after approval, remove the downloaded
file quarantine flag and try again:

```sh
xattr -cr /Applications/HeroQuest.app
```

### Linux

The **AppImage** is the easiest option:

```sh
curl -L -o HeroQuest.AppImage \
  https://github.com/benwbooth/heroquest/releases/latest/download/HeroQuest-linux-x86_64.AppImage
chmod +x HeroQuest.AppImage
./HeroQuest.AppImage
```

To install the **Flatpak** bundle instead:

```sh
flatpak install --user ./HeroQuest-linux-x86_64.flatpak
flatpak run com.heroquest.Game
```

Linux users who prefer a regular executable can download
**HeroQuest-linux-x86_64-static-binary.tar.gz**, extract it, and run
`./heroquest`. This archive includes the same runtime files and first-run asset
installer as the AppImage and Flatpak packages; AppImage or Flatpak is still
recommended for the simplest desktop integration.

## First launch

The release packages contain the game and its project-authored environment, but
the original-US scan collection and classic model collection are downloaded
from their source sites when needed. They are not hosted in this repository.

On the first launch, an in-game setup screen explains the sources, asks you to
accept responsibility for the download and local use, and shows live progress.
Downloads resume after an interruption. Allow about **2.45 GiB** of network
transfers and roughly **7 GiB** of free disk space for the prepared asset pack.

If you do not want automatic downloads, provide your own legally obtained art
and model files using the instructions in
[`docs/local-art-pack.md`](docs/local-art-pack.md).

## Playing

The opening flow takes you through the box, quest selection, hero assignment,
spell selection, and the ready screen. During play, use the highlighted
buttons and squares; the on-screen guide only shows actions that are legal at
that moment.

- Roll movement dice, then click a highlighted square to move.
- Click an adjacent enemy to attack, or use the attack action shown on screen.
- Use the buttons for doors, searches, spells, items, and ending a turn.
- Drag with the left mouse button to orbit the camera.
- Use the mouse wheel or a two-finger pinch to zoom.
- Press `H` to return the camera to its default view.
- Press `Esc` to quit.

The camera moves toward dice rolls, movement, attacks, spells, and enemy turns
so the important action stays visible. Dice settle physically on the table;
their visible upward faces determine the result.

## Troubleshooting

If the first-run setup is interrupted, start the game again and choose **Retry**.
Partial downloads are kept. If the game reports missing assets after a retry,
make sure the installation folder is writable and that `bash`, `curl`, and the
asset extraction tools are available on Linux.

For bug reports, include your operating system, package type, and the message
shown by the in-game setup screen. Please do not upload copyrighted scans or
other private asset files to an issue.

## For developers

The game is written in Rust using SDL3, `wgpu`, and Rapier. To build it locally,
install Rust and the SDL3 development libraries, then run:

```sh
cargo test
cargo run
```

The implementation notes, asset boundaries, and optional development tools are
kept in the [`docs/`](docs/) directory so the player-facing instructions above
stay focused.

## License

Original contributions in this repository are dedicated to the public domain
under [CC0 1.0 Universal](LICENSE), subject to the limitations in
[`NOTICE.md`](NOTICE.md). HeroQuest names, rules, scans, models, and other
third-party material remain subject to their owners’ rights and individual
licenses.
