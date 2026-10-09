#!/usr/bin/env node
import { spawn, execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import { adb, resolveSerial, stopApp, restoreProximityAutomation, missionSelection, restoreMissionSelection } from '../../.claude/skills/vr-device-loop/scripts/quest-device.mjs';
import { renderMarkdown, summarize } from '../../.claude/skills/oculus-profiling/scripts/quest-benchmark.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const configPath = '/sdcard/shock2quest/benchmark-scene.json';

export function validateWorkload(text, fixture, seconds) {
  // Runtime emits workload immediately after its rendered-frame PERF report.
  // Select the same PERF tail as parseEngineTelemetry, then require a paired
  // workload for each bucket. Skipped-frame reports have no workload; older
  // buffered warmup records must never fill those holes.
  const reports = [];
  for (const line of text.split('\n')) {
    if (line.includes('SHOCK2QUEST_PERF')) reports.push({ workload: null });
    else if (line.startsWith('SHOCK2QUEST_BENCHMARK ') && reports.length) {
      const report = reports.at(-1);
      if (report.workload) throw new Error('duplicate workload record for a timing report');
      report.workload = JSON.parse(line.slice('SHOCK2QUEST_BENCHMARK '.length));
    }
  }
  const samples = reports.slice(-seconds).map(report => report.workload);
  if (samples.length !== seconds || samples.some(sample => !sample)) {
    throw new Error('measured timing interval lacks matching workload evidence');
  }
  for (const sample of samples) {
    if (!sample.setup_complete || sample.name !== fixture.name || sample.object_lighting !== fixture.object_lighting ||
        sample.subject_meshes !== fixture.expected_subject_meshes ||
        sample.lit_subject_meshes !== (fixture.object_lighting ? fixture.expected_subject_meshes : 0)) {
      throw new Error(`workload mismatch: ${JSON.stringify(sample)}`);
    }
    if ((sample.upgraded_terrain ?? false) !== (fixture.upgraded_terrain ?? false) ||
        (sample.terrain_wetness ?? 0) !== (fixture.terrain_wetness ?? 0)) throw new Error('terrain mode mismatch');
    if (sample.ffr_requested !== (fixture.ffr ?? 'off') || sample.ffr_effective !== (fixture.ffr ?? 'off')) {
      throw new Error('FFR requested/effective mismatch or missing evidence');
    }
    const groups = sample.additional_subjects ?? [];
    if (groups.length !== (fixture.additional_subjects?.length ?? 0)) throw new Error('missing model groups');
    for (const expected of fixture.additional_subjects ?? []) {
      const group = groups.find(group => group.model === expected.model);
      if (!group || group.meshes !== expected.expected_meshes ||
          group.lit_meshes !== (fixture.object_lighting ? expected.expected_meshes : 0)) {
        throw new Error(`mixed subject mismatch: ${JSON.stringify(groups)}`);
      }
    }
    if (sample.animations.length !== (fixture.spawns ?? []).filter(spawn => spawn.animated !== false).length) throw new Error('missing animated subjects');
    if (!Array.isArray(sample.lamp_intensities) ||
        sample.lamp_intensities.length !== (fixture.light_templates?.length ?? 0) ||
        sample.lamp_intensities.some(intensity => intensity !== (fixture.lights_on ? 1 : 0))) {
      throw new Error(`authored lamp state mismatch: ${JSON.stringify(sample.lamp_intensities)}`);
    }
  }
  for (const subject of samples[0].animations) {
    const states = samples.map(sample => sample.animations.find(a => a.entity === subject.entity));
    const window = Math.min(5, seconds);
    for (let start = 0; start <= states.length - window; start++) {
      const bucket = states.slice(start, start + window);
      if (bucket.some(state => !state) || new Set(bucket.map(state => `${state.clip}:${state.frame}`)).size < 2) {
        throw new Error(`subject ${subject.entity} stopped animating near sample ${start}`);
      }
    }
  }
  return samples;
}

export function validateCpuProfile(text, seconds) {
  const reports = [];
  let pending = null;
  for (const line of text.split('\n')) {
    if (line.startsWith('SHOCK2QUEST_CPU_PROFILE ')) pending = JSON.parse(line.slice('SHOCK2QUEST_CPU_PROFILE '.length));
    else if (line.includes('SHOCK2QUEST_PERF')) { reports.push(pending); pending = null; }
  }
  const samples = reports.slice(-seconds);
  if (samples.length !== seconds || samples.some(sample => !sample || sample.cpu_frames <= 0 ||
      !sample.phases?.game_update || sample.cpu_histogram_100us.reduce((sum, [, count]) => sum + count, 0) !== sample.cpu_frames)) {
    throw new Error('measured interval lacks complete CPU/clock diagnostics');
  }
  return samples;
}

export function runOrder(repeats, lighting) {
  return Array.from({ length: repeats }, (_, repeat) =>
    (lighting === 'both' ? (repeat % 2 ? ['on', 'off'] : ['off', 'on']) : [lighting])
      .map(mode => ({ repeat: repeat + 1, mode }))).flat();
}

export function terrainRuns(repeats, lighting, terrain) {
  return runOrder(repeats, lighting).flatMap(run => {
    const modes = terrain === 'all' ? ['classic', 'upgraded', 'wet'] : [terrain];
    if (run.repeat % 2 === 0) modes.reverse();
    return modes.map(terrain => ({ ...run, terrain }));
  });
}

export function ffrRuns(repeats, lighting, terrain, ffr) {
  // Balanced Latin square: each level occupies each position once across four
  // repeats, with every ordered adjacent pair represented once.
  const orders = [
    ['off', 'low', 'high', 'medium'], ['low', 'medium', 'off', 'high'],
    ['medium', 'high', 'low', 'off'], ['high', 'off', 'medium', 'low'],
  ];
  return terrainRuns(repeats, lighting, terrain).flatMap(run =>
    (ffr === 'all' ? orders[(run.repeat - 1) % 4] : [ffr]).map(ffr => ({ ...run, ffr })));
}

export function validateFoveation(vrapi, level) {
  const expected = ['off', 'low', 'medium', 'high'].indexOf(level);
  if (expected < 0 || vrapi?.foveation_level?.min !== expected ||
      vrapi?.foveation_level?.max !== expected || vrapi?.dynamic_foveation_samples !== 0) {
    throw new Error(`VrApi did not confirm fixed FFR ${level}`);
  }
}

export function terrainFixture(base, mode) {
  if (mode === 'fixture') return base;
  return { ...base, upgraded_terrain: mode !== 'classic', terrain_wetness: mode === 'wet' ? 1.5 : 0 };
}

export function parseGpuCounters(text) {
  const counters = {};
  for (const line of text.split('\n')) {
    const match = line.trim().match(/^(.+?)\s*:\s*([0-9.eE+-]+)$/);
    if (match && Number.isFinite(Number(match[2]))) {
      const value = Number(match[2]);
      // The selected utilization/work counters cannot be negative. The driver
      // emits -1 when a counter is unavailable; never average that as data.
      if (value < 0) throw new Error(`invalid GPU counter ${match[1].trim()}: ${value}`);
      (counters[match[1].trim()] ??= []).push(value);
    }
  }
  return Object.fromEntries(Object.entries(counters).map(([name, values]) => [name, summarize(values)]));
}

function child(args, log, cancellation) {
  return new Promise((resolveChild, reject) => {
    const childProcess = spawn(process.execPath, args, { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] });
    const cancel = () => childProcess.kill('SIGTERM');
    cancellation.addEventListener('abort', cancel, { once: true });
    let output = '';
    childProcess.stdout.on('data', chunk => { output += chunk; });
    childProcess.stderr.on('data', chunk => { output += chunk; });
    childProcess.on('error', reject);
    childProcess.on('close', code => {
      cancellation.removeEventListener('abort', cancel);
      writeFileSync(log, output);
      code === 0 ? resolveChild() : reject(new Error(`profiler exited ${code}; see ${log}`));
    });
  });
}

async function main(argv) {
  const { values } = parseArgs({ args: argv, options: {
    'gpu-seconds': { type: 'string', default: '0' },
    terrain: { type: 'string', default: 'fixture' },
    ffr: { type: 'string', default: 'fixture' },
    'profile-cpu': { type: 'boolean', default: false },
    scene: { type: 'string', default: 'all' }, lighting: { type: 'string', default: 'both' },
    repeats: { type: 'string', default: '2' }, warmup: { type: 'string', default: '10' },
    seconds: { type: 'string', default: '30' }, output: { type: 'string' }, serial: { type: 'string' },
  }});
  if (!['fixture', 'off', 'low', 'medium', 'high', 'all'].includes(values.ffr)) throw new Error('invalid FFR level');
  if (!['fixture', 'classic', 'upgraded', 'wet', 'all'].includes(values.terrain)) throw new Error('invalid terrain variant');
  if (!['off', 'on', 'both'].includes(values.lighting)) throw new Error('lighting must be off, on or both');
  for (const key of ['repeats', 'warmup', 'seconds']) {
    const minimum = key === 'warmup' ? 3 : key === 'seconds' ? 2 : 1;
    if (!/^\d+$/.test(values[key]) || Number(values[key]) < minimum) throw new Error(`invalid ${key}: minimum ${minimum}`);
  }
  if (!/^\d+$/.test(values['gpu-seconds']) || Number(values['gpu-seconds']) > 30) throw new Error('gpu-seconds must be 0..30');
  const sceneDir = resolve(root, 'benchmarks/scenes');
  const files = readdirSync(sceneDir).filter(name => name.endsWith('.json') &&
    (values.scene === 'all' || name === `${values.scene}.json`)).sort();
  if (!files.length) throw new Error('unknown scene');
  const serial = resolveSerial(values.serial);
  const output = resolve(values.output ?? `/tmp/quest-bench-${Date.now()}`);
  mkdirSync(output, { recursive: true });
  // Fail on connection/read errors instead of mistaking them for an absent file.
  const exists = adb(serial, ['shell', `if [ -f ${configPath} ]; then echo yes; else echo no; fi`]) === 'yes';
  const previous = exists ? adb(serial, ['exec-out', 'cat', configPath], { encoding: null }) : null;
  if (previous) writeFileSync(resolve(output, 'previous-benchmark-scene.json'), previous);
  const bufferInfo = adb(serial, ['logcat', '-b', 'main', '-g']);
  const bufferMatch = bufferInfo.match(/ring buffer is (\d+) (KiB|MiB)/);
  if (!bufferMatch) throw new Error(`cannot parse logcat buffer size: ${bufferInfo}`);
  const oldBuffer = `${bufferMatch[1]}${bufferMatch[2] === 'MiB' ? 'M' : 'K'}`;
  const metadata = {
    commit: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
    dirty: !!execFileSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).trim(),
    options: values,
    foveationOverrides: Object.fromEntries(['level', 'dynamic'].map(key =>
      [key, adb(serial, ['shell', 'getprop', `debug.oculus.foveation.${key}`])])),
    os: adb(serial, ['shell', 'getprop', 'ro.build.fingerprint']),
    // Record the mounted remaster data, not just the APK. Old loose data may
    // coexist on the device but the runtime gives these archives precedence.
    assets: adb(serial, ['shell', 'sha256sum /sdcard/shock2quest/sshock2.kpf /sdcard/shock2quest/mods/*.kpf']),
  };
  if (Object.values(metadata.foveationOverrides).some(value => value !== '')) {
    throw new Error('global foveation overrides are set; clear them before benchmarking application profiles');
  }
  // Counter indices vary between OS versions; resolve the device's own list.
  const gpuMetrics = Number(values['gpu-seconds']) ? adb(serial, ['shell', 'ovrgpuprofiler', '-m']) : '';
  const gpuNames = ['GPU % Utilization', '% Time Shading Vertices', '% Time Shading Fragments',
    '% Shaders Busy', '% Shader ALU Capacity Utilized', '% Wave Context Occupancy',
    '% Texture Fetch Stall', '% Texture L1 Miss', '% Texture L2 Miss', '% Texture Pipes Busy',
    'Vertices Shaded / Second', 'Fragments Shaded / Second', 'Textures / Fragment'];
  const gpuIds = gpuNames.map(name => gpuMetrics.split('\n').map(line => line.trim().match(/^(\d+)\s+(.+)$/))
    .find(match => match?.[2] === name)?.[1]);
  if (gpuMetrics && gpuIds.some(id => !id)) throw new Error('device lacks required GPU counters');
  if (gpuMetrics) writeFileSync(resolve(output, 'gpu-metrics.txt'), gpuMetrics);
  const runs = [];
  const previousMission = missionSelection(serial);
  const cancellation = new AbortController();
  const cancel = () => cancellation.abort();
  process.on('SIGINT', cancel);
  process.on('SIGTERM', cancel);
  try {
    // Broad Android logging otherwise evicts READY/FOCUSED during warmup.
    adb(serial, ['logcat', '-b', 'main', '-G', '16M']);
    for (const file of files) {
      const base = JSON.parse(readFileSync(resolve(sceneDir, file), 'utf8'));
      for (const { repeat, mode, terrain, ffr } of ffrRuns(Number(values.repeats), values.lighting, values.terrain, values.ffr)) {
        if (cancellation.signal.aborted) throw new Error('benchmark interrupted');
        const fixture = terrainFixture({ ...base, object_lighting: mode === 'on' }, terrain);
        fixture.ffr = ffr === 'fixture' ? (fixture.ffr ?? 'off') : ffr;
        fixture.profile_cpu = values['profile-cpu'] || (fixture.profile_cpu ?? false);
        const directory = resolve(output, `${fixture.name}-${mode}-${terrain}-ffr-${fixture.ffr}-${repeat}`);
        mkdirSync(directory, { recursive: true });
        const localConfig = resolve(directory, 'fixture.json');
        writeFileSync(localConfig, JSON.stringify(fixture, null, 2) + '\n');
        const run = { scene: fixture.name, mode, terrain, ffr: fixture.ffr, repeat, directory, status: 'failed' };
        process.stdout.write(`Measuring ${fixture.name} lighting=${mode} terrain=${terrain} ffr=${fixture.ffr} repeat=${repeat}\n`);
        try {
          adb(serial, ['push', localConfig, configPath]);
          writeFileSync(resolve(directory, 'battery-before.txt'), adb(serial, ['shell', 'dumpsys', 'battery']));
          await child([resolve(root, '.claude/skills/oculus-profiling/scripts/quest-benchmark.mjs'),
            '--serial', serial, '--mission', fixture.mission, '--warmup', values.warmup,
            '--seconds', values.seconds, '--keep-awake', '--output', directory], resolve(directory, 'profiler.log'), cancellation.signal);
          const result = JSON.parse(readFileSync(resolve(directory, 'results.json'), 'utf8')).results[0];
          if (result.status !== 'ok') throw new Error(result.error ?? result.visual_error ?? result.status);
          const telemetry = readFileSync(resolve(directory, `${fixture.mission.replace(/[^A-Za-z0-9_-]/g, '_')}.telemetry.log`), 'utf8');
          run.workload = validateWorkload(telemetry, fixture, Number(values.seconds));
          validateFoveation(result.vrapi, fixture.ffr);
          if (fixture.profile_cpu) run.cpu_profile = validateCpuProfile(telemetry, Number(values.seconds));
          run.result = result;
          run.status = 'ok';
          if (gpuMetrics) {
            // Separate from timing samples: counters can perturb rendering.
            // Android timeout bounds the remote process even if the host exits.
            let counters;
            try {
              counters = adb(serial, ['shell', '-tt', `timeout -s INT ${values['gpu-seconds']} ovrgpuprofiler --realtime="${gpuIds.join(',')}"`]);
            } catch (error) {
              if (error.status !== 124) throw error;
              counters = error.stdout?.toString() ?? '';
            }
            writeFileSync(resolve(directory, 'gpu-counters.txt'), counters);
            run.gpu = parseGpuCounters(counters);
            if (gpuNames.some(name => !run.gpu[name])) throw new Error('GPU profiler returned incomplete counters');
          }
        } catch (error) {
          run.status = 'failed';
          run.error = error.message;
          process.stderr.write(`Excluded: ${run.error}\n`);
        } finally {
          try {
            writeFileSync(resolve(directory, 'battery-after.txt'), adb(serial, ['shell', 'dumpsys', 'battery']));
          } catch (error) {
            run.battery_error = error.message;
            run.status = 'failed';
          }
          runs.push(run);
          writeFileSync(resolve(output, 'results.json'), JSON.stringify({ metadata, runs }, null, 2) + '\n');
          const report = renderMarkdown(runs.map(run => ({
            ...run.result, status: run.status, error: run.error ?? run.battery_error,
            mission: `${run.scene} / ${run.mode} / ${run.terrain} / FFR ${run.ffr} / ${run.repeat}`,
          })), {
            ...runs.find(run => run.result)?.result.device,
            warmup: Number(values.warmup), seconds: Number(values.seconds),
          });
          writeFileSync(resolve(output, 'report.md'), report.replace('# Quest mission baseline', '# Quest benchmark scenes'));
        }
      }
    }
  } finally {
    process.removeListener('SIGINT', cancel);
    process.removeListener('SIGTERM', cancel);
    try { stopApp(serial); } finally {
      try {
        if (previous) adb(serial, ['push', resolve(output, 'previous-benchmark-scene.json'), configPath]);
        else adb(serial, ['shell', 'rm', '-f', configPath]);
      } finally {
        try { restoreMissionSelection(serial, previousMission); }
        finally {
          try { adb(serial, ['logcat', '-b', 'main', '-G', oldBuffer]); }
          finally { restoreProximityAutomation(serial); }
        }
      }
    }
  }
  process.stdout.write(`Results: ${output}/results.json\n`);
  if (cancellation.signal.aborted || runs.some(run => run.status !== 'ok')) process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).catch(error => { console.error(error); process.exitCode = 1; });
}
