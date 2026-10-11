# Client assets

```
characters/
  blue/player.png
  brown/player.png
  green/player.png
weapons/
  pistol/gun.png
  pistol/bullet.png
  pistol/fire.wav
```

`src/assets.js` maps the backend's `CharacterType` and `WeaponType` strings to
these files. Add a weapon entry there when adding a new gun. Players randomly receive Pistol, Ak47, RocketLauncher, or DesertEagle; character choice is cosmetic.

## Sources and licenses

- Characters and pistol: **Kenney**, [Topdown Shooter](https://opengameart.org/content/topdown-shooter), CC0.
  Original files: `manBlue_hold.png`, `manBrown_hold.png`, `womanGreen_hold.png`,
  and `weapon_gun.png`. Original license included as `Kenney-License.txt`.
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

AK uses Kenney's `weapon_machine.png` from the same CC0 pack. Desert Eagle and
rocket launcher use original SVG drawings. Additional sounds are distinct
single-shot excerpts from the same CC0 Gunshot Sounds pack: `sks.wav` for AK,
`mosin.wav` for Desert Eagle, and `shotty.wav` for rocket launcher. These are
placeholder weapon sounds, rather than recordings of the exact weapon models.
Each clip is 0.55 seconds, mono 44.1 kHz, with a high-pass filter and tail fade.

Damage is hardcoded in `server/src/constants.rs`: pistol 25, AK 20,
Desert Eagle 50, rocket 100. All shots keep the existing speed and fire interval.
Rockets deal direct-hit damage only. Health resets to 100 immediately on death,
at a valid random position at least 300 units away, keeping the same weapon; all clients receive the kill announcement.

Train wheel and whistle WAV files in `props/` are original synthesized audio. The wheel loop plays near the train; its whistle sounds when the leading carriage turns a corner.
