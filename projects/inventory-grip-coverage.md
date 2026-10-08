# Inventory hand-pose coverage

Audited `shock2.gam` and all 23 installed missions against the prepared grip libraries. The audit found **119 pickup models without either hand pose**. Added **238 poses**, one for each hand, at **0.55 scale**. Also replaced the four existing small/large worm-beaker poses at that scale. The empty beakers were among the missing models, so all four beaker variants now have fresh left/right fits.

The fitter scales the mesh **before** searching, then stores the result with `item_scale: 0.55`. Every generated pose has at least three finger contacts. Existing poses are preserved except for the explicitly requested worm-beaker replacements. Explicitly fitted defaults are marked `authored` so an unrelated bulk rebake cannot silently discard the chosen scale.

## Scope

- Candidates have an inherited world pickup/ammo action, an authored container link (whose items can default to 1×1), or positive inventory dimensions plus an inventory action. Dimensions alone also occur on scenery and are insufficient.
- The audit includes unused inventory archetypes, mission model overrides, and downloadable pickups which can be held in VR.
- Fourteen weapon world models already use prepared held models (the psi amp uses its embedded authored hand); they do not need duplicate pickup poses.
- Abstract categories with no mesh cannot be fitted. Their concrete modeled descendants are covered. The model-less PsiSword archetype uses the existing `psword_h` grip.
- The six score-picture tiles needed a fitter correction: paired coplanar faces enclose no solid, and the candidate search must include grips near the edges of broad sheets.

## Newly covered models

| Model | Item names | Hands / scale |
| --- | --- | --- |
| `ammoaps` | AP Clip; Small AP Clip | Both / 0.55 |
| `ammoem` | EMP Grenade | Both / 0.55 |
| `ammohes` | HE Clip; Small HE Clip; clippy | Both / 0.55 |
| `ammoin` | Incend. Grenade | Both / 0.55 |
| `ammopx` | Prox. Grenade | Both / 0.55 |
| `ammosg` | Rifled Slug Box | Both / 0.55 |
| `ammosgp` | Pellet Shot Box | Both / 0.55 |
| `ammoti` | Timed Grenade | Both / 0.55 |
| `ammotx` | Toxin Grenade | Both / 0.55 |
| `anhegl` | Annelid_Medpatch | Both / 0.55 |
| `anpsor` | Annelid_Psipatch | Both / 0.55 |
| `arachor` | Arach. Organ | Both / 0.55 |
| `armore` | Reflec Armor | Both / 0.55 |
| `armorhe` | Light Armor | Both / 0.55 |
| `armorli` | Medium Armor | Both / 0.55 |
| `armorme` | Heavy Armor | Both / 0.55 |
| `armorva` | Vacc Suit | Both / 0.55 |
| `as` | Chem #11 | Both / 0.55 |
| `ba` | Chem #18 | Both / 0.55 |
| `battery` | Power Cell | Both / 0.55 |
| `batteryb` | Dead Power Cell | Both / 0.55 |
| `beaker1` | Small Beaker | Both / 0.55 |
| `beaker2` | Large Beaker | Both / 0.55 |
| `book` | Book_1 | Both / 0.55 |
| `botlech` | Champagne bottle | Both / 0.55 |
| `botleju` | BrightJuice; Juice bottle | Both / 0.55 |
| `botleli` | Liquor bottle | Both / 0.55 |
| `botlevo` | Vodka bottle | Both / 0.55 |
| `cardwh_1` | Sven_Card | Both / 0.55 |
| `cardwh_2` | Candy_Card | Both / 0.55 |
| `cardwh_3` | Nikki_Card | Both / 0.55 |
| `cardwh_4` | Lance_Card | Both / 0.55 |
| `cf` | Chem #7 | Both / 0.55 |
| `cheese` | Cheeseborger | Both / 0.55 |
| `chemif` | Inert Chemical #1 | Both / 0.55 |
| `chemif2` | Inert Chemical #2 | Both / 0.55 |
| `chemif3` | Inert Chemical #3 | Both / 0.55 |
| `chemif4` | Inert Chemical #4 | Both / 0.55 |
| `chemil` | Inert Chemical #5 | Both / 0.55 |
| `chemil2` | Inert Chemical #6 | Both / 0.55 |
| `chemil3` | Inert Chemical #7 | Both / 0.55 |
| `chemil4` | Inert Chemical #8 | Both / 0.55 |
| `chip_a` | Chip A | Both / 0.55 |
| `chip_b` | Chip B | Both / 0.55 |
| `chip_c` | Chip C | Both / 0.55 |
| `chips` | BrightChips; Chips | Both / 0.55 |
| `cigaret` | Cigarettes | Both / 0.55 |
| `circuit` | Circuitboard; RadKey Card | Both / 0.55 |
| `cs` | Chem #12 | Both / 0.55 |
| `cu` | Chem #6 | Both / 0.55 |
| `cue` | Pool Cue | Both / 0.55 |
| `disc03` | Audio Log; Manifest; PDA; PDA Soft | Both / 0.55 |
| `disrupt` | Big Bomb | Both / 0.55 |
| `empgub_w` | Broken EMP Gun | Both / 0.55 |
| `filter` | Anti-Annelid Toxin | Both / 0.55 |
| `fm` | Chem #1 | Both / 0.55 |
| `ga` | Chem #3 | Both / 0.55 |
| `gamecart` | Abyss Cart; Burro Hog Cart; Game Carts; Golf Cart; Hogger Cart; KaBacon Cart; Overworld Cart; Pig Stacker Cart; Ping Cart; Swine Hunter Cart; SwineHunter Cart; Swinekeeper; TTT Cart | Both / 0.55 |
| `grubor` | Grub Organ | Both / 0.55 |
| `hs` | Chem #13 | Both / 0.55 |
| `ir` | Chem #10 | Both / 0.55 |
| `keybed` | Med Bed Key | Both / 0.55 |
| `magapi` | This Month In Ping Pong | Both / 0.55 |
| `magaro` | Rolling Monthly | Both / 0.55 |
| `magdj` | DJ News | Both / 0.55 |
| `magka` | Kangaroo Quarterly | Both / 0.55 |
| `magvi` | Vita Men's Monthly | Both / 0.55 |
| `medanex` | Med Annex Key | Both / 0.55 |
| `medkit` | Medical Kit | Both / 0.55 |
| `medpass` | Med Card | Both / 0.55 |
| `midor` | Midwife Organ | Both / 0.55 |
| `mo` | Chem #15 | Both / 0.55 |
| `monbr` | Monkey_Brain | Both / 0.55 |
| `na` | Chem #8 | Both / 0.55 |
| `nanocan` | 1 Nanite; 10 Nanites; 20 Nanites; 5 Nanites; Big Nanite Pile; FakeNanites; Medium Nanite Pile; Nanites; Small Nanite Pile | Both / 0.55 |
| `organ` | OG Organ | Both / 0.55 |
| `os` | Chem #9 | Both / 0.55 |
| `overor` | Gr. Over. Organ; Mn. Over. Organ | Both / 0.55 |
| `patchx` | INT Boost | Both / 0.55 |
| `pillow` | Heart Pillow | Both / 0.55 |
| `plant1` | Plant #2 | Both / 0.55 |
| `portbatt` | Portable Battery | Both / 0.55 |
| `prism` | Large Prism; Small Prism | Both / 0.55 |
| `puzzle1` | Score Picture 1 | Both / 0.55 |
| `puzzle2` | Score Picture 2 | Both / 0.55 |
| `puzzle3` | Score Picture 3 | Both / 0.55 |
| `puzzle4` | Score Picture 4 | Both / 0.55 |
| `puzzle5` | Score Picture 5 | Both / 0.55 |
| `puzzle6` | Score Picture 6 | Both / 0.55 |
| `ra` | Chem #17 | Both / 0.55 |
| `reckey` | Rec Crew Key | Both / 0.55 |
| `recycler` | Recycler | Both / 0.55 |
| `ringbuoy` | Ring Buoy | Both / 0.55 |
| `rumbor` | Rumbler Organ | Both / 0.55 |
| `sb` | Chem #4 | Both / 0.55 |
| `se` | Chem #19 | Both / 0.55 |
| `sgb_w` | Broken Shotgun | Both / 0.55 |
| `soft11` | Hack Soft; Hack Soft V1 | Both / 0.55 |
| `soft12` | Hack Soft V2 | Both / 0.55 |
| `soft13` | Hack Soft V3 | Both / 0.55 |
| `soft21` | Repair Soft; Repair Soft V1 | Both / 0.55 |
| `soft22` | Repair Soft V2 | Both / 0.55 |
| `soft23` | Repair Soft V3 | Both / 0.55 |
| `soft31` | Modify Soft; Modify Soft V1 | Both / 0.55 |
| `soft32` | Modify Soft V2 | Both / 0.55 |
| `soft33` | Modify Soft V3 | Both / 0.55 |
| `soft41` | Research Soft; Research Soft V1 | Both / 0.55 |
| `soft42` | Research Soft V2 | Both / 0.55 |
| `soft43` | Research Soft V3 | Both / 0.55 |
| `spdpatch` | Speed Boost | Both / 0.55 |
| `strpatch` | Strength Boost | Both / 0.55 |
| `swarmor` | Swarm Organ | Both / 0.55 |
| `tc` | Chem #16 | Both / 0.55 |
| `te` | Chem #14 | Both / 0.55 |
| `upgrade` | 1 EXP; 10 EXP; Big BP Pile; EXP Cookies; Encrypted EXP Cookie; FakeCookie; Medium BP Pile; Small BP Pile | Both / 0.55 |
| `v` | Chem #2 | Both / 0.55 |
| `wormskin` | Worm Skin | Both / 0.55 |
| `yt` | Chem #5 | Both / 0.55 |
| `zapcan` | Soda Can | Both / 0.55 |

## Explicit beaker refits

| Model | Item | Result |
| --- | --- | --- |
| `beaker1` | Small empty beaker | New left/right poses, 0.55 |
| `beaker2` | Large empty beaker | New left/right poses, 0.55 |
| `beakew1` | Small worm beaker | Replaced left/right poses, 0.55 |
| `beakew2` | Large worm beaker | Replaced left/right poses, 0.55 |

## Reproduce

Run from the repository root with the remaster data installed:

```sh
cargo dq inventory-audit --all-missions > /tmp/inventory-audit.json
cargo run -p debug_runtime --example fit_inventory_grips -- \
  --models beaker1,beaker2,beakew1,beakew2 \
  --refit beaker1,beaker2,beakew1,beakew2 --scale 0.55 \
  --output /tmp/refitted-beakers.json
```

The fitter fills only missing poses unless a model is explicitly listed in `--refit`. It reads `assets/vr-grips.json`, uses the production glove rig and `GripSurface` auto-fit path, and writes a separate output file. It refuses to overwrite an existing output and reports any models that could not be fitted. Inspect the output before replacing the source library.
