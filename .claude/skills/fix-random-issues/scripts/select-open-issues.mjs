#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";

function usage() {
  console.log(`Usage:
  node select-open-issues.mjs <count> [--repo OWNER/REPO] [--seed VALUE]

Randomly select a fixed sample of open GitHub issues. Output is JSON.

Options:
  --repo OWNER/REPO  Repository to query (default: current gh repository)
  --seed VALUE       Reproducible shuffle seed (default: random 128-bit hex)
  -h, --help         Show this help`);
}

function fail(message) {
  console.error(`select-open-issues: ${message}`);
  process.exit(1);
}

function parseArgs(argv) {
  let count;
  let repo;
  let seed;

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "-h" || argument === "--help") {
      usage();
      process.exit(0);
    }
    if (argument === "--repo" || argument === "--seed") {
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) {
        fail(`${argument} requires a value`);
      }
      if (argument === "--repo") {
        repo = value;
      } else {
        seed = value;
      }
      index += 1;
      continue;
    }
    if (argument.startsWith("--repo=")) {
      repo = argument.slice("--repo=".length);
      continue;
    }
    if (argument.startsWith("--seed=")) {
      seed = argument.slice("--seed=".length);
      continue;
    }
    if (argument.startsWith("-")) {
      fail(`unknown option: ${argument}`);
    }
    if (count !== undefined) {
      fail(`unexpected positional argument: ${argument}`);
    }
    count = Number(argument);
  }

  if (!Number.isSafeInteger(count) || count < 1) {
    fail("count must be a positive integer");
  }
  if (repo !== undefined && !/^[^/\s]+\/[^/\s]+$/.test(repo)) {
    fail("repo must have the form OWNER/REPO");
  }
  if (seed === "") {
    fail("seed must not be empty");
  }

  return { count, repo, seed };
}

function runGh(args) {
  try {
    return execFileSync("gh", args, {
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  } catch (error) {
    const detail = error.stderr?.trim() || error.message;
    fail(`gh ${args[0]} failed: ${detail}`);
  }
}

function resolveRepository(explicitRepository) {
  if (explicitRepository) {
    return explicitRepository;
  }
  const repository = runGh([
    "repo",
    "view",
    "--json",
    "nameWithOwner",
    "--jq",
    ".nameWithOwner",
  ]);
  if (!repository) {
    fail("could not resolve the current GitHub repository");
  }
  return repository;
}

function hashSeed(value) {
  let hash = 0x811c9dc5;
  for (const byte of Buffer.from(value, "utf8")) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

function seededRandom(seed) {
  let state = hashSeed(seed);
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
}

function shuffle(values, random) {
  const result = [...values];
  for (let index = result.length - 1; index > 0; index -= 1) {
    const other = Math.floor(random() * (index + 1));
    [result[index], result[other]] = [result[other], result[index]];
  }
  return result;
}

function listOpenIssues(repository) {
  const endpoint = `repos/${repository}/issues?state=open&per_page=100`;
  const raw = runGh(["api", "--paginate", "--slurp", endpoint]);
  let pages;
  try {
    pages = JSON.parse(raw);
  } catch (error) {
    fail(`could not parse GitHub response: ${error.message}`);
  }

  if (!Array.isArray(pages)) {
    fail("GitHub response was not a paginated issue list");
  }

  return pages
    .flat()
    .filter((issue) => !issue.pull_request)
    .map((issue) => ({
      number: issue.number,
      title: issue.title,
      url: issue.html_url,
      labels: (issue.labels ?? []).map((label) =>
        typeof label === "string" ? label : label.name
      ),
      assignees: (issue.assignees ?? []).map((assignee) => assignee.login),
    }))
    .sort((left, right) => left.number - right.number);
}

const options = parseArgs(process.argv.slice(2));
const repository = resolveRepository(options.repo);
const seed = options.seed ?? randomBytes(16).toString("hex");
const openIssues = listOpenIssues(repository);
const selected = shuffle(openIssues, seededRandom(seed)).slice(0, options.count);

console.log(
  JSON.stringify(
    {
      repository,
      seed,
      requested: options.count,
      available: openIssues.length,
      sampled: selected.length,
      selected,
    },
    null,
    2
  )
);
