# Earth containment experiment

Run `cargo dbgr --mission earth_horde` (or launch that mission in the desktop runtime). `earth_horde_test` uses three short waves for iteration. The aliases load Earth geometry; ordinary `earth.mis` keeps its original behavior.

Eight escalating waves have about 23 minutes of minimum scheduled combat and rest. Clearing enemies can take longer. After the final wave, use the ready button beside the shops to begin optional endless play. The same button skips a rest. Doors are closed and locked and the training trigger graph is disabled.

The starter backpack contains a wrench, pistol, psi amp, ammunition and medical/psi supplies. Shops are on the street, trainers upstairs. Buy psi tiers and individual powers separately; only powers supported by the current runtime are sold. Replicated items dispense in front of the machines. Random equipment and currency supplement normal enemy loot and wave rewards.

Corpses and their remaining contents stay lootable throughout the rest. Starting the next wave removes them; items already collected survive. Run state, rewards and purchased powers persist in saves.

This is the first playable slice. Circulators, Toxin-A replenishment/infestation, eggs, optional ladders and day/night lighting are not implemented yet. Toxin-A is stocked in preparation for the ecology increment. Balance is experimental; an independent authentic playtest reached a wave-one death after killing a hybrid, confirming combat and stair traversal but not a full-run completion.

Validation includes director unit tests for timing, final-wave/endless gating, rewards, corpse cleanup and serialization; psi purchase quote tests; and debug-runtime purchase/save checks. The short alias is a diagnostic aid, not evidence that a full-length run has been completed.
