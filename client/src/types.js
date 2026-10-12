// JavaScript types matching server/src/types.rs and snapshot objects.

/** @typedef {number} PlayerId */
/** @typedef {string} GameId */
/** @typedef {number} EntityId */

/**
 * @typedef {'Blue'
 *   | 'Brown'
 *   | 'Green'
 *   | 'Black'
 *   | 'Gray'
 *   | 'Olive'
 *   | 'Cyan'
 *   | 'Survivor'
 *   | 'Zombie'
 *   | 'Villain'} CharacterType
 */

/**
 * @typedef {'Pistol'
 *   | 'DesertEagle'
 *   | 'Ak47'
 *   | 'RocketLauncher'} WeaponType
 */

/**
 * @typedef {Object} Position
 * @property {number} x
 * @property {number} y
 */

/**
 * @typedef {Object} PlayerState
 * @property {PlayerId} player_id
 * @property {CharacterType} character_type
 * @property {string} username
 * @property {number} health
 * @property {number} ammo
 * @property {number} ammo_capacity
 * @property {number} speed
 * @property {boolean} riding_train
 * @property {number} respawn_count
 * @property {number} kills
 * @property {number} deaths
 * @property {number} aim_angle
 * @property {WeaponType} weapon_type
 */

/**
 * @typedef {Object} BulletState
 * @property {PlayerId} owner
 * @property {WeaponType} weapon_type
 * @property {number} aim_angle
 */

/**
 * @typedef {Object} WeaponState
 * @property {WeaponType} weapon_type
 * @property {number} expires_at_millis
 * @property {number} lifetime_millis
 */

/**
 * @typedef {Object} TrainState
 * @property {number} carriage_index
 * @property {number} corner_count
 * @property {number} aim_angle
 * @property {number} width
 * @property {number} height
 * @property {number} speed
 */

/**
 * @typedef {{ Player: PlayerState }
 *   | { Bullet: BulletState }
 *   | { Weapon: WeaponState }
 *   | { Train: TrainState }} GameObject
 */

/**
 * @typedef {Object} ObjectState
 * @property {EntityId} entity_id
 * @property {Position} position
 * @property {GameObject} object
 */

export {};
