// Keys match CharacterType and WeaponType values in server snapshots.
const CHARACTER_ASSETS = {
  Blue: { sprite: 'assets/characters/blue/player.png' },
  Brown: { sprite: 'assets/characters/brown/player.png' },
  Black: { sprite: 'assets/characters/black/player.png' },
  Gray: { sprite: 'assets/characters/gray/player.png' },
  Olive: { sprite: 'assets/characters/olive/player.png' },
  Cyan: { sprite: 'assets/characters/cyan/player.png' },
  Survivor: { sprite: 'assets/characters/survivor/player.png' },
  Zombie: { sprite: 'assets/characters/zombie/player.png' },
  Green: { sprite: 'assets/characters/green/player.png' },
};

const WEAPON_ASSETS = {
  Ak47: {
    sprite: 'assets/weapons/weapon-sheet.svg',
    spriteSource: [140, 2150, 930, 310],
    bulletSprite: 'assets/weapons/ak/bullet.svg',
    bulletSize: [4, 16],
    sound: 'assets/weapons/ak/fire.wav',
    width: 24,
    height: 9,
  },
  DesertEagle: {
    sprite: 'assets/weapons/weapon-sheet.svg',
    spriteSource: [215, 1105, 325, 165],
    bulletSprite: 'assets/weapons/desert-eagle/bullet.svg',
    bulletSize: [6, 12],
    sound: 'assets/weapons/desert-eagle/fire.wav',
    width: 18,
    height: 10,
  },
  RocketLauncher: {
    sprite: 'assets/weapons/rocket-sheet.svg',
    spriteSource: [110, 82, 302, 65],
    spriteFlipped: true,
    bulletSprite: 'assets/weapons/rocket-launcher/bullet.svg',
    bulletSize: [10, 24],
    sound: 'assets/weapons/rocket-launcher/fire.wav',
    width: 28,
    height: 13,
  },
  Pistol: {
    sprite: 'assets/weapons/weapon-sheet.svg',
    spriteSource: [625, 1110, 230, 155],
    width: 16,
    height: 10,
    bulletSprite: 'assets/weapons/pistol/bullet.png',
    // The downloaded ammo sheet has three cartridges; use the left cartridge.
    bulletSource: [253, 260, 255, 995],
    sound: 'assets/weapons/pistol/fire.wav',
  },
};

const TILE_SIZE = 50;
const TERRAIN_ASSETS = {
  '.': 'assets/terrain/floor.svg',
  f: 'assets/terrain/forest.svg',
  w: 'assets/terrain/water.svg',
  s: 'assets/terrain/sand.svg',
  '#': 'assets/terrain/wall.svg',
  t: 'assets/terrain/rail.svg',
  b: 'assets/terrain/bridge.svg',
  u: 'assets/terrain/tunnel.svg',
};
const TERRAIN_SPEEDS = {
  '.': 200,
  f: 120,
  w: 70,
  s: 150,
  '#': 0,
  t: 200,
  b: 200,
  u: 160,
};
let terrainRows = [];
const mapReady = fetch('assets/maps/arena.txt').then(async (response) => {
  if (!response.ok) throw new Error('Could not load map');
  terrainRows = (await response.text()).trimEnd().split(/\r?\n/);
  if (
    terrainRows.length !== 36 ||
    terrainRows.some((row) => row.length !== 96 || /[^.fws#tbu]/.test(row))
  )
    throw new Error('Invalid map');
});
function terrainAt(x, y) {
  return (
    terrainRows[Math.floor(y / TILE_SIZE)]?.[Math.floor(x / TILE_SIZE)] || '.'
  );
}

const assetImages = new Map();
const imagePaths = [
  ...new Set([
    'assets/props/train.svg',
    'assets/props/train-carriage.svg',
    ...Object.values(TERRAIN_ASSETS),
    ...Object.values(CHARACTER_ASSETS).map((asset) => asset.sprite),
    ...Object.values(WEAPON_ASSETS).flatMap((asset) => [
      asset.sprite,
      asset.bulletSprite,
    ]),
  ]),
];
const assetsReady = Promise.all([
  protocolReady.then((ready) => {
    if (!ready) throw new Error('Could not load Protobuf protocol');
  }),
  mapReady,
  ...imagePaths.map(
    (path) =>
      new Promise((resolve, reject) => {
        const image = new Image();
        image.onload = () => {
          assetImages.set(path, image);
          resolve(image);
        };
        image.onerror = () => reject(new Error(`Could not load ${path}`));
        image.src = path;
      }),
  ),
]).then(
  () => true,
  (error) => {
    console.error(error);
    return false;
  },
);

let audioContext = null;
const weaponSounds = new Map();
const pendingSounds = new Map();
const SOUND_ASSETS = {
  ...WEAPON_ASSETS,
  TrainWheels: { sound: 'assets/props/train-wheels.wav' },
  TrainWhistle: { sound: 'assets/props/train-whistle.wav' },
  PlayerDeath: { sound: 'assets/sounds/player-death.wav' },
  Water: { sound: 'assets/terrain/water.mp3' },
  GrassStep: { sound: 'assets/terrain/grass-step.wav' },
};

function unlockAudio() {
  const Context = window.AudioContext || window.webkitAudioContext;
  if (!Context) return;
  if (!audioContext) audioContext = new Context({ latencyHint: 'interactive' });
  if (audioContext.state !== 'running')
    audioContext.resume().catch(console.error);
  for (const [type, asset] of Object.entries(SOUND_ASSETS)) {
    if (weaponSounds.has(type) || pendingSounds.has(type)) continue;
    const loading = fetch(asset.sound, { cache: 'no-store' })
      .then(async (response) => {
        if (!response.ok) throw new Error(`Could not load ${asset.sound}`);
        weaponSounds.set(
          type,
          await audioContext.decodeAudioData(await response.arrayBuffer()),
        );
      })
      .catch((error) => console.error(error))
      .finally(() => pendingSounds.delete(type));
    pendingSounds.set(type, loading);
  }
}

function soundLevels(source, listener, range = 450) {
  if (!source || !listener) return { volume: 0, pan: 0 };
  const distance = Math.hypot(source.x - listener.x, source.y - listener.y);
  return {
    volume: 1 / (1 + (distance / range) ** 2),
    pan: Math.max(-1, Math.min(1, (source.x - listener.x) / 700)),
  };
}

function playWeaponSound(type, position, listener, baseVolume = 0.4) {
  const buffer = weaponSounds.get(type);
  if (
    !audioContext ||
    audioContext.state !== 'running' ||
    !buffer ||
    document.hidden
  )
    return;
  const levels = soundLevels(position, listener);
  if (!levels.volume) return;
  const source = audioContext.createBufferSource();
  const gain = audioContext.createGain();
  const pan = audioContext.createStereoPanner();
  source.buffer = buffer;
  gain.gain.value = levels.volume * baseVolume;
  pan.pan.value = levels.pan;
  source.connect(gain);
  gain.connect(pan);
  pan.connect(audioContext.destination);
  source.start();
  source.onended = () => {
    source.disconnect();
    gain.disconnect();
    pan.disconnect();
  };
}

const tintedBulletSprites = new Map();

function bulletSprite(weaponType, color) {
  const key = `${weaponType}:${color}`;
  if (tintedBulletSprites.has(key)) return tintedBulletSprites.get(key);
  const asset = WEAPON_ASSETS[weaponType];
  if (!asset) return null;
  const image = assetImages.get(asset.bulletSprite);
  if (!image) return null;
  const sprite = document.createElement('canvas');
  const [width, height] = asset.bulletSize || [4, 12];
  sprite.width = width;
  sprite.height = height;
  const ctx = sprite.getContext('2d');
  if (!ctx) return null;
  if (asset.bulletSource)
    ctx.drawImage(image, ...asset.bulletSource, 0, 0, width, height);
  else ctx.drawImage(image, 0, 0, width, height);
  ctx.globalCompositeOperation = 'source-in';
  ctx.fillStyle = color;
  ctx.fillRect(0, 0, width, height);
  tintedBulletSprites.set(key, sprite);
  return sprite;
}

// Each ambient sound owns at most one loop. Repeated frames adjust its source,
// gain and pan without creating more nodes or replaying the buffer.
const ambientLoops = new Map();
function stopAmbientLoop(type) {
  const loop = ambientLoops.get(type);
  if (!loop) return;
  loop.source.stop();
  loop.source.disconnect();
  loop.gain.disconnect();
  loop.pan.disconnect();
  ambientLoops.delete(type);
}
function updateAmbientLoop(type, position, listener, baseVolume, range) {
  if (!position || !listener) {
    stopAmbientLoop(type);
    return;
  }
  const buffer = weaponSounds.get(type);
  if (!audioContext || audioContext.state !== 'running' || !buffer) return;
  let loop = ambientLoops.get(type);
  if (!loop) {
    loop = {
      source: audioContext.createBufferSource(),
      gain: audioContext.createGain(),
      pan: audioContext.createStereoPanner(),
    };
    loop.source.buffer = buffer;
    loop.source.loop = true;
    loop.gain.gain.value = 0;
    loop.source.connect(loop.gain);
    loop.gain.connect(loop.pan);
    loop.pan.connect(audioContext.destination);
    loop.source.start();
    ambientLoops.set(type, loop);
  }
  const levels = soundLevels(position, listener, range);
  loop.gain.gain.setTargetAtTime(
    document.hidden ? 0 : levels.volume * baseVolume,
    audioContext.currentTime,
    0.08,
  );
  loop.pan.pan.setTargetAtTime(levels.pan, audioContext.currentTime, 0.08);
}
function updateTrainAudio(trains, player) {
  if (!player || !trains.length) {
    stopTrainAudio();
    return;
  }
  const nearest = trains.reduce((best, train) =>
    Math.hypot(train.x - player.x, train.y - player.y) <
    Math.hypot(best.x - player.x, best.y - player.y)
      ? train
      : best,
  );
  // Wheel clatter has a wider audible range than individual gunshots.
  updateAmbientLoop('TrainWheels', nearest, player, 0.85, 1600);
}
function stopTrainAudio() {
  stopAmbientLoop('TrainWheels');
}

const footstepPositions = new Map();
let nearestWater = null;
let nextWaterScan = 0;
function updateTerrainAudio(players, listener) {
  if (!listener) {
    stopTerrainAudio();
    return;
  }
  const now = performance.now();
  if (now >= nextWaterScan) {
    nextWaterScan = now + 150;
    nearestWater = null;
    let nearestDistance = Infinity;
    const row = Math.floor(listener.y / TILE_SIZE);
    const col = Math.floor(listener.x / TILE_SIZE);
    for (let y = row - 7; y <= row + 7; y++) {
      for (let x = col - 7; x <= col + 7; x++) {
        if (terrainRows[y]?.[x] !== 'w') continue;
        const position = { x: (x + 0.5) * TILE_SIZE, y: (y + 0.5) * TILE_SIZE };
        const distance = Math.hypot(
          position.x - listener.x,
          position.y - listener.y,
        );
        if (distance < nearestDistance) {
          nearestDistance = distance;
          nearestWater = position;
        }
      }
    }
  }
  updateAmbientLoop('Water', nearestWater, listener, 0.15, 180);
  const present = new Set();
  for (const player of players) {
    present.add(player.player_id);
    const previous = footstepPositions.get(player.player_id);
    const sameLife = previous && previous.respawn === player.respawn_count;
    const distance = sameLife
      ? Math.hypot(player.x - previous.x, player.y - previous.y)
      : 0;
    const tile = terrainAt(player.x, player.y);
    let walked = sameLife ? previous.walked : 0;
    if (
      !player.riding_train &&
      (tile === '.' || tile === 'f') &&
      distance < 30
    ) {
      walked += distance;
      if (walked >= 45) {
        playWeaponSound('GrassStep', player, listener, 0.22);
        walked %= 45;
      }
    } else {
      walked = 0;
    }
    footstepPositions.set(player.player_id, {
      x: player.x,
      y: player.y,
      respawn: player.respawn_count,
      walked,
    });
  }
  for (const id of footstepPositions.keys())
    if (!present.has(id)) footstepPositions.delete(id);
}
function stopTerrainAudio() {
  stopAmbientLoop('Water');
  footstepPositions.clear();
  nearestWater = null;
  nextWaterScan = 0;
}
document.addEventListener('visibilitychange', () => {
  if (document.hidden) {
    for (const loop of ambientLoops.values())
      loop.gain.gain.cancelScheduledValues(audioContext.currentTime);
    for (const loop of ambientLoops.values()) loop.gain.gain.value = 0;
    footstepPositions.clear();
  }
});
