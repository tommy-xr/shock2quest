import assert from "node:assert/strict";
import { test } from "node:test";

import { HttpClient } from "../src/client.js";
import { actionPart, channelPart, Game, recordClipInputs } from "../src/index.js";

/** A runtime stand-in: every step advances the frames asked for. */
class FakeClient extends HttpClient {
  constructor() {
    super("http://unused.invalid");
  }
  override async post<T>(path: string, body?: unknown): Promise<T> {
    if (path === "/v1/step") return { frames_advanced: (body as { frames: number }).frames } as T;
    return { success: true, message: "", data: null } as T;
  }
}

test("channelPart maps Touch channels and excludes tracking", () => {
  assert.equal(channelPart("right_hand.thumbstick"), "R.stick");
  assert.equal(channelPart("left_hand.trigger_value"), "L.trigger");
  assert.equal(channelPart("left_hand.squeeze"), "L.grip");
  for (const channel of ["left_hand.position", "right_hand.rotation", "right_hand.world_target", "head.look", "jump", "left_hand.a"]) {
    assert.equal(channelPart(channel), null, channel);
  }
});

test("actionPart maps the raw face and menu buttons only", () => {
  assert.equal(actionPart("LeftHandLowerButton"), "L.lower");
  assert.equal(actionPart("RightHandUpperButton"), "R.upper");
  assert.equal(actionPart("MenuButton"), "L.menu");
  assert.equal(actionPart("Jump"), null);
});

test("recorder stamps inputs with sim time from start()", async () => {
  const game = new Game(new FakeClient());
  const recorder = recordClipInputs(game);
  // Setup before the clip: a held grip carries over to t = 0.
  await game.input.set("left_hand.squeeze", 1);
  await game.input.set("left_hand.position", [0, 1, 0]);
  await game.step({ frames: 30 });
  recorder.start();

  await game.input.set("right_hand.thumbstick", [0, 1]);
  await game.step({ frames: 12 });
  await game.input.set("right_hand.thumbstick", [0, 1]); // unchanged: no keyframe
  await game.input.trigger("LeftHandLowerButton");
  await game.step({ frames: 12 });
  await game.input.set("right_hand.thumbstick", [0.5, 0]);
  await game.input.set("crouch", 1);
  await game.step({ frames: 60 });
  recorder.dispose();
  await game.step({ frames: 60 }); // after dispose: not counted

  assert.deepEqual(recorder.timeline(), {
    format: "shock2quest-clip-inputs/1",
    duration: 84 / 60,
    parts: {
      "L.grip": [[0, 1]],
      "R.stick": [[0, [0, 1]], [0.4, [-0.5, 0]]],
      "L.lower": [[0.2, 1], [0.4, 0]],
      "L.click": [[0.4, 1], [0.6, 0]],
    },
  });
});

test("a press repeated within the press window stays down", async () => {
  const game = new Game(new FakeClient());
  const recorder = recordClipInputs(game);
  await game.input.trigger("RightHandUpperButton");
  await game.step({ frames: 12 });
  await game.input.trigger("RightHandUpperButton");
  assert.deepEqual(recorder.timeline().parts["R.upper"], [[0, 1], [0.4, 0]]);
});

test("a press while held stays held until released", async () => {
  const game = new Game(new FakeClient());
  const recorder = recordClipInputs(game);
  await game.input.hold("MenuButton");
  await game.step({ frames: 12 });
  await game.input.trigger("MenuButton");
  await game.step({ frames: 60 });
  await game.input.release("MenuButton");
  assert.deepEqual(recorder.timeline().parts["L.menu"], [[0, 1], [1.2, 0]]);
});

test("an input shows from the first video frame captured after it", async () => {
  // 15 fps: a screenshot after sim steps 1, 5, 9, ... (video frames 0, 1, 2).
  const game = new Game(new FakeClient());
  const recorder = recordClipInputs(game);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 1); // first in the step-5 frame
  await game.step({ frames: 4 });
  await game.input.set("right_hand.trigger", 0); // first in the step-9 frame
  assert.deepEqual(recorder.timeline().parts["R.trigger"], [[0.0666, 1], [0.1333, 0]]);
});
