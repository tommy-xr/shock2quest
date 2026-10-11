# Nightdive LGMD v6 plant shapes

Observed in the 25th Anniversary 1.3 `mods/sshock2ee.kpf`, independently of
PMNM. `sq_scripts/object_plants.nut` actively registers these archetypes;
`Hanging Plant` and `Tall Plant` registrations are commented out.

| Archetype | Template | Model | Named targets |
| --- | --- | --- | --- |
| Ivy | -2582 | talplant | motion_one, motion_two, motion_three |
| Plant #1 | -68 | flower | motion_one, motion_two, motion_three |
| Plant #2 | -69 | plant1 | motion_one, motion_two, motion_three |
| Wide Plant | -1855 | plantw | motion_one, motion_two, motion_three |
| Egg Depositor | -930 | orifice | contract |
| Tall Plant 2 | -1854 | plantt | motion_one, motion_two, motion_three |
| U-Shaped Nerve | -3437 | nerveu | motion_one, motion_two, motion_three |
| Straight Nerve | -3438 | nervest | motion_one, motion_two, motion_three |

Names/model wiring were checked against the mounted remaster gamesys. This is
shipped-asset evidence, not a comparison against a running Nightdive executable.
The egg depositor requests motion_one/motion_two too, but its model has neither;
missing named shapes contribute zero. It has two LGMD subobjects, so its deltas
must be applied before the existing skeletal transforms.

## Encoding

All offsets are absolute, little endian. The existing v4 prefix remains valid.

| Byte offset | u32 field |
| --- | --- |
| 134 | Polygon-corner shape-index array offset |
| 138 | Shape delta array offset |
| 142 | Shape name array offset |
| 146 | Number of shapes |
| 150 | Number of shape vertices |

The first array has one u16 per polygon corner, in polygon/corner file order.
Indices are **one-based**, with zero meaning no deformation. Triangulation
must carry those corner identities through fan expansion and material grouping.
For PLANT1 and NERVEST, subtracting one makes the position deltas agree across
all corners sharing each original vertex; zero-based interpretation disagrees
for 273/400 and 51/527 vertices respectively.

Deltas are shape-major, 32 bytes per vertex: position xyz, normal xyz, UV xy,
as f32. Convert vectors with the normal Dark-to-runtime axis mapping and scale
only position deltas. Names are 16-byte zero-padded strings. Basis is all zero
in the inspected models. The table sizes satisfy
`names_offset - deltas_offset = shapes * shape_vertices * 32`.
The shipped `base.kpf/progs/DarkMDModel.shader` confirms that position, normal,
and UV offsets are weighted and added before the object transform.

Malformed or truncated extensions fall back to the ordinary base mesh. The
shipped PLANTWA extension does not contain valid names at its declared offset;
it is not one of the driver's active targets and is left static.

## Driver and rendering

For each visible eligible instance, use `(SimTimeMilliseconds + DarkX + DarkY)`
and weights `sin(value/4000)/2`, `sin(value/600)/2`, `sin(value/1000)/2`.
The three render channels are specifically the plant driver contract, not a
generic index convention for arbitrary LGMD targets. Future egg-pod or SHODAN
drivers must explicitly map their own decoded names to channels. The third
channel selects contract on the egg depositor. The geometry keeps an
immutable base and updates a reusable vertex buffer from that base for each
instance's weights. Consecutive equal weights reuse the buffer (including
unchanged depth/material passes). Static and skinned materials both receive
already-deformed vertices, so their lighting/depth/overlay paths stay aligned.

Collision, interaction triangles, ECS positions, and authored rest bounds remain
unchanged. Time comes from the simulation clock; no mutable animation state is
needed in saves. This initial CPU deformation path has not been profiled on a
Quest device; no device performance improvement is claimed.
