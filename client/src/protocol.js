// JSDoc types provide editor checking while keeping this plain JavaScript.
// These are application objects. wire.js encodes them using assets/protocol/game.proto.

/** @typedef {import('./types.js').PlayerId} PlayerId */
/** @typedef {import('./types.js').GameId} GameId */
/** @typedef {import('./types.js').ObjectState} ObjectState */
/** @typedef {import('./types.js').CharacterType} CharacterType */
/** @typedef {import('./types.js').WeaponType} WeaponType */
/** @typedef {import('./types.js').Position} Position */

/**
 * @typedef {Object} ClientRequest
 * @property {string} client_request_id
 * @property {number} timestamp_millis
 */

/**
 * @typedef {ClientRequest & {
 *   username: string,
 *   character_type: CharacterType,
 *   bot_count?: number,
 *   bot_difficulty?: 1 | 2 | 3,
 *   game_id: GameId | null
 * }} JoinGamePayload
 */

/**
 * @typedef {ClientRequest & {
 *   username: string,
 *   game_id: GameId,
 *   player_id: PlayerId
 * }} ExitGamePayload
 */

/**
 * @typedef {ClientRequest & {
 *   player_id: PlayerId,
 *   game_id: GameId,
 *   up: boolean,
 *   down: boolean,
 *   left: boolean,
 *   right: boolean,
 *   shooting: boolean,
 *   sprinting: boolean,
 *   aim_angle: number
 * }} PlayerInputPayload
 */

/**
 * @typedef {{ JoinGame: JoinGamePayload }
 *   | { ExitGame: ExitGamePayload }
 *   | { PlayerInput: PlayerInputPayload }} ClientMessage
 */

/**
 * @typedef {Object} ServerResponse
 * @property {string} server_request_id
 * @property {number} timestamp_millis
 * @property {PlayerId} player_id
 * @property {GameId} game_id
 */

/**
 * @typedef {Object} RoundState
 * @property {number} number
 * @property {boolean} intermission
 * @property {number} ends_at_millis
 */

/**
 * @typedef {Object} InputAck
 * @property {PlayerId} player_id
 * @property {string} last_processed_client_request_id
 */

/**
 * @typedef {Object} SnapshotPayload
 * @property {GameId} game_id
 * @property {number} timestamp_millis
 * @property {RoundState} round
 * @property {ObjectState[]} objects
 * @property {InputAck[]} acknowledgements
 */

/**
 * @typedef {Object} PlayerDamagedPayload
 * @property {GameId} game_id
 * @property {PlayerId} player_id
 * @property {number} damage
 * @property {number} health
 */

/**
 * @typedef {Object} PlayerKilledPayload
 * @property {GameId} game_id
 * @property {PlayerId} killer_id
 * @property {PlayerId} victim_id
 * @property {string} killer_name
 * @property {string} victim_name
 * @property {WeaponType} weapon_type
 * @property {Position} position
 */

/**
 * @typedef {ServerResponse & {
 *   weapon_type: WeaponType,
 *   position: Position,
 *   aim_angle: number
 * }} GunFiredPayload
 */

/** @typedef {{ PlayerDamaged: PlayerDamagedPayload }} PlayerDamagedMessage */
/** @typedef {{ TrainWhistle: { game_id: GameId, position: Position } }} TrainWhistleMessage */
/** @typedef {{ Welcome: ServerResponse }} WelcomeMessage */
/** @typedef {{ GunFired: GunFiredPayload }} GunFiredMessage */
/** @typedef {{ StateSnapshot: SnapshotPayload }} StateSnapshotMessage */

// wire.js transports these application objects as binary Protobuf envelopes.
/**
 * @typedef {PlayerDamagedMessage
 *   | TrainWhistleMessage
 *   | WelcomeMessage
 *   | StateSnapshotMessage
 *   | GunFiredMessage
 *   | { PlayerKilled: PlayerKilledPayload }
 *   | { Error: { code: string } }} ServerMessage
 */

export {};
