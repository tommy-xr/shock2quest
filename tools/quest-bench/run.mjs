#!/usr/bin/env node
import { spawn, execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import { adb, resolveSerial, stopApp, restoreProximityAutomation, missionSelection, restoreMissionSelection } from '../../.claude/skills/vr-device-loop/scripts/quest-device.mjs';
import { renderMarkdown } from '../../.claude/skills/oculus-profiling/scripts/quest-benchmark.mjs';

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
    if (sample.animations.length !== (fixture.spawns?.length ?? 0)) throw new Error('missing animated subjects');
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

export function runOrder(repeats, lighting) {
  return Array.from({ length: repeats }, (_, repeat) =>
    (lighting === 'both' ? (repeat % 2 ? ['on', 'off'] : ['off', 'on']) : [lighting])
      .map(mode => ({ repeat: repeat + 1, mode }))).flat();
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
    scene: { type: 'string', default: 'all' }, lighting: { type: 'string', default: 'both' },
    repeats: { type: 'string', default: '2' }, warmup: { type: 'string', default: '10' },
    seconds: { type: 'string', default: '30' }, output: { type: 'string' }, serial: { type: 'string' },
  }});
  if (!['off', 'on', 'both'].includes(values.lighting)) throw new Error('lighting must be off, on or both');
  for (const key of ['repeats', 'warmup', 'seconds']) {
    const minimum = key === 'warmup' ? 3 : key === 'seconds' ? 2 : 1;
    if (!/^\d+$/.test(values[key]) || Number(values[key]) < minimum) throw new Error(`invalid ${key}: minimum ${minimum}`);
  }
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
    os: adb(serial, ['shell', 'getprop', 'ro.build.fingerprint']),
    // Record the mounted remaster data, not just the APK. Old loose data may
    // coexist on the device but the runtime gives these archives precedence.
    assets: adb(serial, ['shell', 'sha256sum /sdcard/shock2quest/sshock2.kpf /sdcard/shock2quest/mods/*.kpf']),
  };
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
      for (const { repeat, mode } of runOrder(Number(values.repeats), values.lighting)) {
        if (cancellation.signal.aborted) throw new Error('benchmark interrupted');
        const fixture = { ...base, object_lighting: mode === 'on' };
        const directory = resolve(output, `${fixture.name}-${mode}-${repeat}`);
        mkdirSync(directory, { recursive: true });
        const localConfig = resolve(directory, 'fixture.json');
        writeFileSync(localConfig, JSON.stringify(fixture, null, 2) + '\n');
        const run = { scene: fixture.name, mode, repeat, directory, status: 'failed' };
        process.stdout.write(`Measuring ${fixture.name} lighting=${mode} repeat=${repeat}\n`);
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
          run.result = result;
          run.status = 'ok';
        } catch (error) {
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
            mission: `${run.scene} / ${run.mode} / ${run.repeat}`,
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
