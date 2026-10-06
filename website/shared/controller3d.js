// Meta Quest Touch Plus pair rendered from the WebXR Input Profiles models
// (MIT, @webxr-input-profiles/assets). A mode's controls glow in `accent` and
// animate through their authored press/tilt range - replaying the clip's
// recorded inputs when it has them; drag to turn the pair.
// Pages need an import map for "three" and "three/addons/".
import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { sampleInputs } from "./inputs.js";

const PROFILE = "https://cdn.jsdelivr.net/npm/@webxr-input-profiles/assets@1.0/dist/profiles/meta-quest-touch-plus/";

// Mode part name -> glTF component root, per hand.
const ROOTS = {
  L: { stick: "xr_standard_thumbstick", trigger: "xr_standard_trigger", grip: "xr_standard_squeeze", upper: "y_button", lower: "x_button" },
  R: { stick: "xr_standard_thumbstick", trigger: "xr_standard_trigger", grip: "xr_standard_squeeze", upper: "b_button", lower: "a_button" },
};

export function createControllerView(container, { accent = "#39e1e6" } = {}) {
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  let renderer;
  try {
    renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  } catch {
    container.textContent = "3D controller view needs WebGL.";
    return { setMode() {}, setClip() {} };
  }
  renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 0.8;
  container.appendChild(renderer.domElement);
  renderer.domElement.style.cssText = "display:block;width:100%;height:100%;touch-action:pan-y;cursor:grab";

  // The Touch Plus profile omits Menu from its model. Show its state explicitly
  // beside the pair, using the same mode/recording timing as the modeled buttons.
  const menu = document.createElement("span");
  menu.dataset.controllerInput = "L.menu";
  menu.textContent = "LEFT MENU";
  menu.hidden = true;
  menu.style.cssText = "position:absolute;left:12px;bottom:12px;padding:5px 8px;border:1px solid currentColor;background:#09242b;font:600 11px monospace;letter-spacing:1px;pointer-events:none";
  menu.style.color = accent;
  container.style.position = "relative";
  container.appendChild(menu);

  const scene = new THREE.Scene();
  scene.environment = new THREE.PMREMGenerator(renderer).fromScene(new RoomEnvironment(), 0.04).texture;
  const key = new THREE.DirectionalLight(0xffffff, 1.4);
  key.position.set(0.3, 1, 0.6);
  scene.add(key);

  const camera = new THREE.PerspectiveCamera(30, 1, 0.01, 10);
  // Above and slightly behind: the view down onto your own hands.
  camera.position.set(0, 0.32, 0.09);
  camera.lookAt(0, -0.03, 0.02);

  const pair = new THREE.Group();
  scene.add(pair);
  const glow = new THREE.Color(accent);
  const hands = {}; // hand -> { parts: {part: {meshes, responses}} }
  let mode = null;
  let clip = null; // { video, timeline } while the mode's clip has recorded inputs

  const loader = new GLTFLoader();
  const load = (hand, file, x) =>
    loader.loadAsync(PROFILE + file).then((gltf) => {
      const root = gltf.scene;
      root.position.x = x;
      root.rotation.z = hand === "L" ? 0.12 : -0.12;
      pair.add(root);
      const parts = {};
      for (const [part, name] of Object.entries(ROOTS[hand])) {
        // Component meshes hang under the button's "<name>_pressed_value" node.
        const node = root.getObjectByName(`${name}_pressed_value`);
        if (!node) continue;
        const meshes = [];
        node.traverse((o) => {
          if (!o.isMesh) return;
          o.userData.base = o.material;
          o.userData.hot = Object.assign(o.material.clone(), { map: null, color: glow.clone(), emissive: glow.clone(), emissiveIntensity: 0.9 });
          meshes.push(o);
        });
        // Each visual response is a value node moved between authored min/max nodes.
        const keys = part === "stick" ? [`${name}_xaxis_pressed`, `${name}_yaxis_pressed`, `${name}_pressed`] : [`${name}_pressed`];
        const responses = keys
          .map((k) => ({ value: root.getObjectByName(`${k}_value`), min: root.getObjectByName(`${k}_min`), max: root.getObjectByName(`${k}_max`) }))
          .filter((r) => r.value && r.min && r.max)
          .map((r) => ({ ...r, rest: { p: r.value.position.clone(), q: r.value.quaternion.clone() } }));
        parts[part] = { meshes, responses };
      }
      hands[hand] = { parts };
      apply();
    });

  function apply() {
    menu.hidden = !mode?.hot.L.includes("menu");
    for (const [hand, { parts }] of Object.entries(hands)) {
      const hot = new Set(mode ? mode.hot[hand] : []);
      for (const [part, { meshes, responses }] of Object.entries(parts)) {
        const on = hot.has(part);
        meshes.forEach((m) => (m.material = on ? m.userData.hot : m.userData.base));
        // Hot parts are re-posed by the next frame; a replayed stick click is not.
        responses.forEach((r) => { r.value.position.copy(r.rest.p); r.value.quaternion.copy(r.rest.q); });
      }
    }
  }

  // Blend a value node between its min/max poses; t in [0, 1].
  const pose = (r, t) => {
    r.value.position.lerpVectors(r.min.position, r.max.position, t);
    r.value.quaternion.slerpQuaternions(r.min.quaternion, r.max.quaternion, t);
  };

  // Pose every part as the clip's inputs had it at the video's current time;
  // a part glows while pressed.
  const replay = ({ video, timeline }) => {
    const state = sampleInputs(timeline, video.currentTime);
    menu.hidden = !state.L.menu?.pressed;
    for (const [hand, { parts }] of Object.entries(hands)) {
      for (const [part, { meshes, responses }] of Object.entries(parts)) {
        const s = state[hand][part];
        responses.forEach((r, i) => {
          if (s) pose(r, s.pose[i]);
          else { r.value.position.copy(r.rest.p); r.value.quaternion.copy(r.rest.q); }
        });
        meshes.forEach((m) => {
          m.userData.hot.emissiveIntensity = 0.9; // the idle animation pulses it
          m.material = s?.pressed ? m.userData.hot : m.userData.base;
        });
      }
    }
  };

  // Drag to turn; otherwise a slow idle sway.
  let yaw = 0, drag = null, lastInput = -1e9;
  renderer.domElement.addEventListener("pointerdown", (e) => { drag = { x: e.clientX, yaw }; renderer.domElement.setPointerCapture(e.pointerId); });
  renderer.domElement.addEventListener("pointermove", (e) => { if (drag) { yaw = drag.yaw + (e.clientX - drag.x) * 0.01; lastInput = performance.now(); } });
  for (const t of ["pointerup", "pointercancel"]) renderer.domElement.addEventListener(t, () => (drag = null));

  const resize = () => {
    const { width, height } = container.getBoundingClientRect();
    if (!width || !height) return;
    renderer.setSize(width, height, false);
    camera.aspect = width / height;
    // Keep both controllers in frame on narrow containers.
    camera.fov = camera.aspect < 1.6 ? 30 * (1.6 / camera.aspect) ** 0.8 : 30;
    camera.updateProjectionMatrix();
  };
  new ResizeObserver(resize).observe(container);
  resize();

  const frame = (ms) => {
    const t = ms / 1000;
    const idle = performance.now() - lastInput > 2500 && !drag;
    if (idle && !reduce) yaw += (Math.sin(t * 0.4) * 0.25 - yaw) * 0.02;
    pair.rotation.y = yaw;
    if (mode && !reduce && clip) replay(clip);
    else if (mode && !reduce) {
      for (const [hand, { parts }] of Object.entries(hands)) {
        for (const part of mode.hot[hand]) {
          const p = parts[part];
          if (!p) continue;
          if (part === "stick") { pose(p.responses[0], (Math.cos(t * 2.4) + 1) / 2); if (p.responses[1]) pose(p.responses[1], (Math.sin(t * 2.4) + 1) / 2); }
          else p.responses.forEach((r) => pose(r, Math.max(0, Math.sin(t * 3.2))));
          const k = 0.55 + 0.45 * Math.sin(t * 3.2);
          p.meshes.forEach((m) => (m.material.emissiveIntensity = k));
        }
      }
    }
    renderer.render(scene, camera);
  };
  // Render only while on screen.
  const onScreen = new IntersectionObserver(([e]) => renderer.setAnimationLoop(e.isIntersecting ? frame : null));
  onScreen.observe(container);

  Promise.all([load("L", "left.glb", -0.065), load("R", "right.glb", 0.065)]).catch((e) => {
    onScreen.disconnect();
    renderer.setAnimationLoop(null);
    container.textContent = "Controller model unavailable offline.";
    console.error(e);
  });

  return {
    setMode(m) { mode = m; clip = null; apply(); },
    // Drive the pair from `timeline` (see inputs.js) at `video`'s playhead.
    setClip(video, timeline) { clip = { video, timeline }; },
  };
}
