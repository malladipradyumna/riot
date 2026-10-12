// Oneof field names are wire details; application code uses the labels on the left.
const CLIENT_MESSAGE_FIELDS = {
  JoinGame: 'join_game',
  ExitGame: 'exit_game',
  PlayerInput: 'player_input',
};
const SERVER_MESSAGE_NAMES = {
  welcome: 'Welcome',
  state_snapshot: 'StateSnapshot',
  gun_fired: 'GunFired',
  player_killed: 'PlayerKilled',
  error: 'Error',
  train_whistle: 'TrainWhistle',
  player_damaged: 'PlayerDamaged',
};
const SNAPSHOT_OBJECT_NAMES = {
  player: 'Player',
  bullet: 'Bullet',
  weapon: 'Weapon',
  train: 'Train',
};

// The browser and Rust consume the same schema. The runtime is vendored locally.
let clientEnvelope;
let serverEnvelope;
const protocolReady = fetch('assets/protocol/game.proto')
  .then(async (response) => {
    if (!response.ok) throw new Error('Could not load protocol schema');
    const root = protobuf.parse(await response.text(), { keepCase: true }).root;
    clientEnvelope = root.lookupType('riot.ClientEnvelope');
    serverEnvelope = root.lookupType('riot.ServerEnvelope');
    return true;
  })
  .catch((error) => {
    console.error(error);
    return false;
  });

// Keep protobuf field names at this boundary; the rest of the client uses message names.
function encodeClientMessage(message) {
  const type = Object.keys(message)[0];
  if (!CLIENT_MESSAGE_FIELDS[type] || !clientEnvelope)
    throw new Error('Invalid client message');
  // Normalize JS enum names and numbers, validate, then serialize the envelope.
  const value = clientEnvelope.fromObject({
    [CLIENT_MESSAGE_FIELDS[type]]: message[type],
  });
  const error = clientEnvelope.verify(value);
  if (error) throw new Error(error);
  return clientEnvelope.encode(value).finish();
}

// Decode binary frames and turn protobuf oneofs into the application message shapes.
function decodeServerMessage(bytes) {
  // Convert protobuf longs to Numbers and enum values to the strings used by the UI.
  const value = serverEnvelope.toObject(serverEnvelope.decode(bytes), {
    longs: Number,
    enums: String,
    defaults: true,
    arrays: true,
    oneofs: true,
  });
  const type = value.payload;
  if (!SERVER_MESSAGE_NAMES[type]) throw new Error('Missing server payload');
  const payload = value[type];
  // Expand nested object oneofs into the same shapes documented in types.js.
  if (type === 'state_snapshot') {
    payload.objects = payload.objects.map((object) => {
      const kind = object.object;
      if (!SNAPSHOT_OBJECT_NAMES[kind])
        throw new Error('Unknown snapshot object');
      return {
        entity_id: object.entity_id,
        position: object.position || { x: 0, y: 0 },
        object: { [SNAPSHOT_OBJECT_NAMES[kind]]: object[kind] },
      };
    });
  }
  return { [SERVER_MESSAGE_NAMES[type]]: payload };
}
