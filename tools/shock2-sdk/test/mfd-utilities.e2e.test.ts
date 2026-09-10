import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdir, writeFile } from "node:fs/promises";
import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { clickUiElement, clickCanvasWithRay, canvasCenter, requirePanelPose } from "./helpers/ui.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  test(`MFD item information is read-only and cancellable (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({mission: "debug_interactions", debugFlags: vr ? ["--vr"] : []});
    await game.step({frames: 30});
    const hypo = await game.player.spawnItem(-52);
    await game.input.trigger("ToggleUseMode");
    await game.step({frames: 5});
    const output = process.env.ASTRA_MFD_CAPTURE;
    const capture = async (name: string) => {
      if (!output) return;
      await mkdir(output, {recursive: true});
      const path = `${output}/${vr ? "vr" : "flat"}-${name}`;
      await game.screenshot(`${path}.png`, 1600);
      await writeFile(`${path}.json`, JSON.stringify({ui: await game.ui.state(), inventory: await game.player.inventory(), info: await game.info()}, null, 2));
    };
    const elements = async () => (await game.ui.state()).strip!.elements;
    const control = async (label: string) => {
      const element = (await elements()).find(e => e.label === label);
      assert.ok(element, `${label} exists`);
      return element;
    };
    const click = async (element: UiElement) => {
      if (vr) await clickCanvasWithRay(game, requirePanelPose(await game.ui.state()), canvasCenter(element));
      else await clickUiElement(game, element);
    };
    const before = await game.player.inventory();
    const cursor = (await game.ui.state()).cursor;
    await capture("idle");
    await click(await control("inspect"));
    await capture("select");
    await click(await control("inspect"));
    assert.equal((await elements()).some(e => e.label === "utility_close"),false,"second ? cancels selection");
    await click(await control("inspect"));
    const item = (await elements()).find(e => e.entity_id === hypo.entity_id && e.kind === "button");
    assert.ok(item);
    await click(item);
    const text = (await elements()).filter(e => e.label === "utility_text");
    assert.ok(text.length > 0);
    assert.doesNotMatch(text.map(e=>e.text).join(" "), /No description available/i, "authored hypo description resolves");
    assert.ok(text.every(e => e.entity_id === hypo.entity_id),"description belongs to selected hypo");
    await capture("description");
    const next = (await elements()).find(e => e.label === "utility_next");
    assert.ok(next, "medical hypo description exercises pagination");
    if (next) {
      const first = text.map(e=>e.text);
      await click(next);
      assert.notDeepEqual((await elements()).filter(e=>e.label === "utility_text").map(e=>e.text),first);
      await capture("page-2");
      await click(await control("utility_previous"));
      assert.deepEqual((await elements()).filter(e=>e.label === "utility_text").map(e=>e.text),first);
    }
    await click(await control("utility_close"));
    assert.equal((await elements()).some(e=>e.label === "utility_close"),false);
    assert.deepEqual(await game.player.inventory(),before,"inspection does not consume or move hypo");
    assert.deepEqual((await game.ui.state()).cursor,cursor,"inspection leaves cursor item unchanged");
    await capture("closed");
  });
}


test("VR inspection reserves an off-panel held hypo trigger through cancel until release", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({mission:"debug_interactions",debugFlags:["--vr"]});
  await game.step({frames:30});
  const hypo = (await game.entities.list()).entities.find(e=>e.template_id === -52);
  assert.ok(hypo);
  await aimVrHandAt(game,hypo.position,.2,1,0,{hand:"right"});
  assert.equal((await game.info()).player.right_hand_entity_id,hypo.id);
  await game.input.set("right_hand.position",[.8,1,-.2]);
  await game.input.set("right_hand.rotation",[0,0,0,1]);
  await game.input.set("head.rotation",[0,0,0,1]);
  await game.input.trigger("ToggleUseMode");
  await game.step({frames:5});
  const stack = (await game.entities.detail(hypo.id)).properties.find(p=>p.name === "StackCount");
  assert.ok(stack);
  const health = (await game.info()).player.hit_points;
  const inspect = (await game.ui.state()).strip!.elements.find(e=>e.label === "inspect")!;
  const [x,y,w,h] = inspect.rect;
  await aimVrHandAtCanvas(game,(await game.ui.state()).panel_pose!,[x+w/2,y+h/2],{hand:"left",squeeze:0});
  await game.input.set("left_hand.trigger",1);
  await game.step({frames:2});
  await game.input.set("left_hand.trigger",0);
  await game.step({frames:2});
  assert.ok((await game.ui.state()).strip!.elements.some(e=>e.label === "utility_close"));
  // The holding hand points outside every panel. Its trigger must still be reserved.
  await game.input.set("right_hand.rotation",[0,1,0,0]);
  await game.input.set("right_hand.trigger",1);
  await game.step({frames:5});
  assert.equal((await game.info()).player.right_hand_entity_id,hypo.id);
  assert.equal((await game.info()).player.hit_points,health);
  assert.deepEqual((await game.entities.detail(hypo.id)).properties.find(p=>p.name === "StackCount"),stack,"held hypo properties/stack remain unchanged");
  // Closing the interface cancels inspection while the held trigger remains down.
  await game.input.trigger("ToggleUseMode");
  await game.step({frames:5});
  assert.equal((await game.info()).player.right_hand_entity_id,hypo.id);
  assert.equal((await game.info()).player.hit_points,health);
  assert.deepEqual((await game.entities.detail(hypo.id)).properties.find(p=>p.name === "StackCount"),stack);
  await game.input.set("right_hand.trigger",0);
  await game.step({frames:3});
  assert.equal((await game.info()).player.right_hand_entity_id,hypo.id,"releasing reserved trigger does not consume hypo");
});


for (const vr of [false,true]) {
  test(`Research overview reads existing projects without starting research (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout:180_000,
  }, async(t)=>{
    await using game = await GameServer.launch({mission:"debug_interactions",debugFlags:vr?["--vr"]:[]});
    t.after(async()=>{await writeFile(`/tmp/astra-mfd-research-${vr?"vr":"flat"}-runtime.log`,game.logs().join("\n"));});
    await game.step({frames:30});
    await game.player.setStats({skills:{research:6}});
    const toxin=await game.player.spawnItem(-1341);
    await game.input.trigger("ToggleUseMode");await game.step({frames:5});
    const elements=async()=>(await game.ui.state()).strip!.elements;
    const text=async()=>(await elements()).filter(e=>e.label === "utility_text").map(e=>e.text).join(" ");
    const click=async(e:UiElement)=>{
      if(vr) await clickCanvasWithRay(game,requirePanelPose(await game.ui.state()),canvasCenter(e));
      else await clickUiElement(game,e);
    };
    const control=async(label:string)=>{const e=(await elements()).find(e=>e.label===label);assert.ok(e,label);return e;};
    const capture=async(name:string)=>{
      const output=process.env.ASTRA_RESEARCH_CAPTURE;if(!output)return;
      await mkdir(output,{recursive:true});const path=`${output}/${vr?"vr":"flat"}-${name}`;
      await game.screenshot(`${path}.png`,1600);await writeFile(`${path}.json`,JSON.stringify({ui:await game.ui.state(),inventory:await game.player.inventory(),info:await game.info()},null,2));
    };
    const inventory=await game.player.inventory();
    await capture("idle");
    await click(await control("research_overview"));
    assert.match(await text(),/No research projects yet/);
    await capture("empty");
    await game.step({frames:75});
    assert.match(await text(),/No research projects yet/,"overview must not start research on carried Toxin-A");
    assert.deepEqual(await game.player.inventory(),inventory);
    await click(await control("utility_close"));
    const item=(await elements()).find(e=>e.entity_id===toxin.entity_id&&e.kind==="button");assert.ok(item);
    await click(item);await click(item);await game.step({frames:75});
    const active=(await game.ui.state()).active_panel;assert.ok(active,"real item use opens research MFD");
    assert.ok(active.elements.some(e=>e.text?.includes("Antimony")),"real research reaches authored chemical gate");
    const reports=active.elements.find(e=>e.label==="Research reports");assert.ok(reports);
    // The flat case covers the actual Reports button; VR enters through RES.
    // Legacy world-panel proxy targeting is not asserted by this scenario.
    await click(vr ? await control("research_overview") : reports);
    await capture("reports-button-result");
    assert.ok((await elements()).some(e=>e.texture?.toLowerCase()==="iface/pda.pcx"));
    await capture("journal-list");
    await click(await control("research_entry:0"));
    assert.ok((await elements()).some(e=>e.texture?.toLowerCase()==="iface/research.pcx"));
    const gate=await text();assert.match(gate,/chemical/i);assert.match(gate,/Antimony/);assert.match(gate,/5\.0 %/,"authored progress retains its percent glyph");
    await capture("chemical-gate");
    await game.step({frames:60});assert.equal(await text(),gate,"read-only journal leaves chemically blocked progress unchanged");
    assert.deepEqual(await game.player.inventory(),inventory,"journal never consumes research specimen");
    await click(await control("research_back"));
    assert.ok((await elements()).some(e=>e.label==="research_entry:0"));
    await click(await control("utility_close"));
    // Real research progress, accelerated only through the supported skill debug setting.
    const brain=await game.player.spawnItem(-148);
    const brainButton=(await elements()).find(e=>e.entity_id===brain.entity_id&&e.kind==="button");assert.ok(brainButton);
    await click(brainButton);await click(brainButton);await game.step({frames:1000});
    assert.ok((await game.ui.state()).active_panel!.elements.some(e=>e.text?.includes("Fermium")),"Monkey Brain reaches its authored Fermium gate");
    const fermium=await game.player.spawnItem(-20);
    const chemical=(await elements()).find(e=>e.entity_id===fermium.entity_id&&e.kind==="button");assert.ok(chemical);
    await click(chemical);await click(chemical);await game.step({frames:3600});
    assert.ok(!(await game.player.inventory()).items.some(e=>e.entity_id===fermium.entity_id),"real research consumes required Fermium");
    assert.ok((await game.ui.state()).active_panel!.elements.some(e=>e.text?.includes("Research complete")),"Monkey Brain research completes");
    await click(await control("research_overview"));
    await capture("completed-list");
    const rows=(await elements()).filter(e=>e.label?.startsWith("research_entry:"));
    assert.equal(rows.length,2,"suspended toxin and completed brain report");
    const row=rows[1]!;
    await click(row);
    assert.ok((await elements()).some(e=>e.texture?.toLowerCase()==="iface/resrep.pcx"),"completed selection uses retail report artwork");
    assert.ok((await elements()).filter(e=>/^(mport|resicon)\.pcx$/i.test(e.texture??"")).length>=2,"portrait and specimen icon");
    const completedInventory=await game.player.inventory();
    const first=await text();assert.doesNotMatch(first,/No written report/);assert.match(first,/25%/,"authored report bonus retains percent");assert.doesNotMatch(first,/\.\.\.|…/,"wrapped report must not discard text through ellipsis");
    await capture("completed-report");
    await click(await control("utility_next"));
    assert.notEqual(await text(),first);await capture("completed-page-2");
    await click(await control("utility_previous"));assert.equal(await text(),first);
    assert.deepEqual(await game.player.inventory(),completedInventory,"reading report does not mutate inventory");
    await click(await control("research_back"));
    await capture("completed-list");
  });
}

for (const vr of [false,true]) {
  test(`MAP utility opens the real mission automap and preserves inventory (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout:180_000,
  }, async()=>{
    await using game=await GameServer.launch({mission:"medsci1.mis",debugFlags:vr?["--vr"]:[]});
    await game.step({frames:30});
    await game.player.spawnItem(-52);
    await game.input.trigger("ToggleUseMode");await game.step({frames:5});
    const inventory=await game.player.inventory();
    const click=async(e:UiElement)=>{
      if(vr)await clickCanvasWithRay(game,requirePanelPose(await game.ui.state()),canvasCenter(e));
      else await clickUiElement(game,e);
    };
    const mapControl=async()=>{const e=(await game.ui.state()).strip!.elements.find(e=>e.label==="map");assert.ok(e);return e;};
    const capture=async(name:string)=>{
      const out=process.env.ASTRA_MAP_CAPTURE;if(!out)return;await mkdir(out,{recursive:true});
      const path=`${out}/${vr?"vr":"flat"}-${name}`;await game.screenshot(path+".png",1600);
      await writeFile(path+".json",JSON.stringify({ui:await game.ui.state(),info:await game.info(),inventory:await game.player.inventory()},null,2));
    };
    const idleUi=await game.ui.state();
    assert.ok(idleUi.readout_elements.some(e=>e.texture?.toUpperCase()==="AMMOFULL.PCX"),"empty-handed use mode retains expanded ammo frame");
    for (const [label,texture] of [["inspect","iface/ifbtn30.pcx"],["research_overview","iface/ifbtn40.pcx"],["map","iface/ifbtn50.pcx"]]) {
      const button=idleUi.strip!.elements.find(e=>e.label===label);assert.ok(button,label);
      assert.equal(button.texture?.toLowerCase(),texture,"native navigation artwork");
    }
    await capture("idle");
    await click(await mapControl());
    const panel=(await game.ui.state()).active_panel;assert.ok(panel);
    for(const texture of ["mapback.pcx","page001.pcx","plrpip.pcx"]){assert.ok(panel.elements.some(e=>e.texture?.toLowerCase().endsWith(texture)),texture);}
    assert.ok(panel.elements.some(e=>/p001r\d{3}\.pcx$/i.test(e.texture??"")),"current explored room decal");
    for(const e of panel.elements){const [x,y,w,h]=e.rect;assert.ok(x>=0&&x+w<=640.01&&y>=0&&y+h<=480.01,`${e.label??e.texture} remains in canvas`);}
    await capture("open");
    await click(await mapControl());
    assert.equal((await game.ui.state()).active_panel,null,"MAP toggles closed");
    await capture("closed");
    await click(await mapControl());
    const close=(await game.ui.state()).active_panel!.elements.find(e=>e.label==="close");assert.ok(close,"map has close control");
    await click(close);
    assert.equal((await game.ui.state()).active_panel,null,"map close control works");
    assert.deepEqual(await game.player.inventory(),inventory);
    const item=(await game.ui.state()).strip!.elements.find(e=>e.kind==="button"&&e.entity_id===inventory.items[0]!.entity_id);assert.ok(item);
    await click(item);
    const carried=(await game.ui.state()).cursor;assert.ok(carried,"inventory click carries the hypo on the UI cursor");
    const emptyFrame=(await game.ui.state()).readout_elements.find(e=>e.texture?.toUpperCase()==="AMMOFULL.PCX");assert.ok(emptyFrame);
    await click(emptyFrame);
    assert.deepEqual((await game.ui.state()).cursor,carried,"blank ammo strip chrome must not throw the cursor item");
    await click(item);
    assert.equal((await game.ui.state()).cursor,null);
    assert.deepEqual(await game.player.inventory(),inventory,"returning cursor item preserves inventory");
  });
}

for (const vr of [false,true]) {
  test(`Native logs control keeps cyber open and resources follow real awards (${vr ? "VR" : "flat"})`, {
    skip:process.env.SHOCK2_E2E!=="1",timeout:180_000,
  },async()=>{
    await using game=await GameServer.launch({mission:"medsci1.mis",debugFlags:vr?["--vr"]:[]});
    await game.step({frames:30});await game.player.spawnItem(-52);
    await game.input.trigger("ToggleUseMode");await game.step({frames:5});
    const click=async(e:UiElement)=>{if(vr)await clickCanvasWithRay(game,requirePanelPose(await game.ui.state()),canvasCenter(e));else await clickUiElement(game,e);};
    const logs=async()=>{const e=(await game.ui.state()).readout.find(e=>e.label==="logs");assert.ok(e);return e;};
    const capture=async(name:string)=>{const out=process.env.ASTRA_LOG_CAPTURE;if(!out)return;await mkdir(out,{recursive:true});const path=`${out}/${vr?"vr":"flat"}-${name}`;await game.screenshot(path+".png",1600);await writeFile(path+".json",JSON.stringify({ui:await game.ui.state(),info:await game.info(),inventory:await game.player.inventory()},null,2));};
    const inventory=await game.player.inventory();
    await capture("idle");await click(await logs());
    assert.equal((await game.ui.state()).mode,"use");assert.equal((await game.ui.state()).active_panel,null);
    assert.ok((await game.ui.state()).strip!.elements.some(e=>e.label==="utility_text"&&e.text?.includes("No collected logs")),"empty logs render shared PDA feedback");
    await capture("empty-logs");
    await click(await logs());
    assert.equal((await game.ui.state()).mode,"use");
    assert.ok(!(await game.ui.state()).strip!.elements.some(e=>e.label==="utility_text"&&e.text?.includes("No collected logs")),"second click dismisses empty PDA only");
    const [disc]=await game.entities.byTemplate(1608);assert.ok(disc);
    await game.entities.sendMessage(disc.id,{type:"Frob"});await game.step({frames:5});
    await click(await logs());
    const reader=(await game.ui.state()).active_panel;assert.ok(reader);
    assert.ok(reader.elements.some(e=>e.text?.includes("45100")),"native button opens authored Amanpour transcript");
    await capture("reader");await click(await logs());
    assert.equal((await game.ui.state()).active_panel,null,"button closes only the reader");assert.equal((await game.ui.state()).mode,"use");
    const before=(await game.info()).player.stats!;
    const nanites=(await game.entities.list({filter:"Nanite",limit:100})).entities;
    let awarded=0;
    for(const e of nanites){const p=(await game.entities.detail(e.id)).properties;const count=Number(p.find(p=>p.name==="StackCount")?.value??0);if(count>0){await game.entities.sendMessage(e.id,{type:"Frob"});await game.step({frames:3});awarded=count;break;}}
    assert.ok(awarded>0,"authored nanite pickup");assert.equal((await game.info()).player.stats!.nanites,before.nanites+awarded);
    const traps=(await game.entities.list({filter:"Experience Trap"})).entities;let experience=0;
    for(const e of traps){const value=Number((await game.entities.detail(e.id)).properties.find(p=>p.name==="Exp")?.value??0);if(value>0){await game.entities.sendMessage(e.id,{type:"TurnOn"});await game.step({frames:3});experience=value;break;}}
    assert.ok(experience>0);assert.equal((await game.info()).player.stats!.cyber_modules,before.cyber_modules+experience);
    const ui=await game.ui.state();
    for(const [x,value] of [[185,before.nanites+awarded],[224,before.cyber_modules+experience]]){assert.ok(ui.readout_elements.some(e=>e.kind==="text"&&Math.abs(e.rect[0]-x)<.01&&e.text===String(value)),"resource total matches awarded currency");}
    await capture("awarded");assert.deepEqual(await game.player.inventory(),inventory,"resource awards do not become inventory items");
  });
}
