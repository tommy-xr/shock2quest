// Page content: copy, links, control-scheme data.

// Parts per hand: stick, trigger, grip, upper (Y/B), lower (X/A), menu (left only).
// `clip` is gameplay footage for the mode; null until it's recorded.
export const MODES = [
  {
    id: "move",
    title: "Movement",
    blurb: "Smooth locomotion. Crouch for real, or click the stick.",
    hot: { L: ["stick", "lower"], R: ["stick", "lower"] },
    clip: null,
    rows: [
      ["Right stick", "Walk / strafe"],
      ["Left stick ← →", "Turn"],
      ["Left stick click", "Toggle crouch (or just crouch)"],
      ["X / A", "Jump — whatever your hands hold"],
    ],
  },
  {
    id: "climb",
    title: "Climbing",
    blurb: "Ladders and ledges are climbed hand over hand.",
    hot: { L: ["grip"], R: ["grip"] },
    clip: null,
    rows: [
      ["Grip near a ladder", "Hold on — a cyan marker confirms"],
      ["Pull toward chest", "Climb"],
      ["Grip the deck, pull down", "Mantle over the top"],
      ["Open hand", "Let go"],
    ],
  },
  {
    id: "weapon",
    title: "Weapons",
    blurb: "Aim down real sights. Reload by hand.",
    hot: { L: ["grip"], R: ["trigger", "grip", "upper"] },
    clip: "rec1.gif",
    rows: [
      ["Grip", "Hold weapon (off-hand steadies two-handers)"],
      ["Trigger", "Fire"],
      ["Y / B tap", "Switch fire mode"],
      ["Y / B hold", "Drop the loaded clip"],
      ["Free hand: clip → gun", "Reload"],
      ["Y / B holding a clip", "Swap ammo type"],
    ],
  },
  {
    id: "psi",
    title: "Psi Amp",
    blurb: "Two powers at your fingertip, the rest on a carousel.",
    hot: { L: [], R: ["trigger", "upper", "stick"] },
    clip: "ops2.gif",
    rows: [
      ["Trigger", "Cast"],
      ["Y / B tap", "Swap current ↔ alternate power"],
      ["Y / B hold", "Open the power carousel"],
      ["Stick ↑↓ / ←→", "Tier / power"],
      ["Trigger", "Confirm"],
    ],
  },
  {
    id: "mfd",
    title: "MFD",
    blurb: "Jack in: inventory, map, logs and stats float in front of you.",
    hot: { L: ["menu", "upper"], R: ["trigger", "grip", "upper"] },
    clip: null,
    rows: [
      ["Menu tap", "Jack in / out"],
      ["Menu hold", "Pause"],
      ["Point + trigger", "Select"],
      ["Grip, point, release", "Move an item — ghost shows the fit"],
      ["Y / B, free hand", "Play newest audio log"],
    ],
  },
  {
    id: "belt",
    title: "Toolbelt",
    blurb: "Your kit lives on your body, where your hands expect it.",
    hot: { L: ["grip"], R: ["grip"] },
    clip: null,
    rows: [
      ["Grip at buckle", "Draw access card — swipe readers"],
      ["Grip at pouch", "Pull a clip"],
      ["Reach over shoulder, release", "Stow in backpack"],
      ["Grip near other hand", "Pass an item across"],
      ["Swing + release", "Throw"],
    ],
  },
];

export const SHOTS = [
  ["rec1.gif", "Recreation · pistol"],
  ["ops2.gif", "Operations · psi amp + laser"],
  ["medsci1.png", "MedSci · wrench + pistol"],
];

export const LINKS = {
  release: "https://github.com/tommy-xr/shock2quest/releases/latest",
  repo: "https://github.com/tommy-xr/shock2quest",
  install: "https://github.com/tommy-xr/shock2quest/blob/main/INSTALL.md",
  twitter: "https://x.com/tommyxr85",
  gog: "https://www.gog.com/en/game/system_shockr_2_25th_anniversary_remaster",
  steam: "https://store.steampowered.com/app/866570/",
  // Last commit before coding agents were used (2025-06-20, #33).
  preAgent: "https://github.com/tommy-xr/shock2quest/tree/17d9950cd7187bf293907b14389122025c6679a3",
  preAgentShort: "17d9950",
};

const ICONS = {
  github: '<svg viewBox="0 0 16 16" aria-hidden="true"><path fill="currentColor" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>',
  x: '<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z"/></svg>',
};

// Header links: GitHub + X, as icon buttons.
export function socialLinks() {
  return `<a class="social" href="${LINKS.repo}" aria-label="GitHub">${ICONS.github}</a>` +
    `<a class="social" href="${LINKS.twitter}" aria-label="tommyxr85 on X">${ICONS.x}</a>`;
}

// Fill every [data-l] anchor from LINKS.
export function wireLinks() {
  document.querySelectorAll("[data-l]").forEach((a) => (a.href = LINKS[a.dataset.l]));
}

// Latest release tag, or the newest main commit while there is no release yet.
// Resolves to e.g. { label: "v0.3.0", date: "2026-09-20", url }, or null offline.
export async function fetchVersion() {
  const api = "https://api.github.com/repos/tommy-xr/shock2quest";
  try {
    const r = await fetch(`${api}/releases/latest`);
    if (r.ok) {
      const d = await r.json();
      return { label: d.tag_name, date: d.published_at.slice(0, 10), url: d.html_url };
    }
    const c = await fetch(`${api}/commits/main`);
    if (!c.ok) return null;
    const d = await c.json();
    return { label: `pre-release · main@${d.sha.slice(0, 7)}`, date: d.commit.committer.date.slice(0, 10), url: d.html_url };
  } catch {
    return null;
  }
}

export const DISCLOSURE = {
  fan: "shock2quest is an unofficial, fan-made project. It is not affiliated with or endorsed by Nightdive Studios or the System Shock rights holders, and needs your own copy of System Shock 2: 25th Anniversary Remaster.",
  ai: "Hacker tommy-xr used AI development tools to complete this project — noting it because some people are, understandably, opposed to the technology.",
};
