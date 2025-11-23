# Player Damage

This describes how we will handle player damage - currently, the enemies cannot damage the player.

## Part 1: Ensuring monsters create 'Damage' events

There is already a 'Damage' message (MessagePayLoad::Damage) - we'll need to make sure this impacts the player entity in both cases:
1. A melee attack, which gets triggered at acertain point in animation (it's some animation flag?) - we'd need to make sure that, based on some criteria (ie, the ai is close enough), that we'd send the damage message to the player entity
2. A projectile attack - when a monster launches a projectile, once the projectile is close enough to the player, it should also damage the player.
3. Explosions - we need to make sure explosions (explosive barrels, etc) damage the player

## Part 2: Ensuring the 'Damage' event impacts the player

We need to make sure that the MessagePayload::Damage actually impacts the player - should this be a custom script or built into the player entity?

## Testing

We should build a debug scene debug_damage.rs where we spawn a couple types of entities (a pipe hybrid, a shotgun hybrid, and a midwife) and verify all the sorts of damage impact the player. In addition, we should also add an explosive barrel that can be triggered.


## Open Questions

- Should we use PropHitPoints on the player entity, or add hit_points to the PlayerInfo?
