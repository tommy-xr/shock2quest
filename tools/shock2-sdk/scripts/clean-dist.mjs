// TypeScript does not remove outputs for deleted or renamed source files.
// Clean the package output before compiling so test globs cannot run old tests.
import { rmSync } from "node:fs";

rmSync(new URL("../dist/", import.meta.url), { recursive: true, force: true });
