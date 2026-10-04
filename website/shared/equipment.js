// Captured in the game runtime; callout points use the same camera projection.
const section = document.querySelector('#equipment');
const video = section.querySelector('video');
const view = section.querySelector('.equipment-view');
const buttons = [...section.querySelectorAll('[data-equipment]')];
const playButton = section.querySelector('#equipment-play');
const notice = section.querySelector('.equipment-notice');
const motion = matchMedia('(prefers-reduced-motion: reduce)');
const mediaRoot = new URL('../../screenshots/equipment/', import.meta.url);
const subtitles = ['MULTI-FUNCTION DISPLAY', 'BELT-MOUNTED AMMUNITION', 'WEAPON RETENTION', 'WRIST-MOUNTED VITALS', 'SHOULDER ACCESS'];
let clips = [];
let selected = 0;
let visible = false;
let wantsPlayback = !motion.matches;
let loading;
let frame;

function message(text = '') {
  notice.textContent = text;
  notice.hidden = !text;
}

function drawCallout() {
  const points = clips[selected]?.keyframes;
  view.dataset.tracked = String(Boolean(points?.length));
  if (points?.length) {
    const time = video.currentTime;
    const next = points.findIndex(point => point.time > time);
    const a = next < 0 ? points.at(-1) : points[Math.max(0, next - 1)];
    const b = next < 0 ? a : points[next];
    const mix = b.time > a.time ? Math.max(0, (time - a.time) / (b.time - a.time)) : 0;
    const x = (a.x + (b.x - a.x) * mix) * 1000;
    const y = (a.y + (b.y - a.y) * mix) * 750;
    section.querySelector('.equipment-leader').setAttribute('d', `M 60 120 H 200 L ${x} ${y}`);
    for (const circle of section.querySelectorAll('.equipment-tracker circle')) {
      circle.setAttribute('cx', x);
      circle.setAttribute('cy', y);
    }
  }
  const progress = video.duration > 0 ? video.currentTime / video.duration : 0;
  section.querySelector('.equipment-progress span').style.transform = `scaleX(${progress})`;
}

function animate() {
  drawCallout();
  frame = requestAnimationFrame(animate);
}

function updatePlayback() {
  playButton.textContent = wantsPlayback ? 'Pause tour' : 'Play tour';
  cancelAnimationFrame(frame);
  if (!wantsPlayback || !visible || document.hidden || !video.getAttribute('src')) {
    video.pause();
    drawCallout();
    return;
  }
  // A selection or visibility change can interrupt a pending play request.
  const source = video.getAttribute('src');
  video.play().then(() => {
    if (!wantsPlayback || !visible || document.hidden) return video.pause();
    cancelAnimationFrame(frame);
    animate();
  }).catch(error => {
    if (error.name === 'AbortError' || source !== video.getAttribute('src')) return;
    wantsPlayback = false;
    playButton.textContent = 'Play tour';
    message('Tour paused. Select Play tour to start, or inspect the loadout below.');
  });
}

function select(index) {
  selected = index;
  video.pause();
  if (clips[index]) message();
  const id = buttons[index].dataset.equipment;
  const number = String(index + 1).padStart(2, '0');
  const title = buttons[index].querySelector('b').textContent;
  buttons.forEach((button, i) => button.setAttribute('aria-pressed', String(i === index)));
  section.querySelector('#equipment-position').textContent = `${number} / 05`;
  section.querySelector('.equipment-callout span').textContent = `${number} / ${title.toUpperCase()}`;
  section.querySelector('.equipment-callout small').textContent = subtitles[index];
  video.setAttribute('aria-label', `Hacker demonstrating ${title.toLowerCase()}`);
  video.poster = new URL(`${id}.png`, mediaRoot);
  // No video bytes are requested until the section enters the viewport.
  if (clips[index]) {
    video.src = new URL(clips[index].video, mediaRoot);
    video.load();
  }
  drawCallout();
  updatePlayback();
}

async function load() {
  if (loading) return loading;
  loading = (async () => {
    try {
      const response = await fetch(new URL('callouts.json', mediaRoot));
      if (!response.ok) throw new Error('Missing equipment manifest');
      const manifest = await response.json();
      clips = buttons.map(button => manifest.clips.find(clip => clip.id === button.dataset.equipment));
      if (clips.some(clip => !clip)) throw new Error('Incomplete equipment manifest');
      select(selected);
      section.querySelector('.equipment-transport').hidden = false;
    } catch {
      message('The animated tour is unavailable. Explore the loadout descriptions and stills.');
    }
  })();
  return loading;
}

buttons.forEach((button, index) => button.addEventListener('click', () => select(index)));
playButton.addEventListener('click', () => {
  wantsPlayback = !wantsPlayback;
  message();
  if (wantsPlayback && !video.getAttribute('src') && clips[selected]) {
    video.src = new URL(clips[selected].video, mediaRoot);
  }
  updatePlayback();
});
video.addEventListener('ended', () => {
  if (wantsPlayback) select((selected + 1) % buttons.length);
});
video.addEventListener('loadeddata', drawCallout);
video.addEventListener('timeupdate', drawCallout);
video.addEventListener('error', () => {
  wantsPlayback = false;
  updatePlayback();
  // Return to the selected still if a video cannot be decoded or fetched.
  video.removeAttribute('src');
  video.load();
  message('Video unavailable. You can still select each item to inspect its loadout.');
});
document.addEventListener('visibilitychange', updatePlayback);
motion.addEventListener('change', () => {
  if (motion.matches) wantsPlayback = false;
  updatePlayback();
});
new IntersectionObserver(entries => {
  visible = entries[0].isIntersecting;
  if (visible) load();
  updatePlayback();
}, { threshold: 0.1 }).observe(view);
