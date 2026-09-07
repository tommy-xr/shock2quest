/** Optional exact developer port; ordinary fixtures let the OS choose. */
export function e2ePort(
  offset = 0,
  variable = "SHOCK2_E2E_PORT",
  env: NodeJS.ProcessEnv = process.env,
): number {
  const raw = env[variable];
  if (raw === undefined) return 0;
  const base = Number(raw);
  if (raw.trim() === "" || !Number.isInteger(base) || base < 0 || base > 65535) {
    throw new Error(`${variable} must be an integer port from 0 to 65535`);
  }
  // Zero explicitly requests OS allocation; adding an offset would turn it
  // into a fixed privileged port rather than another ephemeral launch.
  if (base === 0) return 0;
  const port = base + offset;
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error(`${variable} plus fixture offset is outside 1 to 65535`);
  }
  return port;
}
