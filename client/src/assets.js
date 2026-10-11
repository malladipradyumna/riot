// Keys match CharacterType and WeaponType values in server snapshots.
const CHARACTER_ASSETS = {
  Blue: { sprite: 'assets/characters/blue/player.png' },
  Brown: { sprite: 'assets/characters/brown/player.png' },
  Green: { sprite: 'assets/characters/green/player.png' },
};

const WEAPON_ASSETS = {
  Ak47: {
    sprite: 'assets/weapons/ak/gun.png',
    bulletSprite: 'assets/weapons/ak/bullet.svg',
    bulletSize: [4, 16],
    sound: 'assets/weapons/ak/fire.wav',
    width: 24, height: 9,
  },
  DesertEagle: {
    sprite: 'assets/weapons/desert-eagle/gun.svg',
    bulletSprite: 'assets/weapons/desert-eagle/bullet.svg',
    bulletSize: [6, 12],
    sound: 'assets/weapons/desert-eagle/fire.wav',
    width: 18, height: 10,
  },
  RocketLauncher: {
    sprite: 'assets/weapons/rocket-launcher/gun.svg',
    bulletSprite: 'assets/weapons/rocket-launcher/bullet.svg',
    bulletSize: [10, 24],
    sound: 'assets/weapons/rocket-launcher/fire.wav',
    width: 28, height: 13,
  },
  Pistol: {
    sprite: 'assets/weapons/pistol/gun.png',
    bulletSprite: 'assets/weapons/pistol/bullet.png',
    // The downloaded ammo sheet has three cartridges; use the left cartridge.
    bulletSource: [253, 260, 255, 995],
    sound: 'assets/weapons/pistol/fire.wav',
  },
};

const TILE_SIZE = 50;
const TERRAIN_ASSETS = { '.': 'assets/terrain/floor.svg', f: 'assets/terrain/forest.svg', w: 'assets/terrain/water.svg', s: 'assets/terrain/sand.svg', '#': 'assets/terrain/wall.svg', t: 'assets/terrain/rail.svg', b: 'assets/terrain/bridge.svg', u: 'assets/terrain/tunnel.svg' };
const TERRAIN_SPEEDS = { '.': 200, f: 120, w: 70, s: 150, '#': 0, t: 200, b: 200, u: 160 };
let terrainRows = [];
const mapReady = fetch('assets/maps/arena.txt').then(async response => {
  if (!response.ok) throw new Error('Could not load map');
  terrainRows = (await response.text()).trimEnd().split(/\r?\n/);
  if (terrainRows.length !== 36 || terrainRows.some(row => row.length !== 96 || /[^.fws#tbu]/.test(row))) throw new Error('Invalid map');
});
function terrainAt(x, y) {
  return terrainRows[Math.floor(y / TILE_SIZE)]?.[Math.floor(x / TILE_SIZE)] || '.';
}

const assetImages = new Map();
const imagePaths = [
  'assets/props/train.svg',
  'assets/props/train-carriage.svg',
  ...Object.values(TERRAIN_ASSETS),
  ...Object.values(CHARACTER_ASSETS).map((asset) => asset.sprite),
  ...Object.values(WEAPON_ASSETS).flatMap((asset) => [asset.sprite, asset.bulletSprite]),
];
const assetsReady = Promise.all([protocolReady.then(ready => { if (!ready) throw new Error("Could not load Protobuf protocol"); }), mapReady, ...imagePaths.map((path) => new Promise((resolve, reject) => {
  const image = new Image();
  image.onload = () => { assetImages.set(path, image); resolve(image); };
  image.onerror = () => reject(new Error(`Could not load ${path}`));
  image.src = path;
}))]).then(() => true, (error) => {
  console.error(error);
  return false;
});

let audioContext = null;
const weaponSounds = new Map();
let audioLoading = null;

function unlockAudio() {
  // Called from a join click/key press so browser autoplay rules permit sound.
  const Context = window.AudioContext || window.webkitAudioContext;
  if (!Context) return;
  if (!audioContext) audioContext = new Context();
  audioContext.resume().catch(console.error);
  if (!audioLoading) {
    audioLoading = Promise.all(Object.entries({ ...WEAPON_ASSETS, TrainWheels: { sound: 'assets/props/train-wheels.wav' }, TrainWhistle: { sound: 'assets/props/train-whistle.wav' } }).map(async ([type, asset]) => {
      const response = await fetch(asset.sound);
      if (!response.ok) throw new Error(`Could not load ${asset.sound}`);
      const buffer = await audioContext.decodeAudioData(await response.arrayBuffer());
      weaponSounds.set(type, buffer);
    })).catch((error) => {
      console.error(error);
      audioLoading = null;
    });
  }
}

function playWeaponSound(weaponType, volume = 0.25) {
  const buffer = weaponSounds.get(weaponType);
  if (!audioContext || audioContext.state !== 'running' || !buffer || document.hidden) return;
  const source = audioContext.createBufferSource();
  const gain = audioContext.createGain();
  source.buffer = buffer;
  gain.gain.value = volume;
  source.connect(gain);
  gain.connect(audioContext.destination);
  source.start();
  source.onended = () => { source.disconnect(); gain.disconnect(); };
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
  if (asset.bulletSource) ctx.drawImage(image, ...asset.bulletSource, 0, 0, width, height);
  else ctx.drawImage(image, 0, 0, width, height);
  ctx.globalCompositeOperation = 'source-in';
  ctx.fillStyle = color;
  ctx.fillRect(0, 0, width, height);
  tintedBulletSprites.set(key, sprite);
  return sprite;
}

// One loop per joined game, regardless of the number of carriages.
let trainWheelSource = null;
let trainWheelGain = null;
let lastTrainCorner = null;
function updateTrainAudio(trains, player) {
  if (!player || !trains.length) { stopTrainAudio(); return; }
  const engine = trains.find(train => train.carriage_index === 0);
  const distance = Math.min(...trains.map(train => Math.hypot(train.x - player.x, train.y - player.y)));
  const volume = Math.max(0, 1 - distance / 1200);
  if (engine) {
    if (lastTrainCorner !== null && engine.corner_count > lastTrainCorner) {
      playWeaponSound('TrainWhistle', volume * 0.45);
    }
    lastTrainCorner = engine.corner_count;
  }
  const buffer = weaponSounds.get('TrainWheels');
  if (!audioContext || audioContext.state !== 'running' || !buffer) return;
  if (!trainWheelSource) {
    trainWheelSource = audioContext.createBufferSource();
    trainWheelGain = audioContext.createGain();
    trainWheelSource.buffer = buffer;
    trainWheelSource.loop = true;
    trainWheelSource.connect(trainWheelGain);
    trainWheelGain.connect(audioContext.destination);
    trainWheelSource.start();
  }
  trainWheelGain.gain.setTargetAtTime(document.hidden ? 0 : volume * 0.16, audioContext.currentTime, 0.1);
}
function stopTrainAudio() {
  if (trainWheelSource) {
    trainWheelSource.stop();
    trainWheelSource.disconnect();
    trainWheelGain.disconnect();
  }
  trainWheelSource = null;
  trainWheelGain = null;
  lastTrainCorner = null;
}
document.addEventListener('visibilitychange', () => {
  if (document.hidden && trainWheelGain) trainWheelGain.gain.value = 0;
});
