# Client assets

```
characters/
  blue/player.png
  brown/player.png
  green/player.png
weapons/
  weapon-sheet.svg
  rocket-sheet.svg
  pistol/bullet.png
  pistol/fire.wav
```

`src/assets.js` maps the backend's `CharacterType` and `WeaponType` strings to
these files. Add a weapon entry there when adding a new gun. Players start with a pistol. Random ground pickups equip Pistol, Ak47, RocketLauncher, or DesertEagle and refill ammunition; character choice is cosmetic.

## Sources and licenses

- Characters: **Kenney**, [Topdown Shooter](https://opengameart.org/content/topdown-shooter), CC0.
  Original files: `manBlue_hold.png`, `manBrown_hold.png`, `womanGreen_hold.png`,
  `hitman1_hold.png`, `manOld_hold.png`, `soldier1_hold.png`, `robot1_hold.png`,
  `survivor1_hold.png`, and `zoimbie1_hold.png`.
  Original license included as `Kenney-License.txt`.
- Bullet sheet: **dantheman.fyi / oglsdl**, [Ammo Sprite](https://opengameart.org/content/ammo-sprite), CC0.
  Original file: `Ammo000.png`. The renderer selects its left cartridge and tints
  it golden for every weapon; the downloaded image is unchanged.
- Pistol sound: **Tabasco / Vince Sevedge**, [Gunshot Sounds](https://opengameart.org/content/gunshot-sounds), CC0.
  Original recording: `sounds/cz.wav`, titled "CZ-52 Pistol". `fire.wav` extracts
  one shot starting at 0.255 seconds, lasts 0.45 seconds, filters low-frequency
  noise, fades the tail, and converts to mono 44.1 kHz PCM.

Sprites are preloaded before joining. Audio is unlocked by the user's join
click and played for server-confirmed `GunFired` events, rather than every
input message. Audio is skipped while the tab is hidden.

## Additional weapons

All four guns use distinct cropped illustrations from **SpriteAttack's** [Gun Construction Kit](https://opengameart.org/content/gun-construction-kit), released under CC0. The shared SVG sheets retain the original artwork; the browser selects each gun and flips the rocket launcher to face the aim direction. These are stylized illustrations rather than exact firearm models. Additional sounds are distinct
single-shot excerpts from the same CC0 Gunshot Sounds pack: `sks.wav` for AK,
`mosin.wav` for Desert Eagle, and `shotty.wav` for rocket launcher. These are
placeholder weapon sounds, rather than recordings of the exact weapon models.
Each clip is 0.55 seconds, mono 44.1 kHz, with a high-pass filter and tail fade.

Damage is hardcoded in `server/src/constants.rs`: pistol 25, AK 20,
Desert Eagle 50, rocket 100. Shot velocity is aim direction × three times the shooter's terrain/sprint walking speed, plus their actual world velocity from walking and train transport. That combined velocity stays constant across terrain; the bullet sprite points along its flight direction. The firing interval is unchanged.
Rockets deal direct-hit damage only. Health resets to 100 immediately on death,
at a valid random position at least 300 units away, keeping the same weapon; all clients receive the kill announcement.

The train wheel loop at `props/train-wheels.wav` is the user-provided recording from [Train wheels sound (Sound effect)](https://www.youtube.com/watch?v=Iki5jlRSZ9Q), moved unchanged from the downloaded WAV. Its upstream license has not been verified; it is not covered by the project MIT license. The whistle is original synthesized audio and sounds when the leading carriage turns a corner. Every sound has a world-space source, distance attenuation, and left/right stereo positioning relative to the listener.

Water ambience uses the user-provided `terrain/water.mp3` recording, downloaded as `creek-trickling-over-stones-by-dragon-studio-royalty-free-music-283274.mp3`. Its upstream license has not been verified; it is not covered by the project MIT license. Grass-footstep audio is original synthesized audio under the project license. Water loops near river tiles; grass swishes follow walking over floor/grass and forest tiles. The train wheel loop has a wider distance range than gunshots.

Pickups spawn every 1.5 seconds (twice the earlier rate), with 32 initially and a cap of 64. Per unit of available area, spawn weights are inversely proportional to terrain walking speed: normal ground 1×, sand 1.33×, forest 1.67×, and water 2.86×. They still expire after 30 seconds.

Train healing restores 3 HP per second. The health bar is red off the train and bright green aboard. `sounds/player-death.wav` is an original synthesized death cue under the project MIT license, played at the victim’s death position with distance attenuation.

Weapon selection odds are pistol 40%, AK 30%, Desert Eagle 20%, rocket launcher 10%, independent of terrain location weighting.
